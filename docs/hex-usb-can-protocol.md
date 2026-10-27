# Ross-Tech HEX-USB+CAN wire protocol — working notes

Source: static analysis (Ghidra) of the VCDS 26.9 binaries on the author's own machine. Purpose:
interoperability with a HEX-USB+CAN interface the author owns. Licensing / authentication logic was
deliberately **not** analysed.

This interface is the **older FTDI-based** generation. It shares VCDS's command/opcode layer with the
HEX-V2 but uses a completely different **transport**. Read this doc for the transport; for the
command frames, opcode map, CAN/K-line opcodes, the `0x50` TEA session key, and the cb1/cb2 verdict,
see **[hex-v2-protocol.md](hex-v2-protocol.md) sections 4–10** — those layers are the same VCDS code
reached through the interface's vtable (+0x88 send / +0x8C transact), independent of transport.

Legend: **[C]** confirmed in decompiled code, **[I]** inferred (shared code path; confirm on
hardware), **[?]** unknown.

## 1. Transport: FTDI, not HID  [C]

The HEX-V2 is USB-HID; the **HEX-USB+CAN is an FTDI USB-serial device**. VCDS's transport layer
dispatches on `DAT_0097fedc`: `== 8` → HID (HEX-V2), `== 7` → TCP (HEX-NET), **else → the FTDI/serial
path** (`FUN_004cd*`). Your "USB mode works, COM mode fails" maps to this: both are the non-HID path,
but they open the byte-I/O handle differently (section 2).

Device identity (`FUN_004bb28c`): enumerated with the D2XX library `rt-usb.dll` via
`FT_CreateDeviceInfoList` / `FT_GetDeviceInfoList`, matching **VID `0x0403`** and
**PID in `0xFA20`–`0xFA2F`** (`(id>>16)==0x0403 && (id&0xFFFF)-0xFA20 < 0x10`). More than one match
→ "Please select ONE interface". Per the RT-USB64.inf the relevant descs are `FA20` "HEX or KEY USB",
`FA23` "Micro-CAN", `FA24` "Direct USB", `FA25` "USB Converter"; a HEX-USB+CAN reports one of these
(confirm with `FT_GetDeviceInfo` on your unit).

## 2. Open / line configuration  [C]

Two stages, both in `FUN_004bb28c` + `FUN_004bb441`:

1. **D2XX config (device-only operations):** `FT_Open(index)` → handle `DAT_00a0b294`;
   `FT_ResetDevice`; `FT_Close`+re-`FT_Open` (driver quirk); `FT_SetLatencyTimer(1)` — `2` only in a
   special case (baud ≥ 9001 **and** a specific protocol id **and** not CAN mode);
   `FT_SetTimeouts(read=1 ms, write=100 ms)`; `FT_GetDeviceInfo`;
   `FT_SetDataCharacteristics(8, 0, 0)` = **8 data bits, 1 stop, no parity (8N1)**;
   `FT_SetBaudRate(DAT_0097fa44)`; `FT_Purge(3)` = flush RX+TX.
2. **Default baud:** `DAT_0097fa44 = 0x2580` = **9600 baud** (`FUN_00xxxx` sets it in the K-line
   setup path). This is the host↔interface FTDI link rate; `HC::TurboBaud` (opcode `0x85`) raises it
   after a K-line session is established. (Other rates like `0x1C200`=115200 and `500000` appear in
   CAN-bitrate fields, not the FTDI link.)

**Byte I/O is a `\\.\COMn` serial handle, not `FT_Write`/`FT_Read`.** `FT_Write`/`FT_Read`/
`FT_W32_WriteFile`/`FT_W32_ReadFile`/`FT_GetQueueStatus` are **never called** anywhere in VCDS
(verified: 0 call sites). The opener `FUN_004cceba` does `wsprintfA("\\\\.\\COM%d", index+1)` →
`CreateFileA(GENERIC_READ|WRITE, share 0, OPEN_EXISTING, non-overlapped)`, stores the handle in
`DAT_00a0be88[index*8]`, then `GetCommState`/`SetCommState`/`SetupComm`/`SetCommTimeouts`. All stream
I/O is `kernel32` `WriteFile`/`ReadFile`/`ClearCommError` on that handle (`FUN_004cd16c`
write-buffer, `FUN_004cd118` write-1-byte, `FUN_004cd1c1` read-1-byte; `FUN_004cd255`/`FUN_004cd2b6`/
`FUN_004cd471` drive DTR/RTS/BREAK via `EscapeCommFunction`/`SetCommBreak`, used around K-line init).
**So D2XX (`rt-usb.dll`) is used only to discover the device, reset it, and set the latency timer +
baud; the actual byte stream is always the FTDI virtual COM port.**

