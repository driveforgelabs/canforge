//! Talks to the vehicle on a worker thread. candriver's calls block on the serial port, so
//! they must never run on the UI thread; the UI sends [`Cmd`]s and drains [`Event`]s.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use candriver_core::{CanBus, CanConfig, Driver};
use candriver_obd::readiness::{self, Readiness};
use candriver_uds::discover::{self, DidFinding, Support};
use candriver_uds::{DtcReport, UdsClient, UdsDtc, nrc_name};
use candriver_obd::{Addressing, ClearResult, Dtc, DtcKind, ObdClient, pid};
use candriver_op20::Op20Driver;

use crate::session::{Channel, Session};
use crate::sim::{self, Simulator};

pub enum Cmd {
    ReadDtcs,
    /// Mode 04: erases codes, freeze frames and readiness monitors.
    ClearDtcs,
    /// Find modules (UDS) and read their identification, codes and services. Read-only.
    ScanModules,
    /// Ask the vehicle for its VIN again.
    ReadVin,
    /// Read every data identifier F100–F1FF of one module. Read-only.
    SweepDids { tx_id: u32 },
    /// Ask one module which routine identifiers (0x0000–0xFFFF) exist. Starts none; slow.
    /// (request ID, first routine ID) of every module to sweep, all at once.
    SweepRoutines { targets: Vec<(u32, u16)> },
    /// Stops a running routine sweep, keeping what it found.
    CancelSweep,
}

pub struct DtcEntry {
    pub ecu: u32,
    pub kind: &'static str,
    pub code: Dtc,
}

