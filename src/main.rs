mod backend;
mod contribute;
mod dbcmd;
mod dump;
mod dtcdb;
mod garage;
mod import;
mod modnames;
mod session;
mod sim;
mod vin;

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use backend::{Backend, Cmd, DidLine, DtcEntry, Event, FreezeFrame, ModuleView, RoutineScan};
use session::Channel;
use candriver_obd::readiness::Readiness;
use candriver_obd::pid;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, Theme, ThemeMode, TitleBar,
    button::{Button, ButtonVariants as _},
    chart::AreaChart,
    h_flex,
    progress::Progress,
    input::{Input, InputState},
    select::{Select, SelectEvent, SelectState},
    table::{Column, DataTable, TableDelegate, TableState},
    v_flex,
};
use gpui_kit::base::Disableable as _;
use gpui_kit::{prelude::FluentBuilder as _, *};

/// How often the UI looks for new data from the worker.
const PUMP: Duration = Duration::from_millis(50);
/// Chart history resolution.
const TICK: Duration = Duration::from_millis(250);
const HISTORY: usize = 120;

#[derive(Clone)]
struct Sample {
    time: String,
    /// One value per entry of `Diagnostics::chart_keys`.
    values: Vec<f64>,
}

struct DtcDelegate {
    codes: Vec<DtcEntry>,
    /// Selects manufacturer-specific descriptions.
    make: Option<String>,
    /// Descriptions the user has just added this session (code → text).
    own: BTreeMap<String, String>,
    columns: Vec<Column>,
}

impl TableDelegate for DtcDelegate {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.codes.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns[col_ix].clone()
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(entry) = self.codes.get(row_ix) else {
            return div().into_any_element();
        };
        let code = entry.code.to_string();
        match col_ix {
            0 => div().text_color(cx.theme().red).child(code),
            1 => div().child(entry.kind),
            2 => div().child(format!("{:03X}", entry.ecu)),
            _ => {
                let info = dtcdb::get().lookup(&code, self.make.as_deref());
                let description = match (&info.scope, self.own.get(&code)) {
                    (dtcdb::Scope::Unknown, Some(own)) => own.clone(),
                    _ => info.description,
                };
                let text = div().child(description);
                match info.scope {
                    dtcdb::Scope::Manufacturer => text.child(
                        div().text_xs().text_color(cx.theme().muted_foreground).child(
                            self.make.as_deref().unwrap_or_default().to_uppercase(),
                        ),
                    ),
                    _ => text,
                }
            }
        }
        .into_any_element()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Codes,
    Live,
    Readiness,
    Modules,
    Contribute,
    Vehicles,
}

impl Page {
    const ALL: [Page; 6] = [Page::Overview, Page::Codes, Page::Live, Page::Readiness, Page::Modules, Page::Contribute];

    fn label(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Codes => "Fault codes",
            Page::Live => "Live data",
            Page::Readiness => "Readiness",
            Page::Modules => "Modules",
            Page::Contribute => "Help improve",
            Page::Vehicles => "Vehicles",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Page::Overview => IconName::LayoutDashboard,
            Page::Codes => IconName::TriangleAlert,
            Page::Live => IconName::ChartPie,
            Page::Readiness => IconName::CircleCheck,
            Page::Modules => IconName::Cpu,
            Page::Contribute => IconName::BookOpen,
            Page::Vehicles => IconName::Frame,
        }
    }
}

enum Link {
    Connecting,
    Online { adapter: String, ecu: Option<u32> },
    Offline(String),
}

struct Diagnostics {
    backend: Backend,
    link: Link,
    page: Page,
    /// What the source provides, in display order.
    channels: Vec<Channel>,
    /// Channels drawn as history charts (at most two).
    chart_keys: Vec<String>,
    readings: BTreeMap<String, f64>,
    history: VecDeque<Sample>,
    ticks: usize,
    last_sample: std::time::Instant,
    mil_on: bool,
    /// Waiting for the user to confirm a clear.
    confirm_clear: bool,
    vin: Option<String>,
    /// Make of the current vehicle (or `--make`): selects manufacturer-specific trouble codes.
    make: Option<String>,
    /// `--make` if given: a fixed make that VIN detection does not change.
    make_override: Option<String>,
    /// A module scan was started for this connection.
    scan_requested: bool,
    /// A full vehicle export is waiting for the module scan and data identifier sweeps.
    dump_pending: bool,
    /// The export also asks every module which routines exist (slow).
    dump_routines: bool,
    routine_running: bool,
    routine_started: std::time::Instant,
    /// Modules the running routine sweep has yet to report on.
    pending_routines: usize,
    /// Findings of the running sweep, not yet final (this run only).
    routine_live: BTreeMap<u32, (Vec<backend::RoutineLine>, u16)>,
    /// A saved routine scan for this vehicle is unfinished.
    resumable: bool,
    dump_cancelled: bool,
    routines: BTreeMap<u32, RoutineScan>,
    vin_requested: bool,
    vehicles: Vec<garage::Vehicle>,
    /// Index into `vehicles`.
    current: Option<usize>,
    /// The add-vehicle form.
    name_input: Entity<InputState>,
    vin_input: Entity<InputState>,
    form_make: Option<String>,
    readiness: Option<Readiness>,
    freeze: Option<FreezeFrame>,
    notice: Option<String>,
    make_select: Entity<SelectState<Vec<String>>>,
    /// Describes the vehicle for the `source` of contributed entries (never a VIN).
    note_input: Entity<InputState>,
    /// One text box per module / code being named, keyed `mod:720` / `dtc:P1572`.
    inputs: BTreeMap<String, Entity<InputState>>,
    /// The user's own entries, this session and earlier ones.
    contrib: Vec<contribute::Entry>,
    contrib_msg: Option<String>,
    _subscriptions: Vec<Subscription>,
    modules: Vec<ModuleView>,
    /// Data identifiers found per module request ID.
    dids: BTreeMap<u32, Vec<DidLine>>,
    codes: Entity<TableState<DtcDelegate>>,
}