> **Why "COM mode" fails but "USB mode" works [I — strong].** Both modes end up doing I/O over
> `\\.\COMn`. The difference is the D2XX step (section 2.1): "USB mode" runs it — which **sets the
> FTDI latency timer to 1 ms** and resolves the device's own COM number — while "COM mode" just opens
> the `COMn` you pick. The FTDI driver's default latency timer is 16 ms; at 16 ms the byte-level
> K-line/command timing breaks, so COM mode (without the D2XX latency fix) fails. Setting the latency
> timer is the load-bearing step, not the API used for I/O.

### For a from-scratch client

Open the FTDI device directly with **libftdi** (`ftdi_usb_open(0x0403, 0xFA2x)`) or **D2XX**
(`FT_OpenEx`): set **8N1**, baud **9600**, **latency timer 1 ms** (`ftdi_set_latency_timer` /
`FT_SetLatencyTimer` — do not skip this), read timeout ~1 ms, purge RX+TX, then read/write the
command frames below with `ftdi_write_data`/`ftdi_read_data` (or `FT_Write`/`FT_Read`). This is
cleaner than VCDS's own discover-via-D2XX / stream-via-COM split and sidesteps the COM/USB-mode
distinction entirely — you get the latency fix and the byte stream through one API.

## 3. Command framing over the stream  [C / I]

Unlike the HEX-V2, there is **no HID report layer, no `04` CB-frame, and no message layer**
(seq/cmd16/len16). On the FTDI stream the HC command frame is written **directly**:

- **Send** (`HC::SendCommand` `FUN_00452ea9` → `FUN_004bb17f` → `FUN_004cd16c` `WriteFile(handle,
  frame, len)`):
  ```
  53 | len+3 | <payload> | xor
  ```
  where `payload = innerlen opcode args…`, `innerlen` = payload length, and `xor` = XOR of all
  preceding bytes (seed `0x53`). [C]
- **Receive** (`FUN_00452f6a`, reading one byte at a time via `FUN_00470267` → `FUN_004cd1c1`
  `ReadFile(handle, &b, 1)`):
  ```
  4D | len | <payload> | xor
  ```
  parsed and XOR-verified; `len > 0x30` or bad XOR are rejected. [C]

This is the same `53`/`4D` frame and XOR rule as [hex-v2-protocol.md §3](hex-v2-protocol.md); the
difference is only that on FTDI it is the **whole** transport (no HID/CB/message wrappers around it).

## 4. Command / opcode layer — shared with HEX-V2  [I: shared code, confirm on HW]

Reached through the same vtable the HEX-V2 uses, so the opcodes are the same bytes (see
[hex-v2-protocol.md §5, §7](hex-v2-protocol.md) for the full map):

- **K-line:** slow/address init `0x84` (`04 84 <addr|odd-parity> <mode> 03`), fast init `0x98`
  (`C1 33 F1 81 66`), `TurboBaud` `0x85`, `GetVersion` `0x02`, bus test `0x74`. Then KW1281 / KWP2000
  as in [§9](hex-v2-protocol.md).
- **CAN:** `CANOpenCtrl` config acks `B0`/`B1`/`B2`, filter/msg-object `0x52`, receive-enable
  (ScrCAN), transmit `0x11` (16-byte frame, XOR-masked), and the `0x50` **TEA** session-key exchange
  (see [§6](hex-v2-protocol.md) — plain 32-round TEA, key schedule index 4; the returned 16-byte
  mask XORs CAN frames).

These are `[I]` for *this* interface: the command dispatch is **not gated by transport/interface
type** (`CANOpenCtrl`, `0x11`, `0x50` do not branch on `DAT_0097fedc`), so VCDS issues the same CAN
sequence over the FTDI link that it does over HID. Whether the HEX-USB+CAN's older **firmware**
answers all of it — especially the `0x50` TEA handshake, which may be HEX-V2-era — is the one thing
that can only be settled on hardware. If the CAN open stalls, suspect the firmware doesn't do `0x50`
and the CAN frames may be unmasked on this unit.

