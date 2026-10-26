# Ross-Tech HEX-V2 wire protocol — working notes

Source: static analysis (Ghidra) of the VCDS 26.9 binaries on the author's own machine, cross-checked
against a HEX-V2 frame capture observed from VCIConfig during the analysis session. Purpose:
interoperability with a device the author owns. Licensing / authentication logic was deliberately
**not** analysed.

> Note on the capture: the example frames quoted below (e.g. `53 04 14 43`, `04 0A 01 00 00 17 …`)
> were observed in VCIConfig's `log.txt` during analysis; VCIConfig rotates/overwrites that file, so
> the current on-disk copy may not contain them. Each quoted frame is self-consistent (valid trailing
> XOR), and every frame **structure** below is independently confirmed in the decompiled code — the
> code, not the log, is the primary evidence.

Legend: **[C]** confirmed in decompiled code, **[I]** inferred, **[?]** unknown.

## 1. USB / HID  [C]

- HEX-V2 enumerates as a USB HID device, `VID 0x0483`, `PID 0xA0CB`. VCDS also probes `PID 0xA00F`
  first (not identified), then falls back to the legacy FTDI path (`rt-usb.dll`, PIDs 0xFA20–FA25
  — a different, older family; not HEX-V2).
- VCDS uses the Windows HID API directly: `SetupDi*` enumeration → `HidD_GetAttributes` (match VID/PID)
  → two overlapped handles (write `GENERIC_WRITE`, read `GENERIC_READ`, `FILE_FLAG_OVERLAPPED`).
  Report buffer size comes from `HidP_GetCaps` (observed: 64 bytes). A dedicated thread keeps an
  overlapped `ReadFile` outstanding and queues up to 127 reassembled messages (255 bytes each).
- VCIConfig uses hidapi (`hid_open_path`, `hid_write`, `hid_read_timeout`); same wire format.

## 2. Layer 0 — HID report  [C]

64-byte reports, always report ID 1, zero padded:

```
byte 0   0x01          report id
byte 1   n             valid bytes in this report (<= report_size - 4 = 60)
byte 2   idx           1-based fragment index
byte 3   total         number of fragments
byte 4.. data[n]       then zero padding to 64
```

A message longer than 60 bytes is split across reports; the receiver appends fragment data until
`idx == total`. (VCDS send: `FUN_004d37f3`; receive thread: `FUN_004d3d0e`; VCIConfig send: `FUN_0046c5a6`.)
VCIConfig's reader copies a fixed 60 bytes per report and stops on `idx == total`; VCDS honours `n`.

## 3. Layer 1 — two frame types inside the data  [C]

| First byte | Type | Layout |
|---|---|---|
| `0x04` | "CB packet" (VCDS diagnostic traffic) | `04 | L | payload[L-3] | xor` — `L = payload_len + 3`, `xor` = XOR of every preceding byte of the frame |
| `0x53` ('S') | command request — the HC K-line/CAN command transport, and VCIConfig device-mgmt | `53 | len | cmd | args… | xor` — `len` = total frame length, `xor` = XOR of all preceding bytes |
| `0x4D` ('M') | command reply | `4D | len | cmd | status/data… | xor` — same XOR rule |

All three frame layouts are confirmed in code: `04` in `FUN_0045a9fa`; `53` build in
`FUN_00452ea9`; `4D` parse + XOR-verify in `FUN_00452f6a`. The example frames (`53 04 14 43`,
`04 0A 01 00 00 17 00 00 00 18`, reply `4D 0E 14 02 43 00 18 00 21 00 02 11 00 3C`) are from the
observed capture and are each XOR-self-consistent, but are not re-checkable against the current
rotated `log.txt`. VCDS also has an encrypted CB-packet variant (flag at object+0x5CBC); **not
analysed**. Note `0x53` is not only "device management" — it is also how HC commands (section 5)
reach the interface.

The same CB-frame layer is used over TCP for HEX-NET (interface type 7); HEX-V2 is interface type 8.

## 4. Layer 2 — message inside a CB frame  [C]

```
byte 0   seq           sender sequence byte (object+0x1A8)
byte 1   0x00
byte 2-3 cmd           big-endian command id
byte 4-5 total_len     big-endian, total payload length of the whole message
byte 6   chunk_len     bytes of data in this CB packet
byte 7.. data[chunk_len]
```

Built by `FUN_0045935c(cmd, data, len)`. If `len` exceeds the per-packet limit (219 normally, 43 when
`DAT_00ac300c == 5`) it is sent as several CB packets with the same header.
Example from the log: `04 0A | 01 00 00 17 00 00 00 | 18` = seq 1, cmd 0x0017, no data.