impl Diagnostics {
    fn new(backend: Backend, make: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let delegate = DtcDelegate {
            codes: Vec::new(),
            make: make.clone(),
            own: BTreeMap::new(),
            columns: vec![
                Column::new("code", "Code").width(90.),
                Column::new("kind", "Type").width(90.),
                Column::new("ecu", "ECU").width(70.),
                Column::new("desc", "Description").width(400.),
            ],
        };
        let codes = cx.new(|cx| TableState::new(delegate, window, cx));

        let mut makes: Vec<String> = dtcdb::get().makes().into_iter().chain(vin::makes()).map(String::from).collect();
        makes.sort();
        makes.dedup();
        let note_input = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. Ford Fiesta Mk7 1.6 TDCi 2018 (no VIN)"));
        let contrib: Vec<contribute::Entry> = dtcdb::data_dir()
            .map(|d| [contribute::Kind::Module, contribute::Kind::Dtc].into_iter().flat_map(|k| contribute::load(&d, k)).collect())
            .unwrap_or_default();
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("e.g. Golf VII 2.0 TDI"));
        let vin_input = cx.new(|cx| InputState::new(window, cx).placeholder("VIN (optional, recognises the car next time)"));
        let vehicles = dtcdb::data_dir().map(|d| garage::load(&d)).unwrap_or_default();
        let make_select = cx.new(|cx| SelectState::new(makes, None, window, cx).searchable(true));
        let subscription = cx.subscribe_in(&make_select, window, |this, _, event: &SelectEvent<Vec<String>>, _window, cx| {
            if let SelectEvent::Confirm(Some(make)) = event {
                this.form_make = Some(make.clone());
                cx.notify();
            }
        });

        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(PUMP).await;
                if this
                    .update_in(cx, |this, window, cx| {
                        // redraw only when something changed
                        if this.pump(window, cx) {
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            backend,
            link: Link::Connecting,
            page: Page::Overview,
            channels: Vec::new(),
            chart_keys: Vec::new(),
            readings: BTreeMap::new(),
            history: VecDeque::with_capacity(HISTORY),
            ticks: 0,
            last_sample: std::time::Instant::now(),
            mil_on: false,
            confirm_clear: false,
            vin: None,
            make: make.clone(),
            make_override: make.clone(),
            scan_requested: false,
            dump_pending: false,
            dump_routines: false,
            routine_running: false,
            routine_started: std::time::Instant::now(),
            pending_routines: 0,
            routine_live: BTreeMap::new(),
            resumable: false,
            dump_cancelled: false,
            routines: BTreeMap::new(),
            vin_requested: false,
            vehicles,
            current: None,
            name_input,
            vin_input,
            form_make: None,
            readiness: None,
            freeze: None,
            notice: None,
            make_select,
            note_input,
            inputs: BTreeMap::new(),
            contrib,
            contrib_msg: None,
            _subscriptions: vec![subscription],
            modules: Vec::new(),
            dids: BTreeMap::new(),
            codes,
        }
    }

    /// Apply everything the worker thread has sent since the last tick.
    /// Applies pending worker events and extends the chart history. Returns whether anything changed.
    fn pump(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok(event) = self.backend.events.try_recv() {
            changed = true;
            match event {
                Event::Offline(reason) => {
                    self.link = Link::Offline(reason);
                    self.readings.clear();
                }
                Event::Connected { adapter, ecu, channels } => {
                    self.link = Link::Online { adapter, ecu };
                    // prefer engine speed and vehicle speed; otherwise the first two channels
                    let preferred = ["pid:0c", "pid:0d"].map(String::from);
                    self.chart_keys = preferred.iter().filter(|k| channels.iter().any(|c| &c.key == *k)).cloned().collect();
                    if self.chart_keys.is_empty() {
                        self.chart_keys = channels.iter().take(2).map(|c| c.key.clone()).collect();
                    }
                    self.channels = channels;
                    self.scan_requested = false;
                    self.vin_requested = false;
                    self.ensure_modules();
                    self.ensure_vin();
                    self.readings.clear();
                    self.history.clear();
                    self.ticks = 0;
                }
                Event::Reading { key, value } => {
                    self.readings.insert(key, value);
                }
                Event::Notice(text) => self.notice = Some(text),
                Event::Modules(modules) => {
                    // every module reports the VIN: use it if the engine ECU did not
                    if self.vin.is_none() {
                        let vin = modules.iter().flat_map(|m| &m.identity).find(|(k, v)| k == "VIN" && v.len() == 17).map(|(_, v)| v.clone());
                        if let Some(vin) = vin {
                            self.set_vin(vin, window, cx);
                        }
                    }
                    self.modules = modules;
                    self.dids.clear();
                    if self.dump_pending {
                        for m in &self.modules {
                            let _ = self.backend.cmds.send(Cmd::SweepDids { tx_id: m.tx_id });
                        }
                    }
                }
                Event::Dids { tx_id, lines } => {
                    self.dids.insert(tx_id, lines);
                    self.advance_dump(cx);
                }
                Event::RoutineProgress { rate, done, slowest } => {
                    if self.dump_pending {
                        let left = (1.0 - done).max(0.0) / done.max(1e-9) * self.routine_started.elapsed().as_secs_f64();
                        self.contrib_msg = Some(format!(
                            "Routine scan {:.0}%  ·  {rate:.0} IDs/s  ·  about {} left  ·  slowest module {:03X} at {:04X}",
                            done * 100.0,
                            if done > 0.005 { format!("{:.0} min", left / 60.0) } else { "…".to_string() },
                            slowest.0,
                            slowest.1
                        ));
                    }
                }
                Event::RoutineCheckpoint { tx_id, lines, next } => {
                    self.routine_live.insert(tx_id, (lines, next));
                    self.save_scan_progress();
                }
                Event::Routines { tx_id, lines, end, complete, next } => {
                    // a resumed scan adds to what the earlier run found
                    let mut all = self.routines.remove(&tx_id).map(|s| s.lines).unwrap_or_default();
                    all.extend(lines);
                    self.routines.insert(tx_id, RoutineScan { lines: all, end, complete, next });
                    self.routine_live.remove(&tx_id);
                    self.pending_routines = self.pending_routines.saturating_sub(1);
                    self.save_scan_progress();
                    if self.pending_routines == 0 {
                        self.routine_running = false;
                        self.advance_dump(cx);
                    }
                }
                Event::Vehicle { vin } => {
                    self.notice = None;
                    self.set_vin(vin, window, cx)
                }
                Event::Readiness(r) => self.readiness = Some(r),
                Event::Freeze(f) => self.freeze = f,
                Event::Dtcs { mil_on, codes } => {
                    self.mil_on = mil_on;
                    self.codes.update(cx, |table, cx| {
                        table.delegate_mut().codes = codes;
                        cx.notify();
                    });
                }
            }
        }
        if matches!(self.link, Link::Online { .. }) && self.last_sample.elapsed() >= TICK {
            self.last_sample = std::time::Instant::now();
            changed = true;
            if self.history.len() >= HISTORY {
                self.history.pop_front();
            }
            self.history.push_back(Sample {
                time: format!("{:.0}s", self.ticks as f64 * TICK.as_secs_f64()),
                values: self.chart_keys.iter().map(|k| self.readings.get(k).copied().unwrap_or(0.0)).collect(),
            });
            self.ticks += 1;
        }
        changed
    }

    /// A VIN was read: recognise the vehicle, or add it when the VIN names a make.
    fn set_vin(&mut self, vin: String, window: &mut Window, cx: &mut Context<Self>) {
        self.vin = Some(vin.clone());
        self.refresh_resumable();
        if self.make_override.is_some() {
            return;
        }
        match garage::identify(&self.vehicles, self.current, &vin) {
            garage::Identity::Known(ix) => self.select_vehicle(ix, window, cx),
            garage::Identity::Attach(ix) => {
                self.vehicles[ix].vin = Some(vin);
                self.persist_garage();
                self.select_vehicle(ix, window, cx);
            }
            garage::Identity::New(make) => {
                let name = make.split(' ').map(|w| w[..1].to_string() + &w[1..].to_lowercase()).collect::<Vec<_>>().join(" ");
                self.vehicles.push(garage::Vehicle { name, make: make.into(), vin: Some(vin) });
                self.persist_garage();
                self.select_vehicle(self.vehicles.len() - 1, window, cx);
            }
            garage::Identity::Unknown => {}
        }
    }

    /// The Modules and Help improve pages need the module list: scan once per connection.
    fn ensure_modules(&mut self) {
        let wanted = matches!(self.page, Page::Modules | Page::Contribute);
        if wanted && !self.scan_requested && self.modules.is_empty() && matches!(self.link, Link::Online { .. }) {
            self.scan_requested = true;
            let _ = self.backend.cmds.send(Cmd::ScanModules);
        }
    }

    /// Ask the car for its VIN, once per connection, when the vehicle page is opened without one.
    fn ensure_vin(&mut self) {
        if self.page == Page::Vehicles && self.vin.is_none() && !self.vin_requested && matches!(self.link, Link::Online { .. }) {
            self.vin_requested = true;
            let _ = self.backend.cmds.send(Cmd::ReadVin);
        }
    }

    fn persist_garage(&mut self) {
        if let Some(dir) = dtcdb::data_dir() {
            if let Err(e) = garage::save(&dir, &self.vehicles) {
                self.notice = Some(format!("Could not save the garage: {e}"));
            }
        }
    }

    fn select_vehicle(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(make) = self.vehicles.get(ix).map(|v| v.make.clone()) else { return };
        self.current = Some(ix);
        self.make_override = None;
        self.refresh_resumable();
        self.set_make(Some(make), window, cx);
        cx.notify();
    }

    fn remove_vehicle(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.vehicles.len() {
            return;
        }
        self.vehicles.remove(ix);
        self.persist_garage();
        match self.current {
            Some(c) if c == ix => {
                self.current = None;
                self.set_make(None, window, cx);
            }
            Some(c) if c > ix => self.current = Some(c - 1),
            _ => {}
        }
        cx.notify();
    }

    fn add_vehicle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).value().trim().to_string();
        let vin = self.vin_input.read(cx).value().trim().to_ascii_uppercase();
        let Some(make) = self.form_make.clone().or_else(|| (vin.len() == 17).then(|| vin::make(&vin)).flatten().map(String::from)) else {
            self.notice = Some("Choose the vehicle's make (or enter its VIN).".into());
            cx.notify();
            return;
        };
        let name = if name.is_empty() { make.clone() } else { name };
        self.vehicles.push(garage::Vehicle { name, make, vin: (!vin.is_empty()).then_some(vin) });
        self.persist_garage();
        self.name_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.vin_input.update(cx, |s, cx| s.set_value("", window, cx));
        self.notice = None;
        self.select_vehicle(self.vehicles.len() - 1, window, cx);
    }

    fn set_make(&mut self, make: Option<String>, _window: &mut Window, cx: &mut Context<Self>) {
        self.make = make.clone();
        self.sync_own_descriptions(make, cx);
    }

    /// Hands the delegate the user's own descriptions for `make`.
    fn sync_own_descriptions(&mut self, make: Option<String>, cx: &mut Context<Self>) {
        let own: BTreeMap<String, String> = self
            .contrib
            .iter()
            .filter(|e| e.kind == contribute::Kind::Dtc && make.as_deref().is_some_and(|m| e.make.eq_ignore_ascii_case(m)))
            .map(|e| (e.id.clone(), e.text.clone()))
            .collect();
        self.codes.update(cx, |table, cx| {
            let d = table.delegate_mut();
            d.make = make;
            d.own = own;
            cx.notify();
        });
    }

    /// The user's own name for a module, if they gave one.
    fn own_module_name(&self, tx_id: u32) -> Option<&str> {
        let make = self.make.as_deref()?;
        let id = format!("{tx_id:03X}");
        self.contrib
            .iter()
            .find(|e| e.kind == contribute::Kind::Module && e.id == id && e.make.eq_ignore_ascii_case(make))
            .map(|e| e.text.as_str())
    }

    /// The user's own description for a trouble code, if they gave one.
    fn own_description(&self, code: &str) -> Option<&str> {
        let make = self.make.as_deref()?;
        self.contrib
            .iter()
            .find(|e| e.kind == contribute::Kind::Dtc && e.id == code && e.make.eq_ignore_ascii_case(make))
            .map(|e| e.text.as_str())
    }

    fn save_contribution(&mut self, kind: contribute::Kind, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let key = format!("{}:{id}", if kind == contribute::Kind::Module { "mod" } else { "dtc" });
        let Some(input) = self.inputs.get(&key).cloned() else { return };
        let text = input.read(cx).value().to_string();
        let note = self.note_input.read(cx).value().to_string();
        let make = self.make.clone().unwrap_or_default();
        let Some(dir) = dtcdb::data_dir() else {
            self.contrib_msg = Some("No data directory available (set CANFORGE_DATA_DIR).".into());
            return;
        };
        match contribute::save(&dir, kind, &id, &make, &text, &note) {
            Ok(entry) => {
                self.contrib.retain(|e| !(e.kind == entry.kind && e.id == entry.id && e.make == entry.make));
                self.contrib.push(entry);
                input.update(cx, |s, cx| s.set_value("", window, cx));
                self.contrib_msg = Some(format!("Saved {id}. It is used right away and kept on this computer."));
                self.sync_own_descriptions(self.make.clone(), cx);
            }
            Err(e) => self.contrib_msg = Some(e),
        }
        cx.notify();
    }

    /// Collects everything readable from the vehicle into one JSON file: scans the modules, sweeps
    /// every module's data identifiers, then writes it (see `finish_dump_if_ready`).
    fn start_dump(&mut self, routines: bool, cx: &mut Context<Self>) {
        if !matches!(self.link, Link::Online { .. }) {
            self.contrib_msg = Some("Connect to a vehicle first.".into());
        } else {
            self.dump_pending = true;
            self.dump_routines = routines;
            self.dump_cancelled = false;
            self.routine_running = false;
            self.routines = match (routines, dtcdb::data_dir()) {
                (true, Some(dir)) => dump::progress::load(&dir, &self.scan_key()),
                _ => BTreeMap::new(),
            };
            self.dids.clear();
            if self.modules.is_empty() {
                self.contrib_msg = Some("Scanning modules, then reading their data identifiers…".into());
                let _ = self.backend.cmds.send(Cmd::ScanModules);
            } else {
                self.contrib_msg = Some("Reading every module's data identifiers…".into());
                for m in &self.modules {
                    let _ = self.backend.cmds.send(Cmd::SweepDids { tx_id: m.tx_id });
                }
            }
        }
        cx.notify();
    }

    /// Identifies this vehicle's saved routine scan: its VIN, else its name.
    fn scan_key(&self) -> String {
        self.vin
            .clone()
            .or_else(|| self.current.and_then(|ix| self.vehicles.get(ix)).map(|v| v.name.clone()))
            .or_else(|| self.make.clone())
            .unwrap_or_else(|| "vehicle".into())
    }

    fn save_scan_progress(&mut self) {
        if let Some(dir) = dtcdb::data_dir() {
            // finished runs, plus what the running sweep has found so far
            let mut all = self.routines.clone();
            for (tx_id, (lines, next)) in &self.routine_live {
                let base = all.remove(tx_id);
                let mut merged = base.as_ref().map(|b| b.lines.clone()).unwrap_or_default();
                merged.extend(lines.iter().cloned());
                all.insert(*tx_id, RoutineScan { lines: merged, end: "in progress".into(), complete: false, next: Some(*next) });
            }
            if let Err(e) = dump::progress::save(&dir, &self.scan_key(), &all) {
                self.contrib_msg = Some(e);
            }
        }
        self.refresh_resumable();
    }

    /// Whether the saved scan for the current vehicle still has modules left to do.
    fn refresh_resumable(&mut self) {
        self.resumable = dtcdb::data_dir().is_some_and(|dir| {
            let saved = dump::progress::load(&dir, &self.scan_key());
            !saved.is_empty() && (saved.values().any(|s| s.next.is_some()) || saved.len() < self.modules.len())
        });
    }

    fn cancel_dump_routines(&mut self, cx: &mut Context<Self>) {
        self.dump_cancelled = true;
        let _ = self.backend.cmds.send(Cmd::CancelSweep);
        self.contrib_msg = Some("Stopping the routine scan (progress is kept so it can resume)…".into());
        cx.notify();
    }

    /// Moves the export along: data identifiers, then (if asked) one module's routine sweep at a time, then the file.
    fn advance_dump(&mut self, cx: &App) {
        if !self.dump_pending {
            return;
        }
        let done = self.dids.len();
        if done < self.modules.len() {
            self.contrib_msg = Some(format!("Reading data identifiers: {done} of {} modules…", self.modules.len()));
            return;
        }
        if self.dump_routines && !self.dump_cancelled {
            if self.routine_running {
                return;
            }
            let todo: Vec<(u32, u16)> = self
                .modules
                .iter()
                .filter_map(|m| match self.routines.get(&m.tx_id) {
                    None => Some((m.tx_id, 0)),
                    Some(s) => s.next.map(|n| (m.tx_id, n)),
                })
                .collect();
            if !todo.is_empty() {
                self.routine_running = true;
                self.routine_started = std::time::Instant::now();
                self.pending_routines = todo.len();
                let _ = self.backend.cmds.send(Cmd::SweepRoutines { targets: todo });
                return;
            }
        } else if self.routine_running {
            return; // wait for the cancelled sweep to hand back what it has
        }
        self.dump_pending = false;
        let Some(dir) = dtcdb::data_dir() else {
            self.contrib_msg = Some("No data directory available (set CANFORGE_DATA_DIR).".into());
            return;
        };
        let current = self.current.and_then(|ix| self.vehicles.get(ix));
        let (adapter, ecu) = match &self.link {
            Link::Online { adapter, ecu } => (adapter.as_str(), *ecu),
            _ => ("", None),
        };
        let snapshot = dump::Snapshot {
            name: current.map(|v| v.name.as_str()),
            make: self.make.as_deref(),
            vin: self.vin.as_deref(),
            adapter,
            ecu,
            mil_on: self.mil_on,
            channels: &self.channels,
            readings: &self.readings,
            codes: self.codes.read(cx).delegate().codes.iter().collect(),
            readiness: self.readiness.as_ref(),
            freeze: self.freeze.as_ref(),
            modules: &self.modules,
            dids: &self.dids,
            routines: self.dump_routines.then_some(&self.routines),
        };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let label = current.map(|v| v.name.clone()).or_else(|| self.make.clone()).unwrap_or_else(|| "vehicle".into());
        let result = dump::write(&dir, &label, &dump::build(&snapshot, now), now);
        self.refresh_resumable();
        self.contrib_msg = Some(match result {
            Ok(path) => format!("Wrote everything the vehicle revealed to {}. It contains the VIN: keep it private.", path.display()),
            Err(e) => e,
        });
    }

    fn export_contributions(&mut self, cx: &mut Context<Self>) {
        self.contrib_msg = Some(match dtcdb::data_dir() {
            None => "No data directory available (set CANFORGE_DATA_DIR).".into(),
            Some(dir) => match contribute::export(&dir, &dir.join("contribution")) {
                Ok(files) => format!(
                    "Wrote {} file(s) to {}. In a checkout of vehicle-db run:  python3 tools/merge.py {}  then open a pull request.",
                    files.len(),
                    dir.join("contribution").display(),
                    dir.join("contribution").display()
                ),
                Err(e) => e,
            },
        });
        cx.notify();
    }

    fn input_for(&mut self, key: String, placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        self.inputs.entry(key).or_insert_with(|| cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))).clone()
    }

    fn render_contribute(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let make = self.make.clone();
        let db = dtcdb::get();
        let resumable = self.resumable;

        // modules the shared data cannot name, and codes it cannot describe
        let unnamed: Vec<(u32, String)> = self
            .modules
            .iter()
            .filter(|m| {
                !m.identity.iter().any(|(k, _)| k == "System name")
                    && modnames::get().name(m.tx_id, make.as_deref()).is_none()
                    && self.own_module_name(m.tx_id).is_none()
            })
            .map(|m| {
                let part = m.identity.iter().find(|(k, _)| k == "ECU software number").map(|(_, v)| v.as_str()).unwrap_or("");
                (m.tx_id, format!("{:03X} → {:03X}   {part}", m.tx_id, m.rx_id))
            })
            .collect();
        let mut codes: Vec<String> = self.modules.iter().flat_map(|m| m.dtcs.iter().map(|d| d.code[..5].to_string())).collect();
        codes.extend(self.codes.read(cx).delegate().codes.iter().map(|e| e.code.to_string()));
        codes.sort();
        codes.dedup();
        codes.retain(|c| db.lookup(c, make.as_deref()).scope == dtcdb::Scope::Unknown && self.own_description(c).is_none());

        let mut rows: Vec<AnyElement> = Vec::new();
        for (tx_id, label) in unnamed {
            let input = self.input_for(format!("mod:{tx_id:03X}"), "What is this module? (e.g. Instrument cluster)", window, cx);
            let id = format!("{tx_id:03X}");
            rows.push(self.contribute_row(label, input, contribute::Kind::Module, id, cx));
        }
        for code in codes {
            let input = self.input_for(format!("dtc:{code}"), "What does this code mean?", window, cx);
            rows.push(self.contribute_row(code.clone(), input, contribute::Kind::Dtc, code, cx));
        }

        v_flex()
            .id("contribute")
            .p_6()
            .gap_3()
            .size_full()
            .overflow_y_scroll()
            .child(
                div().text_xs().text_color(theme.muted_foreground).child(
                    "Name modules and describe codes the shared database does not know yet. Only enter what you worked out \
                     from your own vehicle: the label on the part, what happens when you use that system. Never copy from \
                     other diagnostic software. Entries are used immediately and stay on this computer until you export them.",
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_xs().w(px(120.)).text_color(theme.muted_foreground).child("This vehicle"))
                    .child(div().flex_1().child(Input::new(&self.note_input))),
            )
            .child(div().text_xs().text_color(theme.muted_foreground).child(match &make {
                Some(m) => format!("Make: {m}. Describe the model and engine above, not the VIN."),
                None => "Choose the vehicle's make first (selector at the bottom right or the banner).".to_string(),
            }))
            .children(rows.is_empty().then(|| {
                div().text_sm().text_color(theme.muted_foreground).child(if self.modules.is_empty() {
                    "Scanning the vehicle for modules… (or press Scan modules on the Modules page)."
                } else {
                    "Nothing is missing for the modules and codes found so far."
                })
            }))
            .children(rows)
            .child(
                v_flex()
                    .gap_2()
                    .pt_2()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Export everything from this vehicle"))
                    .child(div().text_xs().text_color(theme.muted_foreground).child(
                        "Scans every module and reads all its data identifiers, then saves one JSON file with the vehicle, \
                         parameters, trouble codes, readiness, freeze frame, modules, the services each one accepts and their \
                         data. Read-only. The routine scan asks each module about every routine ID 0000–FFFF without starting any; it first switches each module to its extended diagnostic session (and back afterwards), can take many minutes and pauses live data meanwhile. The file contains the VIN: it is for your own analysis, not for sharing.",
                    ))
                    .child(h_flex().gap_2().child(
                        Button::new("dump").icon(IconName::Folder).label("Export vehicle data").disabled(self.dump_pending).on_click(
                            cx.listener(|this, _, _, cx| this.start_dump(false, cx)),
                        ),
                    ).child(
                        Button::new("dump-routines").label(if resumable { "Resume routine scan and export" } else { "Export with routine scan (slow)" }).disabled(self.dump_pending).on_click(
                            cx.listener(|this, _, _, cx| this.start_dump(true, cx)),
                        ),
                    ).when(resumable && !self.dump_pending, |this| {
                        this.child(Button::new("dump-restart").label("Start routine scan over").on_click(cx.listener(|this, _, _, cx| {
                            if let Some(dir) = dtcdb::data_dir() {
                                dump::progress::clear(&dir, &this.scan_key());
                            }
                            this.refresh_resumable();
                            this.contrib_msg = Some("Saved routine scan progress cleared.".into());
                            cx.notify();
                        })))
                    }).when(self.dump_pending && self.dump_routines, |this| {
                        this.child(Button::new("dump-cancel").label("Stop scan").on_click(cx.listener(|this, _, _, cx| this.cancel_dump_routines(cx))))
                    })),
            )
            .children(self.contrib_msg.clone().map(|m| div().text_xs().text_color(theme.foreground).child(m)))
    }

    fn contribute_row(&self, label: String, input: Entity<InputState>, kind: contribute::Kind, id: String, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .gap_2()
            .items_center()
            .child(div().w(px(260.)).text_xs().text_color(theme.foreground).child(label))
            .child(div().flex_1().child(Input::new(&input)))
            .child(Button::new(SharedString::from(format!("save-{id}"))).label("Save").on_click(cx.listener(
                move |this, _, window, cx| this.save_contribution(kind, id.clone(), window, cx),
            )))
            .into_any_element()
    }

    fn code_count(&self, cx: &App) -> usize {
        self.codes.read(cx).delegate().codes.len()
    }

    fn gauge(&self, info: &Channel, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let value = self.readings.get(&info.key).copied();
        let warn = value.is_some_and(|v| info.warn_above.is_some_and(|w| v > w));
        let percent = value
            .map(|v| ((v - info.min) / (info.max - info.min) * 100.0).clamp(0.0, 100.0))
            .unwrap_or(0.0);
        v_flex()
            .gap_1()
            .p_3()
            .w(px(200.))
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .child(div().text_xs().text_color(theme.muted_foreground).child(info.name.clone()))
            .child(
                h_flex()
                    .gap_1()
                    .items_baseline()
                    .child(
                        div()
                            .text_2xl()
                            .text_color(if warn { theme.red } else { theme.foreground })
                            .child(match value {
                                Some(v) => format_value(v, info.max - info.min),
                                None => "—".into(),
                            }),
                    )
                    .child(div().text_xs().text_color(theme.muted_foreground).child(info.unit.clone())),
            )
            .child(Progress::new(SharedString::from(info.key.clone())).h_1().value(percent as f32))
    }

    fn chart(
        &self,
        title: String,
        index: usize,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let data: Vec<Sample> = self.history.iter().cloned().collect();
        v_flex()
            .flex_1()
            .min_h(px(140.))
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .child(div().px_3().py_1().text_sm().text_color(theme.foreground).child(title))
            .child(
                AreaChart::new(data)
                    .x(|s| s.time.clone())
                    .y(move |s: &Sample| s.values.get(index).copied().unwrap_or(0.0))
                    .stroke(color)
                    .fill(linear_gradient(
                        0.,
                        linear_color_stop(color.opacity(0.4), 1.),
                        linear_color_stop(theme.background.opacity(0.1), 0.),
                    ))
                    .tick_margin(15),
            )
    }

    fn render_live(&self, cx: &Context<Self>) -> impl IntoElement {
        if self.channels.is_empty() {
            let msg = match &self.link {
                Link::Offline(reason) => reason.clone(),
                _ => "Waiting for the ECU…".into(),
            };
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(msg)
                .into_any_element();
        }
        v_flex()
            .p_6()
            .gap_4()
            .size_full()
            .child(h_flex().flex_wrap().gap_3().children(self.channels.iter().map(|c| self.gauge(c, cx))))
            .child(
                h_flex()
                    .flex_1()
                    .gap_3()
                    .children(self.chart_keys.iter().enumerate().filter_map(|(i, key)| {
                        let ch = self.channels.iter().find(|c| &c.key == key)?;
                        let title = if ch.unit.is_empty() { ch.name.clone() } else { format!("{} ({})", ch.name, ch.unit) };
                        let color = [cx.theme().red, cx.theme().blue][i % 2];
                        Some(self.chart(title, i, color, cx))
                    })),
            )
            .into_any_element()
    }

    fn render_codes(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .p_6()
            .gap_4()
            .size_full()
            .children(self.notice.clone().map(|n| div().text_sm().text_color(cx.theme().muted_foreground).child(n)))
            .child(
                div()
                    .flex_1()
                    .min_h(px(120.))
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded(cx.theme().radius_lg)
                    .overflow_hidden()
                    .child(DataTable::new(&self.codes).stripe(true).small()),
            )
            .child(h_flex().gap_4().items_start().child(self.render_freeze(cx)).child(self.render_readiness(cx)))
    }

    /// The buttons at the right of the page header.
    fn page_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let online = matches!(self.link, Link::Online { .. });
        match self.page {
            Page::Overview => Button::new("scan-all")
                .primary()
                .icon(IconName::RefreshCw)
                .label("Scan all modules")
                .disabled(!online)
                .on_click(cx.listener(|this, _, _, cx| {
                    let _ = this.backend.cmds.send(Cmd::ReadDtcs);
                    let _ = this.backend.cmds.send(Cmd::ScanModules);
                    cx.notify();
                }))
                .into_any_element(),
            Page::Codes if self.confirm_clear => h_flex()
                .gap_2()
                .items_center()
                .child(div().text_xs().text_color(cx.theme().red).child("Erases codes, freeze-frame data and readiness monitors."))
                .child(Button::new("cancel").label("Cancel").on_click(cx.listener(|this, _, _, cx| {
                    this.confirm_clear = false;
                    cx.notify();
                })))
                .child(Button::new("confirm").danger().label("Clear codes").on_click(cx.listener(|this, _, _, cx| {
                    this.confirm_clear = false;
                    this.notice = None;
                    let _ = this.backend.cmds.send(Cmd::ClearDtcs);
                    cx.notify();
                })))
                .into_any_element(),
            Page::Codes => h_flex()
                .gap_2()
                .child(Button::new("rescan").icon(IconName::RefreshCw).label("Read codes").on_click(cx.listener(|this, _, _, _| {
                    let _ = this.backend.cmds.send(Cmd::ReadDtcs);
                })))
                .child(
                    Button::new("clear")
                        .label("Clear codes…")
                        .disabled(self.code_count(cx) == 0 || !online)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_clear = true;
                            cx.notify();
                        })),
                )
                .into_any_element(),
            Page::Modules => Button::new("scan-modules")
                .primary()
                .icon(IconName::RefreshCw)
                .label("Scan modules")
                .disabled(!online)
                .on_click(cx.listener(|this, _, _, cx| {
                    let _ = this.backend.cmds.send(Cmd::ScanModules);
                    cx.notify();
                }))
                .into_any_element(),
            Page::Contribute => Button::new("export")
                .label(format!("Export {} contribution(s)", self.contrib.len()))
                .on_click(cx.listener(|this, _, _, cx| this.export_contributions(cx)))
                .into_any_element(),
            Page::Vehicles => Button::new("read-vin")
                .icon(IconName::RefreshCw)
                .label("Read VIN from vehicle")
                .disabled(!online)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.notice = Some("Reading the VIN…".into());
                    let _ = this.backend.cmds.send(Cmd::ReadVin);
                    cx.notify();
                }))
                .into_any_element(),
            Page::Live | Page::Readiness => div().into_any_element(),
        }
    }

    /// Title and one line of context under it.
    fn page_subtitle(&self, cx: &App) -> String {
        match self.page {
            Page::Overview => match &self.link {
                Link::Online { .. } => format!("{} module(s) scanned", self.modules.len()),
                Link::Connecting => "Connecting…".into(),
                Link::Offline(reason) => reason.clone(),
            },
            Page::Codes => match self.code_count(cx) {
                0 => "No stored or pending trouble codes".into(),
                n => format!("{n} trouble code(s)"),
            },
            Page::Live => format!("{} parameter(s)", self.channels.len()),
            Page::Readiness => match &self.readiness {
                Some(r) => format!("{} of {} monitors ready", r.monitors.iter().filter(|m| m.supported && m.complete).count(), r.monitors.iter().filter(|m| m.supported).count()),
                None => "Not read yet".into(),
            },
            Page::Modules => self.notice.clone().unwrap_or_else(|| {
                if self.modules.is_empty() { "Scan the vehicle's diagnostic network for modules (read-only).".into() } else { format!("{} module(s)", self.modules.len()) }
            }),
            Page::Vehicles => format!("{} saved · the VIN is read from the car when possible", self.vehicles.len()),
            Page::Contribute => "Name modules and describe codes the shared database does not know yet.".into(),
        }
    }

    fn stat(&self, label: &'static str, value: String, unit: &'static str, note: String, note_color: Hsla, last: bool, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .gap_1()
            .p_4()
            .when(!last, |this| this.border_r_1().border_color(theme.border))
            .child(div().text_xs().text_color(theme.muted_foreground).child(label))
            .child(
                h_flex()
                    .gap_1()
                    .items_baseline()
                    .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(value))
                    .child(div().text_sm().text_color(theme.muted_foreground).child(unit)),
            )
            .child(div().text_xs().text_color(note_color).child(note))
    }

    fn render_overview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let codes = self.codes.read(cx).delegate().codes.iter().map(|e| (e.code.to_string(), e.kind, e.ecu)).collect::<Vec<_>>();
        let pending = codes.iter().filter(|c| c.1 == "pending").count();
        let reading = |key: &str| self.readings.get(key).copied();
        let (ready, supported) = self.readiness.as_ref().map_or((0, 0), |r| {
            (r.monitors.iter().filter(|m| m.supported && m.complete).count(), r.monitors.iter().filter(|m| m.supported).count())
        });
        let make = self.make.clone();
        let db = dtcdb::get();

        let stats = h_flex()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .overflow_hidden()
            .child(self.stat(
                "Trouble codes",
                codes.len().to_string(),
                "",
                if codes.is_empty() { "None stored".into() } else { format!("{} stored, {pending} pending", codes.len() - pending) },
                if codes.is_empty() { theme.green } else { theme.red },
                false,
                cx,
            ))
            .child(self.stat(
                "Battery voltage",
                reading("pid:42").map_or("—".into(), |v| format!("{v:.1}")),
                "V",
                reading("pid:42").map_or("Not reported".into(), |v| if v < 12.0 { "Low".into() } else { "Normal".into() }),
                reading("pid:42").map_or(theme.muted_foreground, |v| if v < 12.0 { theme.yellow } else { theme.green }),
                false,
                cx,
            ))
            .child(self.stat(
                "Coolant temperature",
                reading("pid:05").map_or("—".into(), |v| format!("{v:.0}")),
                "°C",
                reading("pid:05").map_or("Not reported".into(), |v| if v < 70.0 { "Warming up".into() } else { "At operating temperature".into() }),
                reading("pid:05").map_or(theme.muted_foreground, |v| if v < 70.0 { theme.yellow } else { theme.green }),
                false,
                cx,
            ))
            .child(self.stat(
                "Readiness monitors",
                if supported == 0 { "—".into() } else { ready.to_string() },
                if supported == 0 { "" } else { "ready" },
                if supported == 0 { "Not read yet".into() } else if ready == supported { "Ready for inspection".into() } else { format!("{} of {supported} not ready", supported - ready) },
                if supported == 0 { theme.muted_foreground } else if ready == supported { theme.green } else { theme.yellow },
                true,
                cx,
            ));

        let fault_rows = codes.iter().take(5).map(|(code, kind, ecu)| {
            let info = db.lookup(code, make.as_deref());
            let description = match (&info.scope, self.own_description(code)) {
                (dtcdb::Scope::Unknown, Some(own)) => own.to_string(),
                _ => info.description,
            };
            let color = if *kind == "stored" { theme.red } else { theme.yellow };
            h_flex()
                .gap_3()
                .px_3()
                .h_10()
                .items_center()
                .border_b_1()
                .border_color(theme.border.opacity(0.7))
                .child(div().w(px(70.)).font_weight(FontWeight::SEMIBOLD).child(code.clone()))
                .child(div().flex_1().truncate().child(description))
                .child(div().w(px(60.)).text_xs().text_color(theme.muted_foreground).child(format!("{ecu:03X}")))
                .child(div().w(px(70.)).text_xs().text_color(color).child(*kind))
        });

        let gauges = h_flex().flex_wrap().gap_3().children(self.channels.iter().take(6).map(|c| self.gauge(c, cx)));

        v_flex()
            .id("overview")
            .p_6()
            .gap_6()
            .size_full()
            .overflow_y_scroll()
            .child(stats)
            .child(
                v_flex()
                    .gap_2()
                    .child(h_flex().justify_between().items_center().child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Fault codes")).child(
                        Button::new("all-codes").ghost().small().label("View all").on_click(cx.listener(|this, _, _, cx| {
                            this.page = Page::Codes;
                            cx.notify();
                        })),
                    ))
                    .child(
                        v_flex()
                            .border_1()
                            .border_color(theme.border)
                            .rounded(theme.radius_lg)
                            .overflow_hidden()
                            .children(fault_rows)
                            .when(codes.is_empty(), |this| {
                                this.child(div().p_3().text_color(theme.muted_foreground).child("No stored or pending trouble codes."))
                            }),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(h_flex().justify_between().items_center().child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Live data")).child(
                        Button::new("open-live").ghost().small().label("Open").on_click(cx.listener(|this, _, _, cx| {
                            this.page = Page::Live;
                            cx.notify();
                        })),
                    ))
                    .child(if self.channels.is_empty() {
                        div().text_color(theme.muted_foreground).child("Waiting for the ECU…").into_any_element()
                    } else {
                        gauges.into_any_element()
                    }),
            )
    }

    fn render_vehicles(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rows = self.vehicles.iter().enumerate().map(|(ix, v)| {
            let on = self.current == Some(ix);
            h_flex()
                .gap_3()
                .px_3()
                .h_12()
                .items_center()
                .border_b_1()
                .border_color(theme.border.opacity(0.7))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(v.name.clone()))
                        .child(div().text_xs().text_color(theme.muted_foreground).child(format!(
                            "{}  ·  {}",
                            v.make,
                            v.vin.as_deref().unwrap_or("no VIN")
                        ))),
                )
                .child(if on {
                    div().text_xs().text_color(theme.green).child("In use").into_any_element()
                } else {
                    Button::new(SharedString::from(format!("use-{ix}"))).small().label("Use").on_click(cx.listener(
                        move |this, _, window, cx| this.select_vehicle(ix, window, cx),
                    )).into_any_element()
                })
                .child(
                    Button::new(SharedString::from(format!("remove-{ix}"))).ghost().small().icon(IconName::Delete).on_click(cx.listener(
                        move |this, _, window, cx| this.remove_vehicle(ix, window, cx),
                    )),
                )
        });
        v_flex()
            .id("vehicles")
            .p_6()
            .gap_6()
            .size_full()
            .overflow_y_scroll()
            .children(self.notice.clone().map(|n| div().text_sm().text_color(theme.muted_foreground).child(n)))
            .child(
                v_flex()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius_lg)
                    .overflow_hidden()
                    .children(rows)
                    .when(self.vehicles.is_empty(), |this| {
                        this.child(div().p_3().text_color(theme.muted_foreground).child(
                            "No vehicles yet. Plug the adapter into a car and it is added from its VIN, or add one below.",
                        ))
                    }),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("Add a vehicle"))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().w(px(80.)).text_xs().text_color(theme.muted_foreground).child("Name"))
                            .child(div().flex_1().child(Input::new(&self.name_input))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().w(px(80.)).text_xs().text_color(theme.muted_foreground).child("Make"))
                            .child(div().w(px(240.)).child(Select::new(&self.make_select).placeholder("Select make…").menu_width(px(240.)))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().w(px(80.)).text_xs().text_color(theme.muted_foreground).child("VIN"))
                            .child(div().flex_1().child(Input::new(&self.vin_input))),
                    )
                    .child(h_flex().pt_1().child(Button::new("add-vehicle").primary().label("Add vehicle").on_click(cx.listener(
                        |this, _, window, cx| this.add_vehicle(window, cx),
                    )))),
            )
    }

    fn render_readiness_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .p_6()
            .gap_4()
            .size_full()
            .child(self.render_readiness(cx))
            .child(self.render_freeze(cx))
    }

    fn render_modules(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let make = self.make.clone();
        v_flex()
            .id("module-list")
            .p_6()
            .gap_3()
            .size_full()
            .overflow_y_scroll()
            .children(self.modules.iter().map(|m| self.module_card(m, make.as_deref(), cx)).collect::<Vec<_>>())
    }

    fn module_card(&self, m: &ModuleView, make: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tx_id = m.tx_id;
        // the module's own system name if it gives one, else vehicle-db's name for this request ID
        let title = m
            .identity
            .iter()
            .find(|(k, _)| k == "System name")
            .map(|(_, v)| v.clone())
            .or_else(|| self.own_module_name(m.tx_id).map(String::from))
            .or_else(|| modnames::get().name(m.tx_id, make).map(String::from))
            .unwrap_or_else(|| "Module".to_string());
        let row = |label: String, value: String, color: Hsla| {
            h_flex()
                .gap_2()
                .text_xs()
                .child(div().w(px(170.)).text_color(theme.muted_foreground).child(label))
                .child(div().text_color(color).child(value))
        };
        let db = dtcdb::get();
        v_flex()
            .gap_1()
            .p_3()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_sm().text_color(theme.foreground).child(format!("{title}  ·  {:03X} → {:03X}", m.tx_id, m.rx_id)))
                    .child(Button::new(SharedString::from(format!("sweep-{tx_id}"))).label("Read data IDs").on_click(cx.listener(
                        move |this, _, _, cx| {
                            let _ = this.backend.cmds.send(Cmd::SweepDids { tx_id });
                            cx.notify();
                        },
                    ))),
            )
            .children(m.identity.iter().filter(|(k, _)| k != "System name").map(|(k, v)| row(k.clone(), v.clone(), theme.foreground)))
            .children(m.dtcs.iter().map(|d| {
                let info = db.lookup(&d.code[..5], make);
                let description = match (&info.scope, self.own_description(&d.code[..5])) {
                    (dtcdb::Scope::Unknown, Some(own)) => own.to_string(),
                    _ => info.description,
                };
                let color = if d.status == "confirmed" { theme.red } else { theme.yellow };
                row(format!("{} ({})", d.code, d.status), description, color)
            }))
            .children(m.dtc_note.iter().map(|n| row("Trouble codes".into(), n.clone(), theme.muted_foreground)))
            .when(m.dtcs.is_empty() && m.dtc_note.is_none(), |this| this.child(row("Trouble codes".into(), "none".into(), theme.green)))
            .child(row(
                "Services".into(),
                m.services.iter().map(|(n, s)| format!("{n}: {s}")).collect::<Vec<_>>().join(" · "),
                theme.muted_foreground,
            ))
            .children(self.dids.get(&tx_id).map(|lines| {
                v_flex()
                    .mt_1()
                    .pt_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(div().text_xs().text_color(theme.muted_foreground).child(format!("Data identifiers F100–F1FF: {} found", lines.len())))
                    .children(lines.iter().map(|l| {
                        let label = if l.name.is_empty() { format!("{:04X}", l.did) } else { format!("{:04X}  {}", l.did, l.name) };
                        row(label, l.value.clone(), theme.foreground)
                    }))
            }))
            .into_any_element()
    }

    fn panel(&self, title: &'static str, cx: &Context<Self>) -> gpui_kit::Div {
        v_flex()
            .flex_1()
            .gap_2()
            .p_3()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .child(div().text_sm().text_color(cx.theme().foreground).child(title))
    }

    fn render_freeze(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let panel = self.panel("Freeze frame", cx);
        let Some(f) = &self.freeze else {
            return panel.child(div().text_xs().text_color(theme.muted_foreground).child("No freeze frame stored."));
        };
        panel
            .child(
                div().text_xs().text_color(theme.muted_foreground).child(format!("Captured when {} was set", f.dtc)),
            )
            .child(h_flex().flex_wrap().gap_x_4().gap_y_1().children(f.values.iter().filter_map(|(p, v)| {
                let info = pid::info(*p)?;
                Some(div().text_xs().child(format!("{}: {v:.0} {}", info.name, info.unit)))
            })))
    }

    fn render_readiness(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let panel = self.panel("Readiness monitors", cx);
        let Some(r) = &self.readiness else {
            return panel.child(div().text_xs().text_color(theme.muted_foreground).child("Not read yet."));
        };
        panel.child(h_flex().flex_wrap().gap_x_4().gap_y_1().children(r.monitors.iter().filter(|m| m.supported).map(
            |m| {
                let (mark, color) = if m.complete { ("✔", theme.green) } else { ("○", theme.yellow) };
                div().text_xs().text_color(color).child(format!("{mark} {}", m.name))
            },
        )))
    }
}

