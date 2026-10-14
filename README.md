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
Discovery is read-only: it only ever sends TesterPresent, read-DTC, read-data and default-session requests. The **Functions** page is the exception: after you confirm, **Run…** opens the function's diagnostic session and starts its UDS routine (0x10, 0x31), which acts on the car. Only routines that need no input of their own can run. Replay accepts CSV data logs and CAN captures
(`candump -L`, Vector ASC, `candriver monitor`). See `THIRD_PARTY_NOTICES.md` for the DTC data.

### Workshop features

- **Fault codes** and the **Overview** combine the engine ECU's codes with the faults every scanned module holds (ABS, airbag, body…), each named with its module. The module scan runs automatically once per connection.
- **Save report** writes a one-page HTML job report (vehicle, VIN, codes, freeze frame, readiness, battery) to `<data dir>/reports/` and opens it in the browser: print it or save it as a PDF. **Copy** puts the codes on the clipboard for a work order; **Copy VIN** is in the status bar.
- Battery is judged against engine speed: the same 12.1 V is a weak battery with the engine off and a failed alternator with it running.
- **Live data** is a dashboard you build: **Add signal** opens a searchable picker (the engine's channels, plus each module's own values from the FORScan import) and one click puts a signal on the dashboard as a **card**. **Edit dashboard** turns any widget into a **line or area chart** and sets its width, time window (10 s to 5 min) and fixed or fitted axis; widgets can be dragged onto each other to reorder (or moved with the arrows) and removed. Chart axes carry labelled ticks on round numbers (800, 1000, 1200…). Module values are re-read once a second while the page shows. The layout is saved to `<data dir>/dashboard.json`. **Functions** has a tab per module, a search box, and groups by system (fuel, EGR/air, exhaust, brakes…); the same procedure listed for several engine variants shows once.
- Pause, min/max per card, and **Record** to a CSV in `<data dir>/recordings/` that `--replay` plays back.
- Clearing codes and starting a function check the engine state against live data and refuse when the car is in the wrong state.
- Light and dark themes, and a **Tablet mode** that enlarges text, buttons and rows for touch use (buttons at the bottom of the sidebar; both are remembered).
- **Backup before write.** Starting a function first reads the module's known data identifiers and saves them to `<data dir>/backups/<VIN>/`. **History → Backups → Restore** writes back only the values that differ (UDS 0x2E, extended session). Modules often refuse writes without security access; each value reports what the module answered.
- **Supply gate.** Starting a function or a restore is blocked below 11.8 V (engine off) or 12.8 V (engine running), warns below 12.4 V or when the voltage is unknown, and always asks first.
- **Clear codes** records what the engine ECU held, clears, re-reads, and says how many were cleared and which came back at once. The result goes into the visit and the report.
- **History.** Once the modules have been scanned, the visit (codes, battery, monitors not ready, actions) is saved to `<data dir>/history/<VIN>/`. The Overview, Fault codes and reports show what is new and what is gone since the previous visit; the code table marks new codes.
- **Procedures** (Advanced): battery registration, DPF regeneration, oil service reset and electric parking brake pad service. Each is a sequence of instructions, live checks (engine state, voltage) and functions found in your FORScan import by title; a step with no matching function can be skipped. Functions use the normal confirmation dialog.
- **Likely causes and what to check** under each fault code (**Why?**). It is general workshop guidance for common codes and for each system, not a diagnosis.
- **Self-test results** (OBD mode 06) on the Readiness page: each monitor's value against its limits, flagging parts near a limit. Values are shown raw (unscaled).
- **Trace** (Advanced) lists every OBD/UDS message sent and received (from candriver's trace log), filterable, with pause and copy. A recording also saves its trace next to the CSV as `live-<time>.trace.txt`.
- **Connection** card on the Overview (adapter, bus speed, engine ECU, source) and a **Reconnect** button, also on the offline banner. The worker retries every 3 s by itself.
- **Ctrl+K** opens a search over pages, commands, procedures, modules, fault codes and functions.
- **PDF reports**: *Save PDF* (technical, with causes and checks) and *Customer PDF* (no VIN, module addresses or freeze frame), written to `<data dir>/reports/`.
- **Português**: a button at the bottom of the sidebar switches the interface (remembered in `<data dir>/lang`). Navigation, the main buttons, status text and safety texts are translated; data from the car, code descriptions and the Help improve page stay English.

### FORScan import (local only)

`scripts/forscan_import.py <forscan.sqlite> --write` reads your own copy of a FORScan database and writes module names,
functions, readable parameters and trouble code wording into your data directory (`modules.d/`, `forscan/`, `dtc.d/`). The
module tabs on **Live data** list a module's parameters, ask the module which of them it has, and read them on request. Nothing
from FORScan is bundled, uploaded or exported, and it must not be copied into vehicle-db (see its LEGAL.md).

### Trouble code data

Descriptions come from [vehicle-db](https://github.com/driveforgelabs/vehicle-db). A snapshot is built into the
binary; `canforge db update` fetches the latest release (SHA-256 verified, replaced atomically) and
`canforge db status` shows what is in use. Put your own `CODE<TAB>MAKE<TAB>DESCRIPTION` files in
`<data dir>/dtc.d/*.tsv` (Linux: `~/.local/share/canforge/`) to add or override entries; they are never uploaded.
Refresh the snapshot with `scripts/sync_vehicle_db.sh ../vehicle-db`.

Set `CANFORGE_TRACE=1` to print how long each live-data polling cycle takes.

## Layouts

The window width picks the layout: wider than 1180 px is the laptop layout (208 px sidebar), up to 1180 px the tablet
landscape layout (92 px icon rail, 44 px touch targets), up to 820 px the portrait layout (bottom tab bar and a More
sheet). The text-size setting (Settings) keeps a wide window in the touch layout.

To look at a layout during development, `CANFORGE_WINDOW=1180x820` opens the window at that size and
`CANFORGE_OPEN=settings|palette|more|report|run|confirm|bench|detail` opens an overlay at start-up.