Observed message command ids: `0x0001` (poll/ack request?), `0x0005` (diagnostic passthrough),
`0x0008` (ping — logged as "HN Pingstamp"), `0x01FB` (2-byte toggle, set when a 0x28 service is
sent), `0x0017` and `0x0020` (zero-length queries issued by VCIConfig).  **[?]** exact meaning of
all but 0x0005.

Reply handling (`FUN_004596b2`): reply `cmd` 1 = ack, 4 = error (error code in first data byte;
`05` aborts), 6 = data, 7 = data variant, 8 = pong. The expected reply cmd is matched; non-matching
packets are discarded or buffered (up to 8). **[I]** for the exact field positions on replies.

## 5. Legacy command buffer carried in cmd 0x0005  [C for the listed opcodes]

The `HC::` class (HEX-COM) builds `[len][opcode][args…]` buffers (`len` counts the bytes after it) and
sends the bytes after `len` as the data of message cmd 0x0005 (`FUN_004595a7`). A reply starting
`0xFE` is success for `FastInit`.

| Method | Buffer | Notes |
|---|---|---|
| GetVersion | `01 02` | |
| KLineTest | `01 82` | |
| TurboBaud | `05 85 b0 b1 b2 b3` | baud rate, little-endian 32 bit |
| FastInit | `0C 98 01 a b 01 00 00 C1 33 F1 81 66` | opcode 0x98 + a KWP2000 StartCommunication message (`C1 33 F1 81`, checksum `66`); `a`,`b` are caller parameters **[?]** meaning |
| SetRcvFilter | `06 B4 p0 p1 p2 p3 00` | CAN receive filter (4 bytes) |
| ReadBoot | `01 0D` | |
| GCr | `01 17` | |
| GCp | `02 D5 04` | |
| Other (not yet decoded) | SetBoot, SetRcvMask, Reset, GC, SCr, SendCommand | see `re-work/vcds.hc.c` |

`HC::CANOpenCtrl` and `HC::*` calls go through virtual methods at vtable +0x88 (send) and +0x8C
(transact).

## 6. Command envelopes and payload masking  [C]

There are two low-level command builders, with **different envelopes**:

- **`HC::` / `uCh::SendCommand`** (`FUN_00452ea9`, `FUN_004607f3`): `53 | len+3 | payload | xor`.
  Plaintext, `0x53`-framed. This is the channel that also carries CAN frames.
- **`KC::SendCommand`** (`FUN_0046ae3f`): `len+2 | payload | xor`, **no `0x53` prefix**, and then
  **every byte XOR-masked with a single key byte** held at object+0x1A0.

The CAN transmit path masks its buffer with a **16-byte key array** at object+0x9A5, and
`KC::SendCommand` masks with a **1-byte key** at object+0x1A0.

**Where the 16-byte CAN key comes from [C]:** `FUN_004610a4` performs a challenge/response at CAN
open:

1. `FUN_004bc031(buf, 0x18)` fills 24 bytes with a PRNG nonce — a 53-iteration additive lagged
   generator (`next = a + b & 0x3FFFFFFF`), seeded from `QueryPerformanceCounter`, output byte
   `(i & 0xF) * qpc + state`. No embedded secret.
2. `FUN_004c2ecf(buf, 0x10, 4)` encrypts the first 16 bytes (2 × 64-bit blocks) with **TEA**
   (Wheeler–Needham, **not** XTEA — see note), **32 rounds**, delta `0x9E3779B9` (added as `sum`,
   stepped by `-0x61C88647` in the decompile), using **key-schedule index 4**:
   `k = {0xAAECA993, 0x6189CB86, 0xC2F51929, 0x7645225E}`. Five schedules (0–4) are embedded; this
   path uses index 4.
3. It sends command `1B 50 02 10 <16 encrypted bytes> …` — **opcode `0x50`**, and waits for the
   device reply (`0xFE` ack).
4. The **16 bytes of the device's reply are copied verbatim into object+0x9A5** and used as the CAN
   XOR mask. A 64-bit timestamp counter is stored at +0x27C.

> **Cipher is TEA, not XTEA.** The round is
> `v0 += ((v1>>5)+k1) ^ ((v1<<4)+k0) ^ (v1+sum)` / `v1 += ((v0>>5)+k3) ^ ((v0<<4)+k2) ^ (v0+sum)`,
> `sum += delta`, with the **four key words fixed for all rounds**. XTEA's defining trait (selecting
> a key word by `sum&3` / `(sum>>11)&3` each round) is absent — so a stock XTEA routine will produce
> the wrong ciphertext and fail the `0x50` exchange. Implement plain 32-round TEA.