/// Fewer decimals for wide ranges (rpm, km/h), more for narrow ones (volts, bar).
fn format_value(v: f64, span: f64) -> String {
    let decimals = if span >= 50.0 { 0 } else if span >= 5.0 { 1 } else { 2 };
    format!("{v:.decimals$}")
}

impl Render for Diagnostics {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.code_count(cx);
        let ask_make = matches!(self.link, Link::Online { .. }) && self.make.is_none();
        let theme = cx.theme().clone();
        let (status, status_color) = match &self.link {
            Link::Connecting => ("Connecting…".to_string(), theme.muted_foreground),
            Link::Online { adapter, ecu } => {
                let ecu = ecu.map(|e| format!(" · ECU {e:03X}")).unwrap_or_default();
                let vin = self.vin.as_deref().map(|v| format!(" · VIN {v}")).unwrap_or_default();
                let make = self.make.as_deref().map(|m| format!(" · {m}")).unwrap_or_default();
                (format!("{adapter}{ecu}{vin}{make}"), theme.green)
            }
            Link::Offline(_) => ("Offline, retrying…".to_string(), theme.red),
        };

        // the vehicle at the top of the sidebar: make, and the VIN with its serial part hidden
        let vehicle_title = match self.current.and_then(|ix| self.vehicles.get(ix)) {
            Some(v) => v.name.clone(),
            None => self.make.clone().unwrap_or_else(|| "Select a vehicle".into()),
        };
        let vehicle_vin = self.vin.as_deref().map_or("VIN not read".to_string(), |v| {
            let keep = v.chars().count().min(11);
            format!("{}••••••", v.chars().take(keep).collect::<String>())
        });
        let adapter = match &self.link {
            Link::Online { adapter, .. } => (IconName::CircleCheck, theme.green, "Adapter connected".to_string(), adapter.clone()),
            Link::Connecting => (IconName::Loader, theme.muted_foreground, "Connecting…".to_string(), String::new()),
            Link::Offline(_) => (IconName::CircleAlert, theme.red, "Adapter offline".to_string(), "Retrying…".to_string()),
        };