## 5. cb1/cb2 / licensing  [C, host-side]

Same verdict as the HEX-V2 (see [hex-v2-protocol.md §10](hex-v2-protocol.md)): `oem`/`product`/`cb1`/
`cb2` are identity values the host *reads from* the interface, not a gate; the connection path checks
only `0xFE` acks. A from-scratch client does not satisfy them.

## 6. Still [?] for this interface (hardware-only now)

Both static unknowns from the first draft are now resolved (byte I/O = `\\.\COMn`; "COM fails"
explained by the latency timer). What genuinely remains needs the device:

- Whether the HEX-USB+CAN **firmware** implements the CAN opcodes and the `0x50` TEA exchange, or an
  older/simpler CAN command set — host code is not the limiter (§4).
- Exact PID of this specific unit (one of `0xFA20`–`0xFA2F`); read it with `FT_GetDeviceInfo`.
- The exact default latency-timer value COM mode leaves in place (inferred 16 ms FTDI default).

## 7. Validation when the device is available

1. Open `0403:FA2x` via libftdi/D2XX at 8N1 9600, latency 1 ms, purge.
2. Send GetVersion = `53 04 02 55` and expect a `4D …` reply. (Frame rule: `53 | L | payload | xor`
   with `L = payload_len + 3`; GetVersion's payload is the single opcode byte `02`, so
   `L = 4`, `xor = 53^04^02 = 55`. `HC::SendCommand` stores the buffer as `{count, opcode, …}` and
   emits `53 | count+3 | opcode… | xor` — the count byte is folded into `L`, not sent.)
3. Bus test `0x74` to see which buses the car exposes, then a K-line slow init `0x84` to a known
   address, and — if CAN is present — attempt `CANOpenCtrl`/`0x50` and watch whether the interface
   requires the TEA exchange.

## 8. K-line implementation Q&A (for the Roomster bring-up)

Concrete answers for implementing KW1281 over this interface. Evidence tags as elsewhere.

> **Correction (supersedes an earlier draft of this section).** An earlier version said the KW1281
> block relay goes through message cmd `0x0005` (`FUN_0045a3eb`/`FUN_0045a6fa` → `FUN_0045935c` →
> `FUN_0045a9fa`) and that the MCU does the complement. On re-tracing, **that `0x0005` CB-block
> mechanism transmits only over TCP/HID** — `FUN_0045a9fa` dispatches to `send()` (fedc==7) or
> `FUN_004d37f3` (HID handle at obj+0x1EC) and has **no FTDI branch**. So that path is the
> HEX-V2 / HEX-NET relay, **not** this FTDI interface. The claim is retracted for HEX-USB+CAN.

**Q (A1/A2): KW1281 block wire shape on *this* (FTDI) interface?**

*Transport frame — [C].* The interface object's dispatch table is at base `0x5AE910`:
**`+0x88` = `FUN_00452ea9` (HC::SendCommand), `+0x8C` = `FUN_00452f6a` (HC receive)** (verified: both
pointers sit at `0x5AE998`/`0x5AE99C`). On FTDI, `+0x88` → `FUN_004bb17f` → `FUN_004cd16c`
`WriteFile`, and `+0x8C` → `FUN_00452f6a` → `FUN_00470267` → `FUN_004cd1c1` `ReadFile`. So every
command/response is an HC frame: **send `53 | L | payload | xor`, receive `4D | L | payload | xor`**
(`L` = framelen, `xor` over all preceding bytes, seed `0x53`/`0x4D`).

*Receive block layout — [C].* A parsed response block lands in the interface object: **body at
`+0x1FA`, length at `+0x1F8`, reply-cmd/title indicator at `+0x1F4`** (`FUN_0045a1ef`/`FUN_004596b2`
populate these; callers read `[len][body…]`). The body is the ECU block payload **`[title][data…]`**
— the `4D` frame wrapper (and its `xor`) and the KW1281 per-byte complements are already stripped
before this point. So on receive you get clean `[title][data…]`, **not** `[len][seq]` and not the
complement bytes.

*Send — there are TWO K-line stacks; which one applies depends on the interface. [C]* The selector
`DAT_0097fedc` (`FUN_0049ded7`) is `0`/`1`/`2`/`3` for FTDI serial, `7` = HEX-NET, `8` = HEX-V2.