So the CAN channel is a session-obfuscated link: the client encrypts a nonce with the embedded TEA
key, and the device returns the per-session mask. A from-scratch client must (a) do the `0x50`
exchange with TEA key #4 and (b) XOR every CAN command/response with the returned mask. The 1-byte
K-line mask at +0x1A0 is only on the CAN-scan/bus-probe path (KC class, 2 callers), **not** the
mainline K-line diagnostic channel (plaintext HC), so a K-line client can ignore it. This is
device-interoperability material for hardware the author owns; it is not a license key.

> A **second** TEA challenge/response exists that this path doesn't use: `FUN_004542b3` sends opcode
> `0xB6` (`1A B6 02`), encrypts with key-schedule **index 3**, and stores the 16-byte reply at
> +0x89BF; index 0 is used elsewhere. So "index 4" is specific to the CAN `0x50` path, not device-wide.

## 7. CAN and K-line opcodes  [C for the bytes, [?] where noted]

Transmit/transact over vtable +0x88 (send) / +0x8C (receive-match). Reply first byte convention:
`0xFE` = ack, `0x53` ('S') = async status packet (dispatched by `FUN_00462807`), `0xB9` = CAN error
(error code in reply byte 1), `0xFF` = non-ack / retry.

### CAN transmit — `HC::SCM` / SendCANMsg (`FUN_0046122a`), opcode `0x11`

16-byte message, built then XOR-masked with the +0x9A5 key:

```
off 0   0x11          opcode
off 1   0x53
        -- the 16 bytes from off 2 onward are the XOR-masked region (key at object+0x9A5) --
off 2   0x00
        -- CAN id, packed from arg (bit0 of flags arg = extended/29-bit) --
off 3   id & 0xFF                              (standard 11-bit low byte)
off 4   ((id >> 8) & 7) | (extId << 3)
off 5   extId >> 5
off 6   flags: bit6 (0x40) always set; bit7 (0x80) set when 29-bit extended
off 7   length/DLC byte
off 8.. data bytes
off 16  PC low   ) 16-bit per-command counter (object+0x27A), incremented on each ack;
off 17  PC high  ) a NACK triggers a resend with PC+1
```

Offsets are approximate (read `FUN_0046122a` at `abStack_6e[...]` for the exact field positions; the
masked region is the 16 bytes starting at off 2, i.e. `abStack_6e[0..15]`). For 11-bit:
`id = arg & 0x7FF`. For 29-bit: the value is split across off3–5 and bit7 of off6. On NACK (`0xFF`)
VCDS resends with PC+1 ("Resending with PC=%d").

### CAN acceptance filter / message object — `uCh::SetMsgObj` (`FUN_00461195`), opcode `0x52`

```
0B 52 m0 m1 m2 m3 i0 i1 i2 i3 07
```
`len=0x0B`, 4-byte mask, 4-byte id/match, trailing `0x07`. Reply `0xFE` = ack.
(`HC::SetRcvFilter` `06 B4 p0 p1 p2 p3 00` is the older/alternate 4-byte filter form.)

### CAN receive enable — `KC::ScrCAN` (`FUN_0046b835`)

Builds a config byte (OR of CAN-mode flag bits, `FUN_0046b7c0`) and sends it; ack reply byte is
`0x8C`. Puts the interface into CAN receive/scan mode. Retried up to ~2.5 s.

### Bus-presence test — `KC::KLineTest` (`FUN_0046d085`), opcode `0x74`

Sends `01 74` (via `KC::SendCommand`, so masked). Response status byte:
bits[1:0] = K-line line state, bits[3:2] = CAN state (logged as `0xD2`/`0xD4`/`0xD3` for K-line,
`0xD5`/`0xD7`/`0xD6` for CAN). Used to detect which buses the connected vehicle exposes.

### K-line init

Two inits, both via the HC channel:

- **Slow / address init (KW1281 + KWP slow)** — `FUN_00452b94`, opcode **`0x84`**:
  buffer `04 84 <addr> <mode> 03`. `<addr>` is the 7-bit module address with **bit 7 set for odd
  parity** (VCDS counts the parity of the address; address `0x33` is left as-is). `<mode>` comes from
  `DAT_005CF9F4`. Reply `0x84` = success, with the two **keyword (KW) bytes** in reply bytes 2–3
  (VCDS logs `KW: %02X%02X`); `04 04` keyword pair marks a KWP-2000-capable ECU.
- **Fast init (KWP2000)** — `HC::FastInit`, opcode **`0x98`** carrying StartCommunication
  `C1 33 F1 81 66` (section 5).