        let nav = Page::ALL.iter().map(|&page| {
            let on = self.page == page;
            h_flex()
                .id(SharedString::from(format!("nav-{}", page.label())))
                .gap_2()
                .px_2()
                .h_8()
                .items_center()
                .rounded(theme.radius)
                .cursor_pointer()
                .text_color(theme.sidebar_foreground)
                .when(on, |this| this.bg(theme.sidebar_accent).font_weight(FontWeight::SEMIBOLD))
                .hover(|this| this.bg(theme.sidebar_accent))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = page;
                    this.ensure_modules();
                    cx.notify();
                }))
                .child(Icon::new(page.icon()).size_4())
                .child(div().child(page.label()))
                .when(page == Page::Codes && count > 0, |this| {
                    this.child(
                        div()
                            .ml_auto()
                            .min_w_5()
                            .h_5()
                            .px_1p5()
                            .rounded_full()
                            .bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .text_xs()
                            .text_center()
                            .child(count.to_string()),
                    )
                })
        });

        let sidebar = v_flex()
            .w(px(232.))
            .flex_none()
            .h_full()
            .gap_4()
            .p_3()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .child(
                h_flex()
                    .id("vehicle-card")
                    .gap_2()
                    .p_2()
                    .items_center()
                    .justify_between()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .when(self.page == Page::Vehicles, |this| this.bg(theme.sidebar_accent))
                    .hover(|this| this.bg(theme.sidebar_accent))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.page = Page::Vehicles;
                        this.ensure_vin();
                        cx.notify();
                    }))
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(vehicle_title))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(vehicle_vin)),
                    )
                    .child(Icon::new(IconName::ChevronsUpDown).size_4().text_color(theme.muted_foreground)),
            )
            .child(v_flex().gap_0p5().children(nav))
            .child(
                h_flex()
                    .mt_auto()
                    .gap_2()
                    .px_2()
                    .items_center()
                    .child(Icon::new(adapter.0).size_4().text_color(adapter.1))
                    .child(
                        v_flex()
                            .child(div().font_weight(FontWeight::MEDIUM).child(adapter.2))
                            .child(div().text_xs().text_color(theme.muted_foreground).child(adapter.3)),
                    ),
            );

        let header = h_flex()
            .justify_between()
            .items_center()
            .gap_3()
            .px_6()
            .pt_5()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(self.page.label()))
                    .child(div().text_color(theme.muted_foreground).child(self.page_subtitle(cx))),
            )
            .child(self.page_actions(cx));

        v_flex()
            .size_full()
            .bg(theme.background)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(div().text_xs().text_color(theme.muted_foreground).child("Diagnostics"))
                        .child(
                            div()
                                .mr_4()
                                .text_xs()
                                .text_color(if self.mil_on { theme.red } else { theme.green })
                                .child(if self.mil_on { "● MIL on" } else { "● MIL off" }),
                        ),
                ),
            )
            .when(ask_make, |this| {
                this.child(
                    h_flex()
                        .gap_3()
                        .px_3()
                        .py_2()
                        .items_center()
                        .bg(theme.yellow.opacity(0.15))
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme.foreground)
                                .child("Vehicle not recognised. Pick or add it so manufacturer-specific codes can be named."),
                        )
                        .child(Button::new("pick-vehicle").label("Choose vehicle").on_click(cx.listener(|this, _, _, cx| {
                            this.page = Page::Vehicles;
                            this.ensure_vin();
                            cx.notify();
                        }))),
                )
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(v_flex().flex_1().min_w_0().h_full().child(header).child(div().flex_1().min_h_0().overflow_hidden().map(
                        |this| match self.page {
                            Page::Overview => this.child(self.render_overview(cx)),
                            Page::Codes => this.child(self.render_codes(cx)),
                            Page::Live => this.child(self.render_live(cx)),
                            Page::Readiness => this.child(self.render_readiness_page(cx)),
                            Page::Modules => this.child(self.render_modules(cx)),
                            Page::Contribute => this.child(self.render_contribute(window, cx)),
                            Page::Vehicles => this.child(self.render_vehicles(cx)),
                        },
                    ))),
            )
            .child(
                h_flex()
                    .h_7()
                    .px_3()
                    .items_center()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_xs()
                    .text_color(status_color)
                    .justify_between()
                    .child(status),
            )
    }
}