1. **HC stack** (`53`/`4D` frames via vtable `+0x88`/`+0x8C`): used by `FUN_00452b94` slow init
   (opcode `0x84`, reply `84 | <baud32 LE> | KW_lo KW_hi`). **Your live `84 8C 25 00 00 01 8A`
   matches this stack**, so your unit is driven through the HC path for init.
2. **KC byte-stream stack** (`FUN_0046cd29`/`FUN_0046ced1` send, `FUN_0046cdeb` receive): the
   genuinely byte-level FTDI K-line path. **This is the FTDI block relay I earlier wrongly said was
   absent** — it exists. Frame:
   ```
   [len] [payload…] [xor]     len = payload_len + 2 ;  xor = XOR(len, all payload bytes)
   ```
   sent **byte-by-byte** (`FUN_004bb1b6`), bracketed by **RTS-set (`0x53`'S') before / RTS-clear
   (`0x43`'C') after** and an **~135 ms pre-delay** (`FUN_004704b6(0x87)`), RX flushed first. Receive
   (`FUN_0046cdeb`) reads `[len][payload][xor]` and checks XOR==0. **No `0x53`/`4D` wrapper on this
   stack** — the `0x53`/`0x43` are RTS line controls, not frame bytes.

*KC-stack slow init (`FUN_0046c702`) — [C].* Two KC frames: first payload `44 <mode> 00`
(`'D'` = detect/baud, `<mode>` = `DAT_005CF9F4`), then `49 <addr|parity>` (`'I'` = init-to-address;
parity rule as in §SlowInit). KW bytes come back at **response offsets 4–5**; **baud = `0x1C2000 /
divisor`** where `0x1C2000` = 1843200 (FTDI base clock) and `divisor` = response bytes 2–3. So on the
KC stack the baud is reported as a divisor, not a raw rate.

**ECU measuring-block / fault-code passthrough — resolved to a binary-proven negative.** The ECU-block
message layer has exactly two senders, `FUN_0045935c` and `FUN_004595a7` (53 call sites total); both
feed `FUN_0045a9fa` / `FUN_0045adca`, and **both transmit only for `fedc==8` (HID handle at obj+0x1EC)
or the HEX-NET socket** — a byte-grep of both functions finds **zero** FTDI send calls
(`FUN_004cd16c`/`FUN_004bb17f`/`FUN_004bb1b6`). FTDI is `fedc 0–3`. **Therefore VCDS 26.9 has no ECU
measuring-block/fault-code passthrough over the FTDI interface — it is provably absent from the
binary, not merely unlocated.** (Consistent with Ross-Tech having deprecated the HEX-USB+CAN: this
build does init/identify/CAN/KC-commands over it, but routes full ECU block diagnostics only to
HEX-V2/HEX-NET.) A from-scratch client must drive the KW1281 block loop itself (standard; the
interface does the per-byte complement — B3).

### HC opcode map (sent via vtable +0x88, confirmed from `FUN_*` buffer writes)

`0xA0` interface test/presence · `0x84` slow init · `0x98` fast init · `0x85` TurboBaud ·
`0x02` get version · `0x82` K-line test (`HC::KLineTest`) · `0x03` comm/baud setup (`HC::Com115`,
payload `00 C2 01 00`) · `0x9A` finalize-connection (1-byte `01 9A`, sets the "connected" flag like
FastInit) · `0xB0`/`0xB1`/`0xB2`/`0xB5`/`0xB8` CAN open/config · `0x50`+`0xB6` TEA key exchange ·
`0x52`/`0xB4` CAN filter · `0x11` CAN TX · `0x0B`/`0x0C` security table · `0x17`/`0xD5` interface
get/set config · `0x01` block upload (counter+complement, `FUN_004564e7`). KC-stack opcodes
(byte-stream frame): `0x44`'D' / `0x49`'I' init · `0x74` K-line test · `0x22` security upload. **No
opcode in this set is a generic "relay an arbitrary KWP/KW1281 block to the K-line."**

### Live-capture Q&A — HC-stack K-line block relay (definitive)

Validated against the capture (`53 07 84 01 00 03 D2` → `4D 0A 84 8C 25 00 00 01 8A E1`; bad opcode
`53 06 03 01 07 50` → `4D 05 FF 01 B6`).