- **`HC::TurboBaud`** (`0x85`) then raises the K-line baud after init.

After init, KW1281 byte-level traffic (block counter, complement-ack `0x09 → 0xF6`, block end `0x03`)
rides inside the HC command channel; VCDS drives the block protocol in software and the interface
relays bytes. The complement/ack and block-end handling is visible in `FUN_004596b2` (reply cmd 7
branch) but the full KW1281 block loop was not transcribed here — it is standard KW1281.

## 8. Implementation blueprint (theoretical — build, then verify with the device later)

A layered client, bottom to top:

1. **HID layer.** Open `0483:A0CB`. Writer: split the message into 60-byte chunks and emit reports
   `01 n idx total <data>` padded to 64. Reader: an overlapped/looping read that concatenates chunk
   `data` until `idx == total` → one assembled message.
2. **Frame layer.** Wrap an assembled payload as a CB frame `04 | len | payload | xor`
   (len = payload+3, xor over all preceding bytes). Parse replies symmetrically; `53/4D` frames are
   the device-management side-channel (firmware/serial), not needed for bus I/O.
3. **Message layer.** Header `seq 00 cmd16 totlen16 chunklen <data>`. Diagnostic traffic uses
   `cmd = 0x0005`. Match replies by cmd; ack=1, data=6/7, error=4, pong=8 (section 4).
4. **Command layer (inside cmd 0x0005).** HC buffer `len opcode args…`; the interface wraps it as
   `53 len+3 <buf> xor`. Opcodes in sections 5 and 7.
5. **Session key.** Before CAN I/O, run the `0x50` TEA exchange (section 6 — plain 32-round TEA, not
   XTEA) and keep the 16-byte mask; XOR it over every CAN command buffer and response (`HC::SCM`).

**Connection state machine (`HC::CANOpenCtrl`, `FUN_004555f2`):**

```
GetVersion (0x02)                 -> interface fw, confirm link
[CAN path]
  key exchange (0x50, XTEA)       -> 16-byte CAN mask            (FUN_004610a4)
  set message object/filter (0x52)-> accept id/mask             (uCh::SetMsgObj)
  ScrCAN                          -> enter CAN receive mode; ack 0x8C
  loop: SendCANMsg (0x11, masked) -> TX; poll 'S' status + 0xFE/0xB9 replies
[K-line path]
  bus test (0x74)                 -> which buses present
  slow init 0x84 <addr|parity>    OR  fast init 0x98 (C1 33 F1 81 66)
  TurboBaud (0x85)                -> raise baud
  then KW1281 blocks / KWP2000 msgs through cmd 0x0005
```

`DAT_0097FACC == 2` marks "CAN mode"; `HC::SendCommand` then uses the masked CAN path, otherwise the
plain K-line path. Start the HID transport + frame/message layers first; they need no device secrets
and can be unit-tested against the captured byte sequences in `VCIConfig\log.txt`.

## 9. Transport protocols on top of the raw link  [std]

Key architectural point from the RE: the HEX-V2 interface only moves **single CAN frames** (opcode
`0x11` TX, `ScrCAN`+`'S'` status RX) and **K-line bytes/blocks** (after `0x84`/`0x98` init). TP2.0,
ISO-TP and KW1281 are **not interface features** — VCDS implements them in software on top of those
primitives. So for the app they are **standard, publicly documented protocols**; the notes below are
from those standards (marked `[std]`), to be driven over the layer-4 primitives in section 8. They
are not additional HEX-V2 opcodes.

### KW1281 (old K-line, "block" protocol)  [std]

1. 5-baud slow init to the module address (interface opcode `0x84`) → ECU returns two **keyword
   (KW) bytes**; `0x01 0x8A` (or `0x01 0x0A`) selects KW1281.
2. Then a byte-level block loop, each byte echoed as its **one's-complement** by the receiver:
   `[block_len] [block_counter] [block_title] [data…] [0x03 end]`. `block_counter` increments by 1
   each block and wraps. Common titles: `0x3C` ACK/keep-alive, `0xF6` ASCII data, `0x09` ack,
   `0x07` fault codes, `0x06` end-output. VCDS runs this loop in software; the interface just relays
   bytes (reply cmd 7 path in `FUN_004596b2`).

### KWP2000 / KW2000 (K-line, ISO 14230)  [std]

Fast init (interface opcode `0x98`, StartCommunication `C1 33 F1 81 66`) or slow init (`0x84`), then
messages `[fmt] [tgt] [src] [data…] [cksum]` where `fmt` encodes length. `0x81` StartCommunication,
`0x10` StartDiagnosticSession, `0x27` SecurityAccess, `0x22`/`0x21` ReadData, `0x3E` TesterPresent.
TurboBaud (`0x85`) raises the link speed after init.

