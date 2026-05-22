# canforge

CAN-bus capture, decode, and replay.

Works with any USB or CANable-compatible adapter. Decode raw frames against a DBC, replay a session, or pipe live data into [`cluster-core`](../cluster-core).

## What it does

- Capture raw CAN frames over USB, CANable, Kvaser, PCAN, or Comma Panda
- Decode frames against DBC files or the community signal library
- Replay recorded sessions for offline development and testing
- Stream live decoded data to other DriveForge tools

## Status

`●` stable — v1.8.0

## Compatible adapters

- CANable (v1 and v2)
- Kvaser Leaf
- PCAN-USB
- Comma Panda

## Related

- [`cluster-core`](../cluster-core) — consumes live canforge streams
- [`flasher`](../flasher) — ECU flashing, shares the adapter layer
- [`hardware-compat`](../hardware-compat) — full adapter compatibility matrix

## License

Apache-2.0
