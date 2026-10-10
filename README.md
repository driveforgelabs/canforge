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

## Diagnostics app (GPUI)

```
cargo run                                   # live, through an OpenPort 2.0
cargo run -- --simulate                     # no hardware
cargo run -- --replay examples/drive.candump --speed 4
cargo run -- --replay examples/datalog.csv
cargo run -- --replay capture.log --ddt ecu.json    # decode a CAN capture with DDT4ALL definitions
```

Live: OBD-II live data, VIN, trouble codes (generic + manufacturer), freeze frame, readiness
monitors, clearing codes (with confirmation), and module discovery (UDS): scan the bus for modules, read
their identification, trouble codes and supported services, and list their readable data identifiers (F100–F1FF).
The **Help improve** tab lets you name modules and describe trouble codes the database lacks (saved on your computer,
used immediately, exportable for a vehicle-db pull request with `tools/merge.py`).
Discovery is read-only: it only ever sends TesterPresent, read-DTC, read-data and default-session requests. Replay accepts CSV data logs and CAN captures
(`candump -L`, Vector ASC, `candriver monitor`). See `THIRD_PARTY_NOTICES.md` for the DTC data.

### Trouble code data

Descriptions come from [vehicle-db](https://github.com/driveforgelabs/vehicle-db). A snapshot is built into the
binary; `canforge db update` fetches the latest release (SHA-256 verified, replaced atomically) and
`canforge db status` shows what is in use. Put your own `CODE<TAB>MAKE<TAB>DESCRIPTION` files in
`<data dir>/dtc.d/*.tsv` (Linux: `~/.local/share/canforge/`) to add or override entries; they are never uploaded.
Refresh the snapshot with `scripts/sync_vehicle_db.sh ../vehicle-db`.

Set `CANFORGE_TRACE=1` to print how long each live-data polling cycle takes.