**Q1. Which HC opcode transmits a K-line block? What `<OPCODE>` in `53 | L | <OPCODE> <payload> | xor`?**
**Binary answer: there is no such opcode in VCDS 26.9.** The complete set of opcodes VCDS emits via
the HC dispatcher (`+0x88`) is the map above — every one is a fixed interface function (init, baud,
test, version, CAN, TEA, security), and the `0xFF 0x01` you got for `0x03` is the MCU's
unknown/!ready reply (`0x03` = `HC::Com115`, a baud-setup command, not a block carrier). The reason
there is no block opcode: in 26.9 the ECU block exchange runs through the **message layer**
(`FUN_0045935c`, msg cmd `0x0005`), and that layer is wired **only** to HEX-V2 (HID) and HEX-NET
(socket) transports (`fedc 8`/`7`) — proven: both of its senders have zero FTDI send calls. So VCDS
26.9 never sends an ECU block over this FTDI interface; the block-relay opcode your MCU accepts lives
in the **MCU firmware** (and in older VCDS that still drove this interface) — neither is in this
binary, so it cannot be read out here. The guessing space is bounded for you: the MCU's valid-opcode
table is what matters, and the opcodes it is *known* to accept are the map above; a block opcode will
be one **outside** that set.

**Q2. Does it need an explicit "enter transparent/relay mode" command after SlowInit?**
**Best binary-grounded answer: `0x9A`.** `FUN_00455aa3` sends the single-byte command `01 9A`,
expects `0xFE`, and on success sets the object's "connected/live" flag (`+0x9BE = 1`) — the *same*
flag `FastInit` sets when a K-line session becomes active. It carries no payload and is the one
opcode whose only effect is to transition the interface into the connected state. It is the
strongest candidate for the post-SlowInit "go live / start relaying" step and the thing most likely
to stop the `0x1B` idle prompts. Send `53 04 9A <xor>` after a good `0x84` and watch whether the
`0x1B` train stops and block I/O opens. (`TurboBaud 0x85` is the other post-init step VCDS issues,
to raise the link speed — not a mode switch.)

**Q3. When the ECU replies with a block, what `4D` frame does the MCU return?**
**`4D | L | <OPCODE_ECHO> <block_bytes…> | xor`, and `<OPCODE_ECHO>` equals the request opcode.** The
HC reply always echoes the opcode byte in the `4D` frame — proven by your own captures: `0x84` →
`4D 0A 84 …`, and the rejected `0x03` → `4D 05 FF …` (`0xFF` = "unknown opcode" echo). So a block
response echoes whatever the (firmware-defined) block-relay opcode is, with the ECU block bytes as
the payload and the trailing XOR. VCDS's receive parser (`FUN_00452f6a` reads the `4D` frame;
`FUN_004596b2`/`FUN_0045a1ef` classify it) treats reply states `6`/`7` as **data** blocks (`1` = ack,
`4` = error, `8` = pong) and copies the block body to the object buffer at `+0x1FA`
(length `+0x1F8`). The echo value cannot be pinned to a number because its request opcode (Q1) isn't
in this binary.

**Q: SlowInit (`0x84`) reply layout?** [C] `84 | <measured baud, 32-bit LE> | KW_lo KW_hi`:
byte0 `0x84`=ok (else byte1 is an error code `03`/`09`/`04`); bytes1–4 = interface-measured K-line
baud (logged `B: %d`); **bytes 5–6 = KW pair** (`local_ff`,`local_fe`). Live `84 8C 25 00 00 01 8A` →
baud `0x0000258C`=9612, **KW `01 8A`** (KW1281). KW `04 04` ⇒ VCDS switches to the KWP-2000/fast path.

**Q: The `<mode>` byte in `04 84 <addr|parity> <mode> 03`?** [C] It is the global `DAT_005CF9F4`,
set by `FUN_0049dfe9` from a config token (`sscanf("%d")`, accepted if `< 100`), **default `0`**. So
`00` is the normal value and correct for a standard slow init; it is a user/config knob, not derived
from the ECU. `<addr>` = 7-bit address, **bit7 set for odd parity** (address `0x33` left as-is).