### VW TP2.0 (KWP2000-over-CAN, most 2000s-era VAG)  [std]

Software layer over raw CAN (`0x11` / RX). Channel setup:

- Broadcast setup request to CAN id `0x200`, data `<dest_logical_addr> C0 00 10 00 03 01`.
- ECU replies from `0x200 + dest` with the pair of **channel CAN ids** to use (RX id, TX id).
- Channel-parameter exchange: request `A0` / response `A1` carrying block size and timings
  (T1=0x0A, T3=0x32, …). `A3` = setup, `A8` = disconnect.
- Data transfer frames: first data byte is `<opcode_nibble><seq>`:
  `0x0`=more, wait for ACK; `0x1`=more+ACK; `0x2`=last, no ACK; `0x3`=last+ACK; `0xB`=ACK.
  The reassembled payload is a KWP2000 message (`0x10` session, `0x22` read, `0x27` security…).
- Keep channel alive with periodic `A3`/TesterPresent.

### ISO-TP / ISO 15765-2 (UDS-over-CAN, newer VAG)  [std]

Software layer over raw CAN. Frames by PCI nibble in byte 0: `0x0` Single (len in low nibble),
`0x1` First (12-bit len), `0x2` Consecutive (seq in low nibble), `0x3` Flow-control
(`30` CTS / `31` wait / `32` overflow, block size, STmin). Payload is UDS (`0x10`, `0x22`, `0x19`,
`0x27`, `0x3E`, …). Standard 11-bit request/response id pairs (e.g. `0x7E0`/`0x7E8`) or 29-bit
normal-fixed addressing; set the accept filter with opcode `0x52`.

## 10. Not covered / still [?]

- Exact derivation the **device** uses to turn the `0x50` nonce into the 16-byte mask (we send the
  challenge and consume the reply — enough to implement — but the device's internal step is
  unverified). The 1-byte KC mask (+0x1A0) is only on the CAN-scan/bus-probe path (2 callers), not
  the mainline K-line diagnostic channel, which is plaintext HC; treat it as out of scope until a
  capture shows otherwise.
- Authentication / OEM / product handshake (`oem`, `product`, `cb1`, `cb2`), encrypted CB packets,
  firmware update, Wi-Fi / HEX-NET config. (Not required for CAN/K-line diagnostics — see verdict.)
- PID 0xA00F device (probed before A0CB; identity unknown).

### Verdict: do `cb1`/`cb2` gate communication?  [C, host-side]

**No, not on the host side.** `oem`/`product`/`cb1`/`cb2` are identity/capability values the host
**reads from** the interface (management cmd `0x15`); they are not a challenge the host must answer.
The `cb*` strings exist only in `VCIConfig.exe` (the config tool), not in VCDS. The full CAN
bring-up `HC::CANOpenCtrl` (`FUN_004555f2` → `FUN_0045425f`, `FUN_0045439f`, `FUN_00455513`) was
read end to end: every gate is a device-config opcode checked only for a `0xFE` ack —
`01 B0`, `01 B1`, `04 B2 <250k/500k> B8 05`, then the controller-open helpers. **None reads cb/oem.**
The only host→device gate on the data path is the `0x50` XTEA exchange (CAN only); K-line has none.
A from-scratch client therefore never has to satisfy `cb1`/`cb2`.

Residual caveat: this proves no *host-side* gate; it cannot prove the *device firmware* doesn't
expect the identity read first. Nothing suggests it does (VCIConfig's read-then-traffic order is
housekeeping, not an enforced dependency). Confirm with the read-only probe in section 11 when the
device is available.

## 11. Suggested validation before implementing

1. Read-only probe from the app: open `0483:A0CB`, send a `53 04 04 53` request (get firmware, as
   VCIConfig does) and a `53 04 14 43` request; expect `4D …` replies.
2. Then a CB-frame probe `04 0A 01 00 00 17 00 00 00 18` (seq 1, cmd 0x0017, no data).
3. Capture USB traffic from VCDS during a known action (one CAN auto-scan, one K-line init) and
   compare the on-wire CAN frames with the `0x11` layout above — this settles the XOR-key question
   (section 6) and confirms the opcodes. The static notes are a map, not a substitute for a trace.

Tooling used (all under `scripts/ghidra/`, `scripts/trim_pe_section.ps1`): headless Ghidra 12.1.4 on
JDK 21; pe-sieve32 to dump the unpacked image. Raw decompiler output is in `re-work/` (not intended
for commit).