const USAGE: &str = "\
usage: canforge [options]
  (no source)           talk to the vehicle through an OpenPort 2.0
  --simulate            fake car, no hardware needed
  --replay FILE         play back a recording instead: a CSV data log (FORScan, DDT4ALL, …)
                        or a CAN capture (candump -L, Vector .asc, `candriver monitor`)
  --ddt FILE.json       DDT4ALL ECU definition used to decode CAN captures (repeatable)
  --speed X             replay speed multiplier (default 1)
  --make NAME           manufacturer for P1xxx-style trouble codes (default: from the VIN)
  --bitrate N           CAN bitrate for the vehicle (default 500000)
  bench [--count N]     time routine sweeps on the connected car (close the app first)
  db status             which trouble code data is in use
  db update [--from X]  fetch the latest vehicle-db release (checksum-verified)";

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("canforge: {message}");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("db") {
        if let Err(e) = dbcmd::run(&args[1..]) {
            fail(e);
        }
        return;
    }
    if args.first().map(String::as_str) == Some("bench") {
        let flag = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone());
        let bitrate = flag("--bitrate").map_or(500_000, |v| v.parse().unwrap_or_else(|_| fail("--bitrate needs a number")));
        let count = flag("--count").map_or(1024, |v| v.parse().unwrap_or_else(|_| fail("--count needs a number up to 60000")));
        match backend::bench_routines(bitrate, count) {
            Ok(report) => println!("{report}"),
            Err(e) => fail(e),
        }
        return;
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    let flag = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone());
    let make = flag("--make");
    let bitrate = flag("--bitrate").map_or(500_000, |v| v.parse().unwrap_or_else(|_| fail("--bitrate needs a number")));
    let speed: f64 = flag("--speed").map_or(1.0, |v| v.parse().ok().filter(|s| *s > 0.0).unwrap_or_else(|| fail("--speed needs a positive number")));

    let source = if let Some(path) = flag("--replay") {
        let ecus: Vec<import::ddt::Ecu> = args
            .windows(2)
            .filter(|w| w[0] == "--ddt")
            .map(|w| {
                let text = std::fs::read_to_string(&w[1]).unwrap_or_else(|e| fail(format!("{}: {e}", w[1])));
                import::ddt::Ecu::from_json(&text).unwrap_or_else(|e| fail(format!("{}: {e}", w[1])))
            })
            .collect();
        let session = import::load(std::path::Path::new(&path), &ecus).unwrap_or_else(|e| fail(e));
        eprintln!(
            "canforge: {} channels, {} readings, {:.1} s",
            session.channels.len(),
            session.readings.len(),
            session.duration()
        );
        backend::Source::Replay { session, speed }
    } else if args.iter().any(|a| a == "--simulate") {
        backend::Source::Simulator
    } else {
        backend::Source::Vehicle { bitrate }
    };
    let source = std::cell::Cell::new(Some(source));

    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1180.), px(760.)), cx)),
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            window.set_window_title("canforge — Diagnostics");
            Theme::change(ThemeMode::Dark, Some(window), cx);
            let backend = backend::spawn(source.take().expect("window opened once"));
            cx.new(|cx| Diagnostics::new(backend, make.clone(), window, cx))
        })
        .expect("failed to open window");
    });
}