**Q: Post-init — does VCDS raise baud (`TurboBaud 0x85`) for KW1281, and with what value?**
[I / not determined] `TurboBaud` (`05 85 <baud32 LE>`) exists but **no caller was located in the
dumped ranges** (indirect dispatch). KW1281 is a 9600-class protocol and the interface already
reported ~9612, so the expectation is **no TurboBaud for plain KW1281** (it is used on the faster
KWP/CAN paths) — confirm against the capture. Per-op timeouts are written to `DAT_005D31C8` before
each transact (e.g. `0xCE4`=3300, `0x2EE`=750); the ~1 s inter-block keep-alive (`3C` block) is
host-side and best taken from your capture.

**Q (B3): after SlowInit, who does the KW1281 byte-level complement / KW2-complement — host or MCU?**
**The interface MCU — [C, by absence].** I searched the entire K-line/command code
(`vcds.hc.c`/`vcds.kc.c`/`vcds.kc2.c`) for a host-side complement (`^ 0xff`, `~b`, `0xff - b`) around
the byte read/write primitives (`FUN_00470267`/`FUN_004bb1b6`) and there is **none**. Combined with
the interface doing 5-baud init + baud measurement itself (§SlowInit), the byte-level complement-ack
and inter-byte timing are done **in the interface**, not the host. You exchange whole blocks
(`[title][data…]`) and never send per-byte complements or drive DTR/RTS/BREAK for the ack (those
primitives — `FUN_004cd255`/`2b6`/`471` — exist but are used for reset/line-state, not the KW2
handshake).

