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

**Q: Does the host or the interface MCU do the KW1281 byte-level complement-ack / pacing?**
**The MCU.** [C] The host sends a whole block as one diagnostic payload
(`FUN_0045a3eb`/`FUN_0045a6fa` → `FUN_0045935c(5, block+1, len)`, message cmd `0x0005`) and reads a
whole block back; there is **no per-byte `XOR 0xFF` loop in the host**. Implement it as: send
`[block_title][data…]` (length-prefixed), receive `[title][data…]`; let the interface handle the
complemented, `0x03`-terminated byte stream and 5-baud. The **block counter** is state you track and
place in the block. (`FUN_0045935c`'s first arg is the *message cmd*; `5` = diagnostic passthrough,
**not** a K-line opcode. The `FUN_004596b2` "cmd 7" path is KWP/UDS negative-response handling
(`7F .. 11`), not the KW1281 relay — ignore it for KW1281.)

**Q: SlowInit (`0x84`) reply layout?** [C] `84 | <measured baud, 32-bit LE> | KW_lo KW_hi`:
byte0 `0x84`=ok (else byte1 is an error code `03`/`09`/`04`); bytes1–4 = interface-measured K-line
baud (logged `B: %d`); **bytes 5–6 = KW pair** (`local_ff`,`local_fe`). Live `84 8C 25 00 00 01 8A` →
baud `0x0000258C`=9612, **KW `01 8A`** (KW1281). KW `04 04` ⇒ VCDS switches to the KWP-2000/fast path.

**Q: The `<mode>` byte in `04 84 <addr|parity> <mode> 03`?** [C] It is the global `DAT_005CF9F4`,
which has **no assignment anywhere in the analyzed code** → BSS default `0`. `00` is correct; no
non-zero use was found. `<addr>` = 7-bit address, **bit7 set for odd parity** (address `0x33` left
as-is).

**Q: Post-init — does VCDS raise baud (`TurboBaud 0x85`) for KW1281, and with what value?**
[I / not determined] `TurboBaud` (`05 85 <baud32 LE>`) exists but **no caller was located in the
dumped ranges** (indirect dispatch). KW1281 is a 9600-class protocol and the interface already
reported ~9612, so the expectation is **no TurboBaud for plain KW1281** (it is used on the faster
KWP/CAN paths) — confirm against the capture. Per-op timeouts are written to `DAT_005D31C8` before
each transact (e.g. `0xCE4`=3300, `0x2EE`=750); the ~1 s inter-block keep-alive (`3C` block) is
host-side and best taken from your capture.

**Q: CAN / `0x50` TEA on this older unit?** [I] Host code is not transport-gated, so VCDS issues the
same `CANOpenCtrl`/`0x11`/`0x50` sequence over FTDI; whether the firmware answers `0x50` is the
hardware unknown. If CAN open stalls, assume no `0x50` and try unmasked CAN frames. K-line needs none
of this.

---

Raw decompiler output backing this doc: `re-work/vcds.sio.c`, `vcds.ftdi.c`, `vcds.open.c`,
`vcds.hid2.c`, `vcds.hc.c`, `vcds.kc2.c` (FTDI open/config, `FUN_004cd*` I/O, SlowInit, block send),
plus the shared command-layer dumps referenced by hex-v2-protocol.md. Not intended for commit.