/// What the module scan learned about one module, ready to display.
#[derive(Clone)]
pub struct ModuleView {
    pub tx_id: u32,
    pub rx_id: u32,
    /// (standard name, value) of the identification data it answered.
    pub identity: Vec<(String, String)>,
    pub dtcs: Vec<ModuleDtc>,
    /// Why there is no code list, if so.
    pub dtc_note: Option<String>,
    pub services: Vec<(String, &'static str)>,
}

#[derive(Clone)]
pub struct ModuleDtc {
    /// `P0113-1C`
    pub code: String,
    pub status: &'static str,
}

/// One routine identifier that exists on a module.
#[derive(Clone)]
pub struct RoutineLine {
    pub id: u16,
    /// What the module said: results, or why it would not (e.g. "never started", "security access denied").
    pub state: String,
}

/// The outcome of a routine sweep of one module.
#[derive(Clone)]
pub struct RoutineScan {
    pub lines: Vec<RoutineLine>,
    pub end: String,
    pub complete: bool,
    /// Where a later scan resumes, if this one stopped early.
    pub next: Option<u16>,
}

/// One data identifier found by a sweep.
#[derive(Clone)]
pub struct DidLine {
    pub did: u16,
    pub name: String,
    pub value: String,
}

pub struct FreezeFrame {
    pub dtc: Dtc,
    pub values: Vec<(u8, f64)>,
}

pub enum Event {
    /// Not connected; the string says why. The worker keeps retrying.
    Offline(String),
    Connected { adapter: String, ecu: Option<u32>, channels: Vec<Channel> },
    Reading { key: String, value: f64 },
    Dtcs { mil_on: bool, codes: Vec<DtcEntry> },
    /// Sweep progress: probes answered per second, fraction of all work done, and the slowest module.
    RoutineProgress { rate: f64, done: f64, slowest: (u32, u16) },
    /// What a running sweep has found so far in one module (this run only), to be saved.
    RoutineCheckpoint { tx_id: u32, lines: Vec<RoutineLine>, next: u16 },
    Routines { tx_id: u32, lines: Vec<RoutineLine>, end: String, complete: bool, next: Option<u16> },
    Vehicle { vin: String },
    Readiness(Readiness),
    /// `None` when the ECU holds no freeze frame.
    Freeze(Option<FreezeFrame>),
    Modules(Vec<ModuleView>),
    Dids { tx_id: u32, lines: Vec<DidLine> },
    /// Outcome of a user action, to show in the UI.
    Notice(String),
}

pub struct Backend {
    pub cmds: Sender<Cmd>,
    pub events: Receiver<Event>,
}

/// Where data comes from.
pub enum Source {
    Vehicle { bitrate: u32 },
    Simulator,
    Replay { session: Session, speed: f64 },
}

fn obd_key(pid: u8) -> String {
    format!("pid:{pid:02x}")
}

/// A live-data channel for an OBD-II PID.
fn obd_channel(pid: u8) -> Option<Channel> {
    let info = pid::info(pid)?;
    let mut c = Channel::new(obd_key(pid), info.name, info.unit);
    (c.min, c.max) = (info.min, info.max);
    c.warn_above = match pid {
        0x0c => Some(5000.0),
        0x05 => Some(105.0),
        0x04 => Some(90.0),
        0x0f => Some(70.0),
        _ => None,
    };
    Some(c)
}

pub fn spawn(source: Source) -> Backend {
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (ev_tx, ev_rx) = mpsc::channel();
    thread::spawn(move || {
        match source {
            Source::Simulator => run_simulated(&ev_tx, &cmd_rx),
            Source::Vehicle { bitrate } => run_real(&ev_tx, &cmd_rx, bitrate),
            Source::Replay { session, speed } => run_replay(&session, speed, &ev_tx, &cmd_rx),
        }
    });
    Backend { cmds: cmd_tx, events: ev_rx }
}

/// Live-data PIDs worth showing in a freeze frame.
const FREEZE_PIDS: &[u8] = &[0x04, 0x05, 0x06, 0x07, 0x0b, 0x0c, 0x0d, 0x0f, 0x11];

const RETRY: Duration = Duration::from_secs(3);
const CYCLE: Duration = Duration::from_millis(250);

fn simulated_modules() -> Vec<ModuleView> {
    let svc = |s: [&'static str; 4]| {
        ["Diagnostic session control", "Read DTC information", "Read data by identifier", "Tester present"]
            .iter()
            .zip(s)
            .map(|(n, s)| (n.to_string(), s))
            .collect()
    };
    vec![
        ModuleView {
            tx_id: 0x7e0,
            rx_id: 0x7e8,
            identity: vec![("System name".into(), "Engine control (simulated)".into()), ("ECU software number".into(), "SIM-0001".into())],
            dtcs: vec![ModuleDtc { code: "P0113-1C".into(), status: "confirmed" }, ModuleDtc { code: "P0101-00".into(), status: "pending" }],
            dtc_note: None,
            services: svc(["yes", "yes", "yes", "yes"]),
        },
        ModuleView {
            tx_id: 0x760,
            rx_id: 0x768,
            identity: vec![("System name".into(), "ABS (simulated)".into())],
            dtcs: Vec::new(),
            dtc_note: None,
            services: svc(["other session", "yes", "yes", "yes"]),
        },
    ]
}

fn run_simulated(tx: &Sender<Event>, rx: &Receiver<Cmd>) {
    let mut sim = Simulator::new();
    let mut codes: Vec<u16> = sim::CODES.to_vec();
    let pids: Vec<u8> = pid::PIDS.iter().map(|i| i.pid).filter(|p| sim.value(*p).is_some()).collect();
    let dtcs = |tx: &Sender<Event>, codes: &[u16]| {
        let list = codes.iter().map(|&v| DtcEntry { ecu: 0x7e8, kind: "stored", code: Dtc(v) }).collect();
        tx.send(Event::Dtcs { mil_on: !codes.is_empty(), codes: list }).is_ok()
    };
    let channels = pids.iter().filter_map(|p| obd_channel(*p)).collect();
    if tx.send(Event::Connected { adapter: "Simulator".into(), ecu: Some(0x7e8), channels }).is_err()
        || tx.send(Event::Vehicle { vin: "WF0SIMULATED00000".into() }).is_err()
        || tx.send(Event::Readiness(readiness::decode(&[0x83, 0x0f, 0xc1, 0x40]).expect("static"))).is_err()
        || tx
            .send(Event::Freeze(Some(FreezeFrame { dtc: Dtc(0x0301), values: vec![(0x0c, 2250.0), (0x05, 88.0)] })))
            .is_err()
        || !dtcs(tx, &codes)
    {
        return;
    }
    loop {
        match rx.recv_timeout(CYCLE) {
            Ok(Cmd::ReadDtcs) if !dtcs(tx, &codes) => return,
            Ok(Cmd::SweepRoutines { targets }) => {
                for (tx_id, _) in targets {
                let lines = vec![
                    RoutineLine { id: 0x0203, state: "never started".into() },
                    RoutineLine { id: 0xff00, state: "exists, not readable: security access denied".into() },
                ];
                if tx.send(Event::Routines { tx_id, lines, end: "finished".into(), complete: true, next: None }).is_err() {
                    return;
                }
                }
            }
            Ok(Cmd::ReadVin) => {
                if tx.send(Event::Vehicle { vin: "WF0SIMULATED00000".into() }).is_err() {
                    return;
                }
            }
            Ok(Cmd::ScanModules) => {
                if tx.send(Event::Modules(simulated_modules())).is_err() {
                    return;
                }
            }
            Ok(Cmd::SweepDids { tx_id }) => {
                let lines = vec![
                    DidLine { did: 0xf188, name: "ECU software number".into(), value: "SIM-0001".into() },
                    DidLine { did: 0xf190, name: "VIN".into(), value: "WF0SIMULATED00000".into() },
                    DidLine { did: 0xf1a0, name: String::new(), value: "exists, not readable: security access denied".into() },
                ];
                if tx.send(Event::Dids { tx_id, lines }).is_err() {
                    return;
                }
            }
            Ok(Cmd::ClearDtcs) => {
                codes.clear();
                if tx.send(Event::Notice("Codes cleared.".into())).is_err() || !dtcs(tx, &codes) {
                    return;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
            _ => {}
        }
        sim.tick(CYCLE.as_secs_f64());
        for &p in &pids {
            if let Some(value) = sim.value(p) {
                if tx.send(Event::Reading { key: obd_key(p), value }).is_err() {
                    return;
                }
            }
        }
    }
}

/// Plays a recorded session back in real time (scaled by `speed`), looping at the end.
fn run_replay(session: &Session, speed: f64, tx: &Sender<Event>, rx: &Receiver<Cmd>) {
    let adapter = format!("Replay · {}", session.source);
    if tx.send(Event::Connected { adapter, ecu: None, channels: session.channels.clone() }).is_err() {
        return;
    }
    let notice = |text: &str| tx.send(Event::Notice(text.into())).is_ok();
    loop {
        let start = std::time::Instant::now();
        for r in &session.readings {
            let due = Duration::from_secs_f64(r.t / speed);
            loop {
                let wait = due.saturating_sub(start.elapsed());
                match rx.recv_timeout(wait) {
                    Ok(Cmd::ClearDtcs) if !notice("A replay is read-only: nothing was cleared.") => return,
                    Ok(Cmd::ScanModules | Cmd::SweepDids { .. } | Cmd::ReadVin | Cmd::SweepRoutines { .. }) if !notice("Module scans need a vehicle, not a replay.") => return,
                    Ok(_) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            let key = session.channels[r.channel].key.clone();
            if tx.send(Event::Reading { key, value: r.value }).is_err() {
                return;
            }
        }
        // pause before looping
        match rx.recv_timeout(Duration::from_secs(2)) {
            Err(RecvTimeoutError::Disconnected) => return,
            _ => {}
        }
    }
}

fn run_real(tx: &Sender<Event>, rx: &Receiver<Cmd>, bitrate: u32) {
    loop {
        let reason = match connect(bitrate) {
            Ok(session) => match session.run(tx, rx) {
                Ok(()) => return, // UI went away
                Err(e) => e,
            },
            Err(e) => e,
        };
        if tx.send(Event::Offline(reason)).is_err() {
            return;
        }
        // wait before retrying, discarding commands that cannot be served while offline
        let deadline = std::time::Instant::now() + RETRY;
        loop {
            match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

struct VehicleLink {
    bus: Box<dyn CanBus>,
    addressing: Addressing,
    adapter: String,
    ecu: u32,
    pids: Vec<u8>,
}

fn connect(bitrate: u32) -> Result<VehicleLink, String> {
    let driver = Op20Driver;
    let adapters = driver.enumerate().map_err(|e| e.to_string())?;
    let adapter = adapters
        .into_iter()
        .find(|a| !a.in_use)
        .ok_or("No free OpenPort 2.0 adapter found (plugged in? udev rule installed? used by another program?)")?;
    let bus = driver
        .open_can(&adapter, &CanConfig { bitrate, accept_extended: true })
        .map_err(|e| e.to_string())?;

    for addressing in [Addressing::Standard, Addressing::Extended] {
        let mut client = ObdClient::new(bus.as_ref(), addressing);
        client.timeout = Duration::from_millis(300);
        let supported = client.supported_pids().map_err(|e| e.to_string())?;
        // the lowest response ID is the engine ECU (7E8)
        if let Some((ecu, pids)) = supported.into_iter().next() {
            let pids = pids.into_iter().filter(|p| pid::info(*p).is_some()).collect();
            return Ok(VehicleLink { bus, addressing, adapter: adapter.description, ecu, pids });
        }
    }
    Err(format!("No ECU answered at {bitrate} bit/s: ignition on, right bitrate?"))
}

impl VehicleLink {
    /// Finds modules and reads their identification (which includes the VIN). `Ok(false)`: UI gone.
    fn scan_modules(&self, tx: &Sender<Event>) -> Result<bool, String> {
        let _ = tx.send(Event::Notice("Scanning for modules…".into()));
        let found = discover::find_modules(self.bus.as_ref(), discover::DEFAULT_RANGE, Duration::from_millis(400)).map_err(|e| e.to_string())?;
        let mut views = Vec::new();
        for m in &found {
            let report = discover::inspect(self.bus.as_ref(), m, Duration::from_millis(250)).map_err(|e| e.to_string())?;
            views.push(module_view(&report));
        }
        let note = format!("Found {} module(s).", views.len());
        Ok(tx.send(Event::Modules(views)).is_ok() && tx.send(Event::Notice(note)).is_ok())
    }

    /// The OBD VIN request (mode 09) is optional and many ECUs ignore it; the UDS identification
    /// data identifier F190 is the usual fallback.
    fn read_vin(&self, client: &ObdClient) -> Result<Option<String>, String> {
        let mut vin = client.read_vin().map_err(|e| e.to_string())?.into_iter().find(|(e, _)| *e == self.ecu).map(|(_, v)| v);
        if vin.is_none() && self.addressing == Addressing::Standard && self.ecu >= 8 {
            let mut uds = UdsClient::new(self.bus.as_ref(), self.ecu - 8, self.ecu);
            uds.timeout = Duration::from_millis(250);
            if let Ok(Some(candriver_uds::DidResult::Data(d))) = uds.read_did(0xf190) {
                let v = show_bytes(&d);
                vin = (v.len() == 17).then_some(v);
            }
        }
        Ok(vin)
    }

    /// Runs until the connection fails (`Err`) or the UI goes away (`Ok`).
    fn run(&self, tx: &Sender<Event>, rx: &Receiver<Cmd>) -> Result<(), String> {
        let mut client = ObdClient::new(self.bus.as_ref(), self.addressing);
        client.timeout = Duration::from_millis(300);
        let channels = self.pids.iter().filter_map(|p| obd_channel(*p)).collect();
        let up = tx.send(Event::Connected { adapter: self.adapter.clone(), ecu: Some(self.ecu), channels });
        if up.is_err() {
            return Ok(());
        }
        if let Some(vin) = self.read_vin(&client)?
            && tx.send(Event::Vehicle { vin }).is_err()
        {
            return Ok(());
        }
        if !self.send_dtcs(&client, tx)? {
            return Ok(());
        }
        let mut misses = 0;
        loop {
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    Cmd::ReadDtcs if !self.send_dtcs(&client, tx)? => return Ok(()),
                    Cmd::ReadDtcs => {}
                    Cmd::ReadVin => {
                        let event = match self.read_vin(&client)? {
                            Some(vin) => Event::Vehicle { vin },
                            // the engine ECU does not say: other modules usually carry the VIN
                            None => {
                                let _ = tx.send(Event::Notice("The engine ECU gave no VIN; asking the other modules…".into()));
                                if !self.scan_modules(tx)? {
                                    return Ok(());
                                }
                                continue;
                            }
                        };
                        if tx.send(event).is_err() {
                            return Ok(());
                        }
                    }
                    Cmd::ScanModules if !self.scan_modules(tx)? => return Ok(()),
                    Cmd::ScanModules => {}
                    Cmd::SweepDids { tx_id } => {
                        let _ = tx.send(Event::Notice(format!("Reading data identifiers of module {tx_id:03X}…")));
                        let mut client = UdsClient::new(self.bus.as_ref(), tx_id, tx_id + 8);
                        client.timeout = Duration::from_millis(150);
                        let found = discover::sweep_dids(&client, 0xf100..=0xf1ff).map_err(|e| e.to_string())?;
                        let note = format!("Module {tx_id:03X}: {} data identifier(s) found.", found.len());
                        if tx.send(Event::Dids { tx_id, lines: did_lines(found) }).is_err() || tx.send(Event::Notice(note)).is_err() {
                            return Ok(());
                        }
                    }
                    Cmd::SweepRoutines { targets } => {
                        let _ = tx.send(Event::Notice(format!("Asking {} module(s) which routines exist…", targets.len())));
                        let lanes: Vec<discover::RoutineTarget> =
                            targets.iter().map(|&(tx_id, from)| discover::RoutineTarget { tx_id, rx_id: tx_id + 8, from }).collect();
                        let total_work: f64 = lanes.iter().map(|l| (0x1_0000 - l.from as u32) as f64).sum();
                        let started = std::time::Instant::now();
                        let mut calls = 0u32;
                        let mut ui_gone = false;
                        let results = discover::sweep_routines_multi(self.bus.as_ref(), &lanes, &mut |state| {
                            calls += 1;
                            // a module that has ended (finished, or given up on) has no work left
                            let done: f64 = state
                                .iter()
                                .zip(&lanes)
                                .map(|(s, l)| if s.done { (0x1_0000 - l.from as u32) as f64 } else { (s.next.max(l.from) - l.from) as f64 })
                                .sum();
                            // the rate counts only probes actually sent
                            let probed: f64 = state.iter().zip(&lanes).map(|(s, l)| (s.next.max(l.from) - l.from) as f64).sum();
                            let slowest = state.iter().filter(|s| !s.done).min_by_key(|s| s.next).map_or((0, 0xffff), |s| (s.tx_id, s.next));
                            let rate = probed / started.elapsed().as_secs_f64().max(0.001);
                            let mut ok = tx.send(Event::RoutineProgress { rate, done: done / total_work, slowest }).is_ok();
                            // save what has been found every so often, in case the app or the car goes away
                            if ok && calls % 32 == 0 {
                                for s in state.iter().filter(|s| !s.done) {
                                    let lines = s.found.iter().map(|(id, f)| RoutineLine { id: *id, state: routine_state(f) }).collect();
                                    ok &= tx.send(Event::RoutineCheckpoint { tx_id: s.tx_id, lines, next: s.next }).is_ok();
                                }
                            }
                            if !ok {
                                ui_gone = true;
                                return false;
                            }
                            // only a cancel matters while sweeping
                            let mut keep_going = true;
                            while let Ok(c) = rx.try_recv() {
                                keep_going &= !matches!(c, Cmd::CancelSweep);
                            }
                            keep_going
                        })
                        .map_err(|e| e.to_string())?;
                        if ui_gone {
                            return Ok(());
                        }
                        for r in results {
                            let (tx_id, found, end) = (r.tx_id, r.found, r.end);
                            let (end_text, complete, next) = sweep_end_text(&end, r.session);
                            let lines = found.into_iter().map(|(id, f)| RoutineLine { id, state: routine_state(&f) }).collect();
                            if tx.send(Event::Routines { tx_id, lines, end: end_text, complete, next }).is_err() {
                                return Ok(());
                            }
                        }
                    }
                    Cmd::CancelSweep => {}
                    Cmd::ClearDtcs => {
                        let notice = clear_notice(&client.clear_dtcs().map_err(|e| e.to_string())?);
                        if tx.send(Event::Notice(notice)).is_err() || !self.send_dtcs(&client, tx)? {
                            return Ok(());
                        }
                    }
                }
            }
            let mut answered = false;
            let cycle_start = std::time::Instant::now();
            // ask the engine ECU directly, several PIDs per request
            let data = client.read_pids_from(self.ecu, &self.pids).map_err(|e| e.to_string())?;
            for &p in &self.pids {
                if let Some(value) = data.get(&p).and_then(|d| pid::decode(p, d)) {
                    answered = true;
                    if tx.send(Event::Reading { key: obd_key(p), value }).is_err() {
                        return Ok(());
                    }
                }
            }
            if std::env::var_os("CANFORGE_TRACE").is_some() {
                eprintln!("poll cycle: {} PIDs in {:?}", self.pids.len(), cycle_start.elapsed());
            }
            misses = if answered { 0 } else { misses + 1 };
            if misses >= 3 {
                return Err("The ECU stopped answering".into());
            }
        }
    }

    /// Returns `Ok(false)` if the UI is gone.
    fn send_dtcs(&self, client: &ObdClient, tx: &Sender<Event>) -> Result<bool, String> {
        let mil_on = client
            .mil_status()
            .map_err(|e| e.to_string())?
            .iter()
            .any(|(_, s)| s.mil_on);
        let mut codes = Vec::new();
        for (kind, label) in [(DtcKind::Stored, "stored"), (DtcKind::Pending, "pending")] {
            for (ecu, list) in client.read_dtcs(kind).map_err(|e| e.to_string())? {
                codes.extend(list.into_iter().flatten().map(|code| DtcEntry { ecu, kind: label, code }));
            }
        }
        let has_stored = codes.iter().any(|c| c.kind == "stored");
        let sent = tx.send(Event::Dtcs { mil_on, codes }).is_ok();

        let ready = client.readiness().map_err(|e| e.to_string())?;
        if let Some((_, r)) = ready.into_iter().find(|(e, _)| *e == self.ecu)
            && tx.send(Event::Readiness(r)).is_err()
        {
            return Ok(false);
        }
        Ok(sent && tx.send(Event::Freeze(self.freeze_frame(client, has_stored)?)).is_ok())
    }

    /// The engine ECU's freeze frame, only worth asking for when a code is stored.
    fn freeze_frame(&self, client: &ObdClient, has_stored: bool) -> Result<Option<FreezeFrame>, String> {
        if !has_stored {
            return Ok(None);
        }
        let dtc = client.freeze_frame_dtc().map_err(|e| e.to_string())?;
        let Some((_, dtc)) = dtc.into_iter().find(|(e, _)| *e == self.ecu) else { return Ok(None) };
        let mut values = Vec::new();
        for &p in FREEZE_PIDS.iter().filter(|p| self.pids.contains(p)) {
            let replies = client.read_freeze_frame(p, 0).map_err(|e| e.to_string())?;
            if let Some(v) = replies.iter().find(|(e, _)| *e == self.ecu).and_then(|(_, d)| pid::decode(p, d)) {
                values.push((p, v));
            }
        }
        Ok(Some(FreezeFrame { dtc, values }))
    }
}

fn clear_notice(results: &[(u32, ClearResult)]) -> String {
    if results.is_empty() {
        return "No ECU answered the clear request.".into();
    }
    let parts: Vec<String> = results
        .iter()
        .map(|(ecu, r)| match r {
            ClearResult::Cleared => format!("ECU {ecu:03X}: cleared"),
            ClearResult::Refused(0x22) => format!("ECU {ecu:03X}: refused (conditions not correct, is the engine running?)"),
            ClearResult::Refused(nrc) => format!("ECU {ecu:03X}: refused (code {nrc:#04x})"),
        })
        .collect();
    parts.join(" · ")
}

/// ASCII if it is text, hex otherwise.
fn show_bytes(d: &[u8]) -> String {
    let text = d.iter().all(|b| (0x20..0x7f).contains(b) || *b == 0);
    if text && d.iter().any(|b| *b != 0) {
        d.iter().filter(|b| **b != 0).map(|&b| b as char).collect::<String>().trim().to_string()
    } else {
        d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
    }
}

fn dtc_view(d: &UdsDtc) -> ModuleDtc {
    let status = if d.confirmed() {
        "confirmed"
    } else if d.pending() {
        "pending"
    } else {
        "history"
    };
    ModuleDtc { code: d.display(), status }
}

fn service_label(s: Support) -> &'static str {
    match s {
        Support::Yes => "yes",
        Support::No => "no",
        Support::OtherSession => "other session",
        Support::Unknown => "no answer",
    }
}

fn module_view(r: &discover::ModuleReport) -> ModuleView {
    let (dtcs, dtc_note) = match &r.dtcs {
        Some(DtcReport::Codes(c)) => (c.iter().map(dtc_view).collect(), None),
        Some(DtcReport::Refused(nrc)) => (Vec::new(), Some(format!("refused: {}", nrc_name(*nrc)))),
        None => (Vec::new(), Some("no answer".into())),
    };
    ModuleView {
        tx_id: r.tx_id,
        rx_id: r.rx_id,
        identity: r
            .identity
            .iter()
            .map(|(did, d)| (discover::did_name(*did).unwrap_or("data").to_string(), show_bytes(d)))
            .collect(),
        dtcs,
        dtc_note,
        services: r.services.iter().map(|(_, name, s)| (name.to_string(), service_label(*s))).collect(),
    }
}

/// (what to tell the user, whether this module needs no further scanning, where to resume otherwise)
fn sweep_end_text(end: &discover::SweepEnd, session: discover::SessionState) -> (String, bool, Option<u16>) {
    use discover::SessionState as S;
    use discover::SweepEnd::*;
    let session_note = match session {
        S::Default => "default session".to_string(),
        S::Extended => "extended session".to_string(),
        S::Refused(nrc) => format!("extended session refused: {}", nrc_name(nrc)),
        S::NoAnswer => "extended session: no answer".to_string(),
    };
    let (text, complete, next) = match end {
        Finished => ("finished".to_string(), true, None),
        Cancelled(next) => (format!("stopped at {next:04X}"), false, Some(*next)),
        Unsupported(0x7f | 0x7e) => ("routine control is not offered in this session".into(), true, None),
        Unsupported(0x12) => ("routine control answered, but not the request-results sub-function".into(), true, None),
        Unsupported(_) => ("the module does not offer routine control".into(), true, None),
        Silent(next) => (format!("the module stopped answering at {next:04X}"), false, Some(*next)),
        Uninformative(nrc) => (format!("answered every identifier alike ({}): nothing learned", nrc_name(*nrc)), true, None),
    };
    (format!("{text} ({session_note})"), complete, next)
}

fn routine_state(f: &discover::RoutineFinding) -> String {
    match f {
        discover::RoutineFinding::Results(d) if d.is_empty() => "has results (empty)".into(),
        discover::RoutineFinding::Results(d) => format!("has results: {}", show_bytes(d)),
        discover::RoutineFinding::Exists(0x24) => "exists, never started".into(),
        discover::RoutineFinding::Exists(nrc) => format!("exists, no results: {}", nrc_name(*nrc)),
    }
}

fn did_lines(found: Vec<(u16, DidFinding)>) -> Vec<DidLine> {
    found
        .into_iter()
        .map(|(did, f)| DidLine {
            did,
            name: discover::did_name(did).unwrap_or("").to_string(),
            value: match f {
                DidFinding::Data(d) => show_bytes(&d),
                DidFinding::Protected(nrc) => format!("exists, not readable: {}", nrc_name(nrc)),
            },
        })
        .collect()
}

/// Measures how fast routine sweeps run on this car: the same `count` identifiers on every module,
/// all modules at once and then one module at a time. Read-only, like the sweep itself.
pub fn bench_routines(bitrate: u32, count: u16) -> Result<String, String> {
    use std::fmt::Write as _;
    let link = connect(bitrate)?;
    let found = discover::find_modules(link.bus.as_ref(), discover::DEFAULT_RANGE, Duration::from_millis(400)).map_err(|e| e.to_string())?;
    if found.is_empty() {
        return Err("No module answered.".into());
    }
    let first = 0x0200u16;
    let options = discover::SweepOptions { early_stop: false, last: first + count.max(1) - 1, extended_session: true };
    let targets: Vec<discover::RoutineTarget> =
        found.iter().map(|m| discover::RoutineTarget { tx_id: m.tx_id, rx_id: m.rx_id, from: first }).collect();
    let mut out = String::new();
    let _ = writeln!(out, "{} module(s), ids {first:04X}..{:04X} ({count} each)", found.len(), options.last);

    let t = std::time::Instant::now();
    let together = discover::sweep_routines_multi_with(link.bus.as_ref(), &targets, options, &mut |_| true).map_err(|e| e.to_string())?;
    let together_time = t.elapsed();
    let _ = writeln!(out, "\nall modules at once: {:.1} s, {:.0} probes/s in total", together_time.as_secs_f64(), (count as usize * targets.len()) as f64 / together_time.as_secs_f64());
    let _ = writeln!(out, "  module   answered   silent   reply time   session   answers");
    for r in &together {
        let answers: Vec<String> = r
            .tally
            .iter()
            .map(|(code, n)| if *code == 0 { format!("{n}x positive") } else { format!("{n}x {}", nrc_name(*code)) })
            .collect();
        let _ = writeln!(
            out,
            "  {:03X}      {:>8}   {:>6}   {:>9}   {:?}   {}",
            r.tx_id,
            r.answered,
            r.silent,
            r.rtt.map_or("-".into(), |d| format!("{:.1} ms", d.as_secs_f64() * 1000.0)),
            r.session,
            answers.join(", ")
        );
    }

    let mut alone_total = Duration::ZERO;
    let _ = writeln!(out, "\none module at a time:");
    for target in &targets {
        let t = std::time::Instant::now();
        discover::sweep_routines_multi_with(link.bus.as_ref(), std::slice::from_ref(target), options, &mut |_| true).map_err(|e| e.to_string())?;
        let took = t.elapsed();
        alone_total += took;
        let _ = writeln!(out, "  {:03X}  {:.1} s, {:.0} probes/s", target.tx_id, took.as_secs_f64(), count as f64 / took.as_secs_f64());
    }
    let _ = writeln!(
        out,
        "\nsequential total {:.1} s vs {:.1} s together: {:.1}x faster",
        alone_total.as_secs_f64(),
        together_time.as_secs_f64(),
        alone_total.as_secs_f64() / together_time.as_secs_f64()
    );
    let full = together_time.as_secs_f64() * 65536.0 / count as f64;
    let _ = writeln!(out, "estimated full 0000–FFFF sweep of all modules at this speed: {:.0} min", full / 60.0);
    Ok(out)
}