**Q (B4): the `0x1B` bytes emitted ~3× every ~116 ms right after SlowInit?** [C, by absence]
**VCDS's receive/parse code never tests a byte against `0x1B`** (searched `vcds.hc.c`/`kc.c`/`kc2.c`/
`v2layer.c`/`xport1.c` for `== 0x1b`, `!= 0x1b`, `- 0x1b` on read bytes — zero hits in the K-line
path). So `0x1B` is **not a VCDS-defined protocol byte**: VCDS neither emits it nor branches on it. It
originates in the **interface firmware** (which isn't in this binary) and VCDS simply reads past it.
Behaviour fits an interface idle-prompt while it waits for the host to proceed — respond (KC frame,
or the standard KW1281 keyword-complement + first ACK) and see if the train stops.

**Q (C5): first host action + block-counter start?** The binary-backed part: on the KC stack
(`FUN_0046c702`) the first host frames after bus detect are the init pair **`D` (`44 <mode> 00`)**
then **`I` (`49 <addr|parity>`)**, and the interface returns the KW pair and runs the
5-baud/keyword-complement handshake itself.

The deeper binary fact about the block loop: **VCDS does not contain a KW1281 byte-level block loop at
all.** There is no host-side complement (`^0xff`) loop anywhere (B3), no block-counter increment in
the K-line send code, and no `03 .. 09 03` ACK-block builder in the binary. VCDS delegates the entire
block framing — counter, complement, ACK, `0x03` terminator, timing — to the **interface firmware**
(which is not in this executable). So the "first block / counter start / ACK bytes" are **not
recoverable from the VCDS binary**, because VCDS never sees or builds them; it exchanges whole blocks
with a firmware that does the KW1281 framing. The implementable values are therefore the **KW1281
standard**: ECU block 1 (`counter = 1`), host ACK `03 02 09 03`, counter +1 per block. This is the
spec to code to — not a VCDS-specific variant, because none exists in the binary.

**Q: CAN / `0x50` TEA on this older unit?** [I] Host code is not transport-gated, so VCDS issues the
same `CANOpenCtrl`/`0x11`/`0x50` sequence over FTDI; whether the firmware answers `0x50` is the
hardware unknown. If CAN open stalls, assume no `0x50` and try unmasked CAN frames. K-line needs none
of this.

> **For the byte-level KW1281 questions (A1/A2/B3/B4/C5): your live capture is better evidence than
> this static analysis.** The code confirms the transport (FTDI byte stream, `53`/`4D` HC frames) and
> the SlowInit handshake, but the older FTDI diagnostic/block driver was not fully traced. If you want
> the static answer anyway, the next RE step is to locate the interface object's vtable +0x88/+0x8C
> targets for the K-line session and the KW1281 block packager — ask and I'll trace it.

### Follow-up Q&A — dispatch gating, the 0x1B train, raw byte writes, KC preamble

**Q1. After `0x9A`, does a `+0x9BE` ("connected") check silence/reroute non-init HC dispatch?** **No.**
`+0x9BE` is the connected/live flag (set by SlowInit, FastInit, and `0x9A`/`FUN_00455aa3`). Its only
read is in **`HC::Reset` (`FUN_00453a97`)**, where `+0x9BE != 0` just prepends a BREAK pulse
(`FUN_004bb26a`) before the reset command. The HC send dispatcher **`FUN_00452ea9` (vtable +0x88)
branches solely on `DAT_0097facc`** (`==2` → CAN, whole-frame; else → K-line, byte-by-byte) and
**never reads `+0x9BE`**; the receive classifier (`FUN_004596b2`/`FUN_0045a1ef`, in `vcds.v2layer.c`)
has **zero** `+0x9BE` references. So VCDS does not gate dispatch on the connected flag — "only `0x84`
answers after `0x9A`" is the **MCU firmware's** state machine, not a host-side routing change.

**Q2. What emits the `0x1B` train, and does any host write stop it?** The `0x1B` bytes are **not
referenced anywhere in VCDS** (re-confirmed across all K-line sources) — VCDS neither emits nor
consumes them, so they are **interface-firmware** output. After SlowInit VCDS writes **no raw stream
payload**; its next output is always a framed command. There is no secret "write-this-to-the-stream"
step in VCDS — the only thing that changes interface state post-init is a framed HC command
(`0x9A` finalize is the best candidate, per the previous section).

**Q3. Does VCDS ever write plain (non-`53`-framed) bytes on the FTDI stream — decisive via
`FUN_004cd118` callers?** **Decisively: exactly one raw byte, `0xA5`, and only pre-init.**
`FUN_004cd118` (single-byte `WriteFile`) is called only by `FUN_004bb1b6`, whose direct callers are:
(a) **`FUN_00451cdf` (interface detect)** → `FUN_004bb1b6(0xA5)` (`PUSH -0x5B` at `0x451EAF`), then
reads expecting `'C'`/`'D'`/`'F'` — a raw **sync/identify** byte; (b) `FUN_00452ea9` (HC::SendCommand)
— emits the `53` **frame** byte-by-byte; (c) `FUN_0046cd29`/`FUN_0046ced1` (KC) — emit the KC
`[len][payload][xor]` **frame** byte-by-byte. So the **only** non-framed raw write VCDS ever does is
`0xA5`, during interface detection **before** diagnostics — **not after SlowInit**. The missing
handshake is therefore not a hidden post-init raw byte in VCDS; `0xA5` is the one raw sync byte (worth
trying as a re-sync if the interface is stuck, but it is a detect-phase byte, not a per-session step).

**Q4. KC slow init `44 <mode> 00` — is `<mode>` required, does `0x00` apply, and is a DTR/BREAK
preamble needed?** `<mode>` = the global `DAT_005CF9F4` (config token, **default `0`**), so **`0x00`
is correct** unless a config set it otherwise. **A preamble IS required and is in `FUN_0046cd29`
before the first byte:** `FUN_004bb22f` → `FUN_004cd0bd` (set comm state/baud) + `FUN_004bb1ef(fedc,
'C')` = **clear DTR** + `FUN_004bb20f(fedc,'C')` = **clear RTS**; then `FUN_004bb20f(fedc,'S')` =
**set RTS**; then **~135-unit delay** (`FUN_004704b6(0x87)`); then `FUN_004bb163` = **flush RX**; then
the bytes stream out; afterwards `~20` delay + **clear RTS**. Net line state during the frame:
**DTR = low, RTS = high, with a ~135 ms settle after raising RTS**. Silence with `mode=00` most
likely means the **RTS-high + settle + flush preamble** (not the mode byte) was missing. No `BREAK`
(`FUN_004bb26a`) precedes the KC first frame — BREAK is only used by `HC::Reset` and some init paths.

---

Raw decompiler output backing this doc: `re-work/vcds.sio.c`, `vcds.ftdi.c`, `vcds.open.c`,
`vcds.hid2.c`, `vcds.hc.c`, `vcds.kc2.c` (FTDI open/config, `FUN_004cd*` I/O, SlowInit, block send),
plus the shared command-layer dumps referenced by hex-v2-protocol.md. Not intended for commit.
