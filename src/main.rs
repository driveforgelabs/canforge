mod backend;
mod monitors;
mod trace;
mod backup;
mod checks;
mod dashboard;
mod contribute;
mod dbcmd;
mod dump;
mod dtcdb;
mod functions;
mod garage;
mod health;
mod history;
mod i18n;
mod import;
mod modnames;
mod params;
mod pdf;
mod procedures;
mod report;
mod session;
mod sim;
mod vin;
mod codes;
mod fnpage;
mod histpage;
mod live;
mod modpage;
mod notes;
mod procpage;
mod readiness;
mod overview;
mod shell;
mod tracepage;
mod units;
mod verify;
mod ui;
mod workshop;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use workshop::{OpenPalette, Workshop};
use backend::{Backend, Cmd, DidLine, DtcEntry, Event, FreezeFrame, ModuleView, RoutineScan};
use session::Channel;
use candriver_obd::readiness::Readiness;
use candriver_obd::pid;
use gpui_kit::component::{
    ActiveTheme, Icon, IconName, Sizable, Theme, ThemeMode, TitleBar,
    button::{Button, ButtonVariants as _},
    Size,
    chart::{AreaChart, LineChart},
    h_flex,
    slider::{Slider, SliderEvent, SliderState},
    input::{Input, InputEvent, InputState},
    select::{Select, SelectEvent, SelectState},
    table::{Column, TableDelegate, TableState},
    v_flex,
};
use gpui_kit::base::Disableable as _;
use gpui_kit::{prelude::FluentBuilder as _, *};

/// How often the UI looks for new data from the worker.
const PUMP: Duration = Duration::from_millis(50);
/// Chart history resolution.
const TICK: Duration = Duration::from_millis(250);
/// Samples kept per charted signal (5 minutes).
const SERIES_MAX: usize = 1200;

/// A dashboard widget being dragged (its index), and the label that follows the pointer.
struct DragWidget(usize);

struct DragPreview {
    label: String,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div().px_3().py_2().rounded(theme.radius).bg(theme.primary).text_color(theme.primary_foreground).child(self.label.clone())
    }
}

#[derive(Clone)]
struct Pt {
    t: String,
    v: f64,
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
    Functions,
    Contribute,
    Vehicles,
    Procedures,
    History,
    Trace,
}

/// Development only (`CANFORGE_DEV=1`, set by scripts/dev.sh): remember the open page across the
/// restarts hot reload does, so a rebuild lands back where the edit is being looked at.
fn dev_page_file() -> Option<std::path::PathBuf> {
    std::env::var_os("CANFORGE_DEV").and_then(|_| dtcdb::data_dir()).map(|d| d.join("dev-page"))
}

impl Page {
    const ALL: [Page; 11] = [
        Page::Overview,
        Page::Codes,
        Page::Live,
        Page::Readiness,
        Page::Modules,
        Page::Functions,
        Page::Contribute,
        Page::Vehicles,
        Page::Procedures,
        Page::History,
        Page::Trace,
    ];

    fn dev_restore() -> Page {
        dev_page_file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|t| t.trim().parse::<usize>().ok())
            .and_then(|i| Page::ALL.get(i).copied())
            .unwrap_or(Page::Overview)
    }

    fn dev_save(self) {
        static LAST: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);
        let Some(file) = dev_page_file() else { return };
        let ix = Page::ALL.iter().position(|p| *p == self).unwrap_or(0);
        if LAST.swap(ix, std::sync::atomic::Ordering::Relaxed) != ix {
            let _ = std::fs::write(file, ix.to_string());
        }
    }

    /// Sidebar sections: everyday diagnosis first, the rarely needed pages last.
    fn groups() -> Vec<(&'static str, Vec<Page>)> {
        let mut advanced = Vec::new();
        if !functions::get().is_empty() {
            advanced.push(Page::Functions);
        }
        advanced.push(Page::Procedures);
        advanced.push(Page::Trace);
        advanced.push(Page::Contribute);
        vec![
            ("Diagnose", vec![Page::Overview, Page::Codes, Page::Live, Page::Readiness]),
            ("Vehicle", vec![Page::Modules, Page::History]),
            ("Advanced", advanced),
        ]
    }

    fn label(self) -> &'static str {
        i18n::tr(match self {
            Page::Overview => "Overview",
            Page::Codes => "Fault codes",
            Page::Live => "Live data",
            Page::Readiness => "Readiness",
            Page::Modules => "Modules",
            Page::Functions => "Functions",
            Page::Contribute => "Help improve",
            Page::Vehicles => "Vehicles",
            Page::Procedures => "Procedures",
            Page::History => "History",
            Page::Trace => "Trace",
        })
    }

    fn icon(self) -> IconName {
        match self {
            Page::Overview => IconName::LayoutDashboard,
            Page::Codes => IconName::TriangleAlert,
            Page::Live => IconName::ChartPie,
            Page::Readiness => IconName::CircleCheck,
            Page::Modules => IconName::Cpu,
            Page::Functions => IconName::Frame,
            Page::Contribute => IconName::BookOpen,
            Page::Vehicles => IconName::Frame,
            Page::Procedures => IconName::BookOpen,
            Page::History => IconName::Folder,
            Page::Trace => IconName::Search,
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
    readings: BTreeMap<String, f64>,
    /// What is on the Live data dashboard, and the recent values of each charted or carded signal.
    widgets: Vec<dashboard::Widget>,
    series: BTreeMap<String, VecDeque<f64>>,
    /// The dashboard is being edited (each widget shows its settings).
    editing: bool,
    /// The add-to-dashboard picker is open.
    picker: bool,
    /// The user changed the dashboard, so it is not the automatic default any more.
    dashboard_custom: bool,
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
    /// Module data page: the module shown, the filter applied, and what each Read returned.
    /// The picker lists this module's values; `None` is the engine's own live data.
    picker_tab: Option<u32>,
    /// Functions page: the module shown and the groups opened.
    fn_module: Option<u32>,
    open_groups: BTreeSet<String>,
    last_poll: std::time::Instant,
    param_values: BTreeMap<(u32, u16), Result<Vec<u8>, String>>,
    /// Identifiers each module answered to during a probe, and the probe in progress (module, done, total).
    param_supported: BTreeMap<u32, BTreeSet<u16>>,
    picker_supported_only: bool,
    probe: Option<(u32, usize, usize)>,
    /// Functions page: the run awaiting confirmation (module, index in its list) and what each run returned.
    confirm_run: Option<(u32, usize)>,
    run_results: BTreeMap<(u32, u16), (Result<Vec<u8>, String>, String)>,
    codes: Entity<TableState<DtcDelegate>>,
    /// A module scan is running (cleared when its result arrives).
    scanning: bool,
    /// A trouble code read is running.
    reading_codes: bool,
    /// How many times the adapter has been reported offline since the last connection.
    offline_attempts: usize,
    /// Live data page: display frozen at the moment Pause was pressed.
    paused: Option<BTreeMap<String, f64>>,
    /// Lowest and highest value seen per channel since connecting (or since Reset).
    extremes: BTreeMap<String, (f64, f64)>,
    /// Rows being recorded: seconds since start and one value per channel (NaN = no reading yet).
    recording: Option<(std::time::Instant, Vec<(f64, Vec<f64>)>)>,
    /// Modules whose full details are open on the Modules page.
    expanded_modules: BTreeSet<u32>,
    /// The long export section on the Help improve page is open.
    show_export: bool,
    /// How many parameters the Module data page lists.
    param_limit: usize,
    ui: ui::UiState,
    ws: Workshop,
    /// Short confirmation of the last copy/save, shown in the status bar.
    toast: Option<String>,
}

impl Diagnostics {
    fn new(backend: Backend, make: Option<String>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (ws, ws_subs) = Workshop::new(window, cx);
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

        let scrub_state = cx.new(|_| SliderState::new().min(0.).max(60.).step(1.).default_value(60.));
        let scrub_sub = cx.subscribe(&scrub_state, |this, _, event: &SliderEvent, cx| {
            if let SliderEvent::Change(v) = event {
                this.ui.scrub = v.start();
                cx.notify();
            }
        });

        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(PUMP).await;
                if this
                    .update_in(cx, |this, window, cx| {
                        // redraw only when something changed
                        if this.pump(window, cx) | PATCHED.swap(false, std::sync::atomic::Ordering::Relaxed) {
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

        let mut this = Self {
            backend,
            link: Link::Connecting,
            page: Page::dev_restore(),
            channels: Vec::new(),
            readings: BTreeMap::new(),
            widgets: dtcdb::data_dir().and_then(|d| dashboard::load(&d)).unwrap_or_default(),
            series: BTreeMap::new(),
            editing: false,
            picker: false,
            dashboard_custom: dtcdb::data_dir().is_some_and(|d| d.join("dashboard.json").exists()),
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
            _subscriptions: [subscription, scrub_sub].into_iter().chain(ws_subs).collect(),
            modules: Vec::new(),
            dids: BTreeMap::new(),
            picker_tab: None,
            fn_module: None,
            open_groups: BTreeSet::new(),
            last_poll: std::time::Instant::now(),
            param_values: BTreeMap::new(),
            param_supported: BTreeMap::new(),
            picker_supported_only: false,
            probe: None,
            confirm_run: None,
            run_results: BTreeMap::new(),
            codes,
            scanning: false,
            reading_codes: false,
            offline_attempts: 0,
            paused: None,
            extremes: BTreeMap::new(),
            recording: None,
            expanded_modules: BTreeSet::new(),
            show_export: false,
            param_limit: 60,
            ui: ui::UiState::new(scrub_state),
            ws,
            toast: None,
        };
        this.dev_open();
        this
    }

    /// `CANFORGE_OPEN=settings|palette|more|report|run|confirm|bench|detail` opens that overlay at start-up, to look at it.
    fn dev_open(&mut self) {
        match std::env::var("CANFORGE_OPEN").as_deref() {
            Ok("settings") => self.ui.settings = true,
            Ok("palette") => self.ws.palette_open = true,
            Ok("more") => self.ui.more = true,
            Ok("report") => self.ui.report_menu = true,
            Ok("run") => self.confirm_run = functions::get().for_module(0x7e0).iter().position(|f| f.runnable && f.routine_id().is_some()).map(|i| (0x7e0, i)),
            Ok("confirm") => self.confirm_clear = true,
            Ok("bench") => self.ui.bench = true,
            Ok("detail") => self.ui.detail = true,
            Ok("fnopen") => {
                self.fn_module = Some(0x7e0);
                for f in functions::get().for_module(0x7e0) {
                    self.open_groups.insert(format!("{:03X}:{}", 0x7e0, f.category()));
                }
            }
            Ok("verify") => {
                self.ws.clear_result = Some("Cleared 3 of 3 code(s) in the engine ECU. None came back yet. Readiness monitors were reset: drive the car and read the codes again to confirm the repair.".into());
                self.ws.verify = Some(verify::Verify { at: workshop::unix_now(), codes_before: vec!["P0113".into(), "P0101".into(), "P0300".into()], ready_before: (1, 6), cleared: true, last_read: std::time::Instant::now(), announced: true, dismissed: false });
            }
            Ok("procbar") => {
                self.ws.proc = Some(workshop::ProcRun { index: 1, step: 2, waiting: None, skipped: BTreeSet::new() });
                self.ui.proc_open = false;
            }
            _ => {}
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
                    self.offline_attempts += 1;
                    self.scanning = false;
                    self.reading_codes = false;
                    self.readings.clear();
                }
                Event::Connected { adapter, ecu, bitrate, channels } => {
                    self.ws.bitrate = bitrate;
                    self.link = Link::Online { adapter, ecu };
                    self.offline_attempts = 0;
                    self.paused = None;
                    self.extremes.clear();
                    // a saved dashboard that fits nothing this source provides is replaced by the default
                    let fits = self.widgets.iter().any(|w| match &w.sig {
                        dashboard::Sig::Obd(k) => channels.iter().any(|c| &c.key == k),
                        dashboard::Sig::Param { .. } => true,
                    });
                    if !fits {
                        let keys: Vec<String> = channels.iter().map(|c| c.key.clone()).collect();
                        self.widgets = dashboard::defaults(&keys);
                    }
                    self.channels = channels;
                    self.scan_requested = false;
                    self.vin_requested = false;
                    self.ensure_modules();
                    self.ensure_vin();
                    self.readings.clear();
                    self.series.clear();
                    self.ticks = 0;
                }
                Event::Reading { key, value } => {
                    let e = self.extremes.entry(key.clone()).or_insert((value, value));
                    e.0 = e.0.min(value);
                    e.1 = e.1.max(value);
                    self.readings.insert(key, value);
                }
                Event::Notice(text) => {
                    // a replay refuses reads and scans with a notice and never answers them
                    if text.contains("replay") || text.contains("need a vehicle") {
                        self.scanning = false;
                        self.reading_codes = false;
                    }
                    self.notice = Some(text);
                }
                Event::Modules(modules) => {
                    self.scanning = false;
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
                Event::DidValue { tx_id, did, result } => {
                    self.param_values.insert((tx_id, did), result);
                }
                Event::ProbeProgress { tx_id, done, total } => self.probe = Some((tx_id, done, total)),
                Event::Probed { tx_id, found, note } => {
                    self.probe = None;
                    let mut dids = BTreeSet::new();
                    for (did, result) in found {
                        dids.insert(did);
                        self.param_values.insert((tx_id, did), result);
                    }
                    self.notice = Some(format!("Module {tx_id:03X} answered {} identifier(s){}", dids.len(), if note.is_empty() { String::new() } else { format!(" ({note})") }));
                    self.param_supported.insert(tx_id, dids);
                }
                Event::RoutineRun { tx_id, routine, result, after } => {
                    self.notice = Some(match &result {
                        Ok(_) => format!("Module {tx_id:03X}: routine {routine:04X} started."),
                        Err(why) => format!("Module {tx_id:03X}: routine {routine:04X} {why}"),
                    });
                    self.on_routine_run(tx_id, routine, result.is_ok(), cx);
                    self.run_results.insert((tx_id, routine), (result, after));
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
                Event::Backup { tx_id, routine, values } => self.on_backup(tx_id, routine, values),
                Event::Restored { tx_id, results } => self.on_restored(tx_id, results, cx),
                Event::Monitors(tests) => {
                    self.toast = Some(format!("{} self-test result(s) read.", tests.len()));
                    self.ws.monitors = Some(tests);
                }
                Event::Readiness(r) => self.readiness = Some(r),
                Event::Freeze(f) => self.freeze = f,
                Event::Dtcs { mil_on, codes } => {
                    self.mil_on = mil_on;
                    self.reading_codes = false;
                    self.codes.update(cx, |table, cx| {
                        table.delegate_mut().codes = codes;
                        cx.notify();
                    });
                    self.finish_clear(cx);
                }
            }
        }
        changed |= self.sync_history(cx);
        changed |= self.verify_tick(cx);
        if self.page == Page::Trace && self.ws.trace_frozen.is_none() && trace::latest() != self.ws.trace_seen {
            self.ws.trace_seen = trace::latest();
            changed = true;
        }
        // module values on the dashboard are asked for again every second while it is showing
        if self.page == Page::Live && self.paused.is_none() && matches!(self.link, Link::Online { .. }) && self.last_poll.elapsed() >= Duration::from_secs(1) {
            self.last_poll = std::time::Instant::now();
            let wanted: BTreeSet<(u32, u16)> = self
                .widgets
                .iter()
                .filter_map(|w| match w.sig {
                    dashboard::Sig::Param { tx, did, .. } => Some((tx, did)),
                    _ => None,
                })
                .collect();
            for (tx_id, did) in wanted {
                let _ = self.backend.cmds.send(Cmd::ReadDid { tx_id, did });
            }
        }
        if matches!(self.link, Link::Online { .. }) && self.last_sample.elapsed() >= TICK {
            self.last_sample = std::time::Instant::now();
            changed = true;
            if self.paused.is_none() {
                for w in self.widgets.clone() {
                    if let Ok(v) = self.sig_value(&w.sig, &self.readings) {
                        let q = self.series.entry(w.sig.id()).or_default();
                        if q.len() >= SERIES_MAX {
                            q.pop_front();
                        }
                        q.push_back(v);
                    }
                }
            }
            self.ticks += 1;
            if let Some((start, rows)) = &mut self.recording {
                if rows.len() < 200_000 {
                    let values = self.channels.iter().map(|c| self.readings.get(&c.key).copied().unwrap_or(f64::NAN)).collect();
                    rows.push((start.elapsed().as_secs_f64(), values));
                }
            }
        }
        changed
    }

    /// Starts a module scan (read-only) and shows that it is running.
    fn start_scan(&mut self) {
        if matches!(self.link, Link::Online { .. }) && !self.scanning {
            self.scanning = true;
            self.scan_requested = true;
            let _ = self.backend.cmds.send(Cmd::ScanModules);
        }
    }

    fn start_read_codes(&mut self) {
        if matches!(self.link, Link::Online { .. }) && !self.reading_codes {
            self.reading_codes = true;
            let _ = self.backend.cmds.send(Cmd::ReadDtcs);
        }
    }

    fn rpm(&self) -> Option<f64> {
        self.readings.get("pid:0c").copied()
    }

    /// Whether the engine is known to be running (`None` when engine speed is not available).
    fn engine_running(&self) -> Option<bool> {
        health::engine_running(self.rpm())
    }

    fn module_title(&self, tx_id: u32) -> String {
        self.modules
            .iter()
            .find(|m| m.tx_id == tx_id)
            .and_then(|m| m.identity.iter().find(|(k, _)| k == "System name").map(|(_, v)| v.clone()))
            .or_else(|| self.own_module_name(tx_id).map(String::from))
            .or_else(|| modnames::get().name(tx_id, self.make.as_deref()).map(String::from))
            .unwrap_or_else(|| "Module".to_string())
    }

    /// Every fault the car reported: the engine ECU's OBD list plus what each scanned module holds,
    /// without listing the same code twice for the same module.
    fn faults(&self, cx: &App) -> Vec<report::Fault> {
        let db = dtcdb::get();
        let make = self.make.as_deref();
        let describe = |code: &str| {
            let info = db.lookup(code, make);
            match (&info.scope, self.own_description(code)) {
                (dtcdb::Scope::Unknown, Some(own)) => own.to_string(),
                _ => info.description,
            }
        };
        let mut out: Vec<report::Fault> = Vec::new();
        let mut seen: BTreeSet<(String, u32)> = BTreeSet::new();
        for e in self.codes.read(cx).delegate().codes.iter() {
            let code = e.code.to_string();
            let tx = e.ecu.saturating_sub(8);
            seen.insert((code.clone(), tx));
            out.push(report::Fault { description: describe(&code), module: format!("{} · {tx:03X}", self.module_title(tx)), status: e.kind.to_string(), code });
        }
        for m in &self.modules {
            for d in &m.dtcs {
                let code = d.code.get(..5).unwrap_or(&d.code).to_string();
                if !seen.insert((code.clone(), m.tx_id)) {
                    continue;
                }
                out.push(report::Fault {
                    description: describe(&code),
                    module: format!("{} · {:03X}", self.module_title(m.tx_id), m.tx_id),
                    status: d.status.to_string(),
                    code,
                });
            }
        }
        // what is urgent first, and the likely root cause before what it set off
        let weak = self.battery_weak();
        out.sort_by_key(|f| checks::order_key(&f.code, f.status == "pending", weak));
        out
    }

    /// The battery reads bad (flat, or not charging with the engine running).
    pub fn battery_weak(&self) -> bool {
        self.readings.get("pid:42").is_some_and(|v| matches!(health::battery(*v, self.rpm()).1, health::Level::Bad))
    }

    fn battery_text(&self) -> Option<String> {
        let v = self.readings.get("pid:42").copied()?;
        Some(format!("{v:.1} V, {}", health::battery(v, self.rpm()).0))
    }

    fn build_report(&self, cx: &App) -> report::Report {
        let unix_time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let vehicle = self
            .current
            .and_then(|ix| self.vehicles.get(ix))
            .map(|v| v.name.clone())
            .or_else(|| self.make.clone())
            .unwrap_or_else(|| "Vehicle".into());
        let adapter = match &self.link {
            Link::Online { adapter, .. } => adapter.clone(),
            _ => String::new(),
        };
        report::Report {
            unix_time,
            vehicle,
            vin: self.vin.clone(),
            adapter,
            mil_on: self.mil_on,
            battery: self.battery_text(),
            faults: self.faults(cx),
            modules_scanned: !self.modules.is_empty(),
            readiness: self.readiness.as_ref().map(|r| r.monitors.iter().filter(|m| m.supported).map(|m| (m.name.to_string(), m.complete)).collect()).unwrap_or_default(),
            freeze: self
                .freeze
                .as_ref()
                .map(|f| f.values.iter().filter_map(|(p, v)| pid::info(*p).map(|i| { let (n, u) = units::show_free(*v, i.unit); (i.name.to_string(), format!("{n} {u}")) })).collect())
                .unwrap_or_default(),
            freeze_code: self.freeze.as_ref().map(|f| f.dtc.to_string()),
            actions: self.ws.actions.clone(),
            since_last: self.since_last(cx),
            extra: report::Extra {
                shop: self.ws.shop_input.read(cx).value().to_string(),
                note: self.ws.note_input.read(cx).value().to_string(),
                recommendation: self.ws.reco_input.read(cx).value().to_string(),
                estimate: self.ws.est_input.read(cx).value().to_string(),
                checks: self.ws.checks_done.iter().cloned().collect(),
                photos: self.photo_paths(),
                repair: self.ws.verify.as_ref().filter(|v| v.cleared).map(|v| report::Repair {
                    codes_before: v.codes_before.len(),
                    codes_now: self.obd_codes(cx).len(),
                    ready_before: v.ready_before,
                    ready_now: self.monitor_counts(),
                    came_back: v.codes_before.iter().filter(|c| self.obd_codes(cx).contains(c)).cloned().collect(),
                }),
            },
        }
    }

    /// Saves the job report, opens it in the default browser for printing, and says where it went.
    fn save_report(&mut self, cx: &mut Context<Self>) {
        let report = self.build_report(cx);
        self.toast = Some(match dtcdb::data_dir() {
            None => "No data directory available (set CANFORGE_DATA_DIR).".into(),
            Some(dir) => match report::write(&dir, &report) {
                Ok(path) => {
                    let _ = std::process::Command::new("xdg-open").arg(&path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
                    format!("Report saved: {}. Print it from the browser to get a PDF.", path.display())
                }
                Err(e) => e,
            },
        });
        cx.notify();
    }

    fn copy_text(&mut self, text: String, what: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.toast = Some(format!("Copied {what}."));
        cx.notify();
    }

    /// Stops recording and writes the rows as a CSV that `--replay` can play back.
    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        let Some((_, rows)) = self.recording.take() else { return };
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        self.toast = Some(match dtcdb::data_dir() {
            None => "No data directory available (set CANFORGE_DATA_DIR).".into(),
            Some(dir) => {
                let mut csv = String::from("Time [s]");
                for c in &self.channels {
                    let name = c.name.replace([',', '[', ']'], " ");
                    if c.unit.is_empty() { csv.push_str(&format!(",{name}")) } else { csv.push_str(&format!(",{name} [{}]", c.unit.replace(',', " "))) }
                }
                csv.push('\n');
                for (t, values) in &rows {
                    csv.push_str(&format!("{t:.2}"));
                    for v in values {
                        if v.is_nan() { csv.push(',') } else { csv.push_str(&format!(",{v}")) }
                    }
                    csv.push('\n');
                }
                let folder = dir.join("recordings");
                let path = folder.join(format!("live-{unix}.csv"));
                if let Some(from) = self.ws.record_trace_from.take() {
                    let lines = trace::since(from);
                    if !lines.is_empty() {
                        let _ = std::fs::create_dir_all(&folder).and_then(|_| std::fs::write(folder.join(format!("live-{unix}.trace.txt")), trace::to_text(&lines)));
                    }
                }
                match std::fs::create_dir_all(&folder).and_then(|_| std::fs::write(&path, csv)) {
                    Ok(()) => format!("Recording saved ({} rows): {}. Replay it with --replay.", rows.len(), path.display()),
                    Err(e) => format!("{}: {e}", path.display()),
                }
            }
        });
        cx.notify();
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

    /// Scans the modules once per connection, so faults outside the engine ECU are not missed.
    fn ensure_modules(&mut self) {
        // faults in other modules belong on the Overview and Fault codes pages too
        if !self.scan_requested && self.modules.is_empty() {
            self.start_scan();
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
        } else if routines && self.engine_running() != Some(false) {
            self.contrib_msg = Some(if self.engine_running().is_some() {
                "Stop the engine first: the routine scan switches modules into another diagnostic session.".into()
            } else {
                "Cannot tell whether the engine is stopped (no engine speed): not starting the routine scan.".into()
            });
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
        if let Some(existing) = self.inputs.get(&key) {
            return existing.clone();
        }
        let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        if matches!(key.as_str(), "params-search" | "fn-search" | "pick-search") {
            // filter as the user types
            let sub = cx.subscribe_in(&state, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.param_limit = 60;
                    cx.notify();
                }
            });
            self._subscriptions.push(sub);
        }
        self.inputs.insert(key, state.clone());
        state
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

        let show_export = self.show_export;
        v_flex()
            .id("contribute")
            .p_6()
            .gap_3()
            .size_full()
            .overflow_y_scroll()
            .child(
                div().text_sm().text_color(theme.muted_foreground).child(
                    "Name modules and describe codes the shared database does not know yet. Only enter what you found out on \
                     your own vehicle (the label on the part, what stops working). Never copy from other diagnostic software. \
                     Entries work immediately and stay on this computer until you export them.",
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_sm().w(px(120.)).text_color(theme.muted_foreground).child("This vehicle"))
                    .child(div().flex_1().child(Input::new(&self.note_input))),
            )
            .child(div().text_sm().text_color(theme.muted_foreground).child(match &make {
                Some(m) => format!("Make: {m}. Describe the model and engine above, not the VIN."),
                None => "Choose the vehicle's make first (the yellow banner or the vehicle card at the top left).".to_string(),
            }))
            .children(rows.is_empty().then(|| {
                div().text_base().text_color(theme.muted_foreground).child(if self.modules.is_empty() {
                    "Scanning the vehicle for modules…"
                } else {
                    "Nothing is missing for the modules and codes found so far."
                })
            }))
            .children(rows)
            .child(
                v_flex()
                    .gap_2()
                    .pt_2()
                    .child(
                        h_flex().gap_2().items_center()
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child("Export everything from this vehicle"))
                            .child(Button::new("export-toggle").ghost().small().label(if show_export { "Hide" } else { "Show" }).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.show_export = !this.show_export;
                                    cx.notify();
                                }),
                            )),
                    )
                    .when(show_export, |this| {
                        this.child(div().text_sm().text_color(theme.muted_foreground).child(
                            "Saves one JSON file with the vehicle, trouble codes, readiness, freeze frame, modules and the data they \
                             answer. The file contains the VIN: keep it for your own analysis, do not share it.",
                        ))
                        .child(div().text_sm().text_color(theme.yellow).child(
                            "The routine scan asks every module about every routine ID without starting any, but it switches each \
                             module into its extended diagnostic session (and back), takes many minutes and pauses live data. \
                             Engine must be stopped.",
                        ))
                    })
                    .child(h_flex().gap_2().child(
                        Button::new("dump").icon(IconName::Folder).label("Export vehicle data").disabled(self.dump_pending).on_click(
                            cx.listener(|this, _, _, cx| this.start_dump(false, cx)),
                        ),
                    ).when(show_export, |this| {
                        this.child(
                            Button::new("dump-routines").label(if resumable { "Resume routine scan and export" } else { "Export with routine scan (slow)" }).disabled(self.dump_pending).on_click(
                                cx.listener(|this, _, _, cx| this.start_dump(true, cx)),
                            ),
                        )
                    }).when(resumable && !self.dump_pending && show_export, |this| {
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
            .children(self.contrib_msg.clone().map(|m| div().text_sm().text_color(theme.foreground).child(m)))
    }

    fn contribute_row(&self, label: String, input: Entity<InputState>, kind: contribute::Kind, id: String, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .gap_2()
            .items_center()
            .child(div().w(px(260.)).text_sm().text_color(theme.foreground).child(label))
            .child(div().flex_1().child(Input::new(&input)))
            .child(Button::new(SharedString::from(format!("save-{id}"))).label("Save").with_size(btn()).on_click(cx.listener(
                move |this, _, window, cx| this.save_contribution(kind, id.clone(), window, cx),
            )))
            .into_any_element()
    }

    fn code_count(&self, cx: &App) -> usize {
        self.codes.read(cx).delegate().codes.len()
    }






    /// Name, unit, range and where a dashboard signal comes from.
    fn sig_info(&self, sig: &dashboard::Sig) -> Channel {
        match sig {
            dashboard::Sig::Obd(key) => match self.channels.iter().find(|c| &c.key == key) {
                Some(c) => c.clone(),
                None => Channel::new(key.clone(), key.clone(), ""),
            },
            dashboard::Sig::Param { tx, did, bit } => {
                let p = params::get().for_module(*tx).iter().find(|p| p.did == *did && p.bit == *bit);
                let mut c = Channel::new(sig.id(), p.map_or(format!("{did:04X}"), |p| p.name.clone()), p.map_or(String::new(), |p| p.unit.clone()));
                if p.is_some_and(|p| p.is_bit) {
                    c.max = 1.0;
                }
                c
            }
        }
    }

    /// Where the signal comes from, for the card's small print.
    fn sig_source(&self, sig: &dashboard::Sig) -> String {
        match sig {
            dashboard::Sig::Obd(_) => "Engine".into(),
            dashboard::Sig::Param { tx, .. } => format!("{} · {tx:03X}", self.module_title(*tx)),
        }
    }

    /// The current number, or why there is none.
    fn sig_value(&self, sig: &dashboard::Sig, shown: &BTreeMap<String, f64>) -> Result<f64, String> {
        match sig {
            dashboard::Sig::Obd(key) => shown.get(key).copied().ok_or_else(|| "no data".to_string()),
            dashboard::Sig::Param { tx, did, bit } => match self.param_values.get(&(*tx, *did)) {
                None => Err("waiting for the module…".into()),
                Some(Err(why)) => Err(why.clone()),
                Some(Ok(data)) => params::get().for_module(*tx).iter().find(|p| p.did == *did && p.bit == *bit).and_then(|p| p.value(data)).ok_or_else(|| "no value".to_string()),
            },
        }
    }

    fn save_dashboard(&mut self) {
        self.dashboard_custom = true;
        if let Some(dir) = dtcdb::data_dir() {
            if let Err(e) = dashboard::save(&dir, &self.widgets) {
                self.toast = Some(e);
            }
        }
    }

    /// Puts a signal on the dashboard as a card, or takes it off if it is already there.
    fn toggle_signal(&mut self, sig: dashboard::Sig) {
        if self.widgets.iter().any(|w| w.sig == sig) {
            self.widgets.retain(|w| w.sig != sig);
        } else {
            if let dashboard::Sig::Param { tx, did, .. } = sig {
                let _ = self.backend.cmds.send(Cmd::ReadDid { tx_id: tx, did });
            }
            self.widgets.push(dashboard::Widget::card(sig));
        }
        self.save_dashboard();
    }

    fn render_widget(&self, ix: usize, w: &dashboard::Widget, shown: &BTreeMap<String, f64>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let info = self.sig_info(&w.sig);
        let value = self.sig_value(&w.sig, shown);
        let source = self.sig_source(&w.sig);
        let last = self.widgets.len().saturating_sub(1);
        let seg = |label: &'static str, view: dashboard::View, cx: &mut Context<Self>| {
            Button::new(SharedString::from(format!("v{ix}-{label}"))).small().label(label).when(w.view == view, |b| b.primary()).on_click(cx.listener(move |this, _, _, cx| {
                this.widgets[ix].view = view;
                this.save_dashboard();
                cx.notify();
            }))
        };
        let is_chart = w.view != dashboard::View::Card;
        let ranged = info.max > info.min;
        let edit_bar = h_flex()
            .gap_1()
            .flex_wrap()
            .items_center()
            .child(seg("Card", dashboard::View::Card, cx))
            .child(seg("Line", dashboard::View::Line, cx))
            .child(seg("Area", dashboard::View::Area, cx))
            .when(is_chart, |this| {
                this.child(Button::new(SharedString::from(format!("w{ix}-size"))).small().label(format!("Width: {}", dashboard::SIZES[w.size as usize % 3].0)).on_click(cx.listener(move |this, _, _, cx| {
                    this.widgets[ix].size = (this.widgets[ix].size + 1) % 3;
                    this.save_dashboard();
                    cx.notify();
                })))
                .child(Button::new(SharedString::from(format!("w{ix}-win"))).small().label(format!("Last {}", dashboard::WINDOWS[w.window as usize % 4].0)).on_click(cx.listener(move |this, _, _, cx| {
                    this.widgets[ix].window = (this.widgets[ix].window + 1) % 4;
                    this.save_dashboard();
                    cx.notify();
                })))
                .when(ranged, |this| {
                    this.child(Button::new(SharedString::from(format!("w{ix}-axis"))).small().label(if w.lock_range { "Axis: fixed" } else { "Axis: fit data" }).on_click(cx.listener(move |this, _, _, cx| {
                        this.widgets[ix].lock_range = !this.widgets[ix].lock_range;
                        this.save_dashboard();
                        cx.notify();
                    })))
                })
            })
            .child(Button::new(SharedString::from(format!("w{ix}-left"))).small().label("◀").disabled(ix == 0).on_click(cx.listener(move |this, _, _, cx| {
                this.widgets.swap(ix, ix - 1);
                this.save_dashboard();
                cx.notify();
            })))
            .child(Button::new(SharedString::from(format!("w{ix}-right"))).small().label("▶").disabled(ix == last).on_click(cx.listener(move |this, _, _, cx| {
                this.widgets.swap(ix, ix + 1);
                this.save_dashboard();
                cx.notify();
            })))
            .child(Button::new(SharedString::from(format!("w{ix}-del"))).small().danger().label("Remove").on_click(cx.listener(move |this, _, _, cx| {
                this.widgets.remove(ix);
                this.save_dashboard();
                cx.notify();
            })));

        let body = if !is_chart {
            let note = match &value {
                Ok(_) => match &w.sig {
                    dashboard::Sig::Param { tx, did, .. } => {
                        let raw = self.param_values.get(&(*tx, *did)).and_then(|r| r.as_ref().ok()).map(|d| d.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")).unwrap_or_default();
                        Some(format!("{source} · raw {raw}"))
                    }
                    _ => None,
                },
                Err(why) => Some(why.clone()),
            };
            let _ = note;
            self.tile(ix, w, &info, &value, cx)
        } else {
            let (_, count) = dashboard::WINDOWS[w.window as usize % 4];
            let queue = self.series.get(&w.sig.id());
            let len = queue.map_or(0, |q| q.len());
            let skip = len.saturating_sub(count);
            let shown_len = len - skip;
            let data: Vec<Pt> = queue
                .map(|q| q.iter().skip(skip).enumerate().map(|(i, v)| Pt { t: format!("-{:.0}s", (shown_len - 1 - i) as f64 * TICK.as_secs_f64()), v: *v }).collect())
                .unwrap_or_default();
            let color = [theme.red, theme.blue, theme.green, theme.yellow][ix % 4];
            let tick = (count / 8).max(2);
            let span = info.max - info.min;
            let current = match &value {
                Ok(v) => format!("{} {}", format_value(*v, if ranged { span } else { 10.0 }), info.unit),
                Err(why) => why.clone(),
            };
            // axis limits and ticks on friendly numbers: 800, 1000, 1200…
            let (dmin, dmax) = if w.lock_range && ranged {
                (info.min, info.max)
            } else {
                data.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.v), hi.max(p.v)))
            };
            let (lo, hi, ticks) = health::nice_axis(dmin, dmax);
            let chart: AnyElement = if data.is_empty() {
                div().flex_1().flex().items_center().justify_center().text_color(theme.muted_foreground).child("Waiting for data…").into_any_element()
            } else if w.view == dashboard::View::Line {
                LineChart::new(data)
                    .x(|p: &Pt| p.t.clone())
                    .y(|p: &Pt| p.v)
                    .stroke(color)
                    .tick_margin(tick)
                    .y_domain(lo, hi)
                    .y_axis(true)
                    .y_tick_count(ticks)
                    .y_tick_format(|v| health::compact(v))
                    .into_any_element()
            } else {
                let c = AreaChart::new(data)
                    .x(|p: &Pt| p.t.clone())
                    .y(|p: &Pt| p.v)
                    .stroke(color)
                    .fill(linear_gradient(0., linear_color_stop(color.opacity(0.4), 1.), linear_color_stop(theme.background.opacity(0.1), 0.)))
                    .tick_margin(tick)
                    .y_domain(lo, hi)
                    .y_axis(true)
                    .y_tick_count(ticks)
                    .y_tick_format(|v| health::compact(v));
                c.into_any_element()
            };
            v_flex()
                .w(px(dashboard::SIZES[w.size as usize % 3].1))
                .h(px(260.))
                .border_1()
                .border_color(theme.border)
                .rounded(theme.radius)
                .child(
                    h_flex()
                        .px_3()
                        .py_2()
                        .gap_2()
                        .items_baseline()
                        .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(info.name.clone()))
                        .child(div().text_sm().text_color(theme.muted_foreground).child(source))
                        .child(div().ml_auto().text_xl().font_weight(FontWeight::SEMIBOLD).child(current)),
                )
                .child(div().px_3().text_xs().text_color(theme.muted_foreground).child(if info.unit.is_empty() { "value".to_string() } else { format!("{} ({}) · time axis: seconds ago", info.name, info.unit) }))
                .child(chart)
                .into_any_element()
        };
        let label = info.name.clone();
        v_flex()
            .id(SharedString::from(format!("widget-{ix}")))
            .gap_1()
            .cursor_grab()
            .on_drag(DragWidget(ix), move |_, _, _, cx| {
                let label = label.clone();
                cx.new(|_| DragPreview { label })
            })
            .drag_over::<DragWidget>(|style, _, _, _| style.bg(gpui_kit::hsla(0.6, 0.8, 0.5, 0.18)))
            .on_drop(cx.listener(move |this, from: &DragWidget, _, cx| {
                if from.0 < this.widgets.len() && ix < this.widgets.len() && from.0 != ix {
                    let w = this.widgets.remove(from.0);
                    this.widgets.insert(ix, w);
                    this.save_dashboard();
                }
                cx.notify();
            }))
            .when(!is_chart, |d| if self.ui.bench { d.w(relative(0.485)) } else { d.w(px(236.)) })
            .child(body)
            .when(self.editing, |this| this.child(edit_bar))
            .into_any_element()
    }

    /// The signal picker: engine channels and each module's own values, searchable, added with one click.
    fn picker_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.picker {
            return None;
        }
        let theme = cx.theme().clone();
        let search = self.input_for("pick-search".into(), "Search, e.g. coolant, boost, injector", window, cx);
        let query = search.read(cx).value().trim().to_lowercase();
        let tabs: Vec<(Option<u32>, String)> = std::iter::once((None, "Engine".to_string()))
            .chain(self.modules.iter().filter(|m| !params::get().for_module(m.tx_id).is_empty()).map(|m| (Some(m.tx_id), self.module_title(m.tx_id))))
            .collect();
        let tab = self.picker_tab.filter(|id| tabs.iter().any(|(t, _)| *t == Some(*id)));
        let on_dashboard = |sig: &dashboard::Sig| self.widgets.iter().any(|w| &w.sig == sig);
        let row = |id: String, name: String, unit: String, sig: dashboard::Sig, cx: &mut Context<Self>| {
            let added = on_dashboard(&sig);
            h_flex()
                .gap_3()
                .px_2()
                .py_1()
                .items_center()
                .border_b_1()
                .border_color(theme.border.opacity(0.5))
                .child(div().w(px(70.)).flex_none().text_sm().text_color(theme.muted_foreground).child(id.clone()))
                .child(div().flex_1().text_base().child(if unit.is_empty() { name } else { format!("{name} ({unit})") }))
                .child(Button::new(SharedString::from(format!("pk-{}", sig.id()))).label(if added { "Remove" } else { "Add" }).when(!added, |b| b.primary()).on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_signal(sig.clone());
                    cx.notify();
                })))
                .into_any_element()
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        let note;
        match tab {
            None => {
                for c in self.channels.iter().filter(|c| query.is_empty() || c.name.to_lowercase().contains(&query)) {
                    rows.push(row(c.key.trim_start_matches("pid:").to_uppercase(), c.name.clone(), c.unit.clone(), dashboard::Sig::Obd(c.key.clone()), cx));
                }
                note = format!("{} engine signals", rows.len());
            }
            Some(tx) => {
                let supported = self.param_supported.get(&tx).filter(|_| self.picker_supported_only);
                let mut seen = BTreeSet::new();
                let all: Vec<&params::Param> = params::get()
                    .for_module(tx)
                    .iter()
                    .filter(|p| supported.is_none_or(|set| set.contains(&p.did)))
                    .filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query))
                    .filter(|p| seen.insert((p.did, p.bit)))
                    .collect();
                note = format!("{} of {} values", all.len().min(self.param_limit), all.len());
                for p in all.into_iter().take(self.param_limit) {
                    rows.push(row(format!("{:04X}", p.did), p.name.clone(), p.unit.clone(), dashboard::Sig::Param { tx, did: p.did, bit: p.bit }, cx));
                }
            }
        }
        let total_hint = rows.len() >= self.param_limit && tab.is_some();
        let probing = tab.and_then(|tx| self.probe.filter(|(id, ..)| *id == tx));
        let probed = tab.is_some_and(|tx| self.param_supported.contains_key(&tx));
        let card = v_flex()
            .w(px(720.))
            .h(px(640.))
            .max_w(relative(0.96))
            .max_h(relative(0.92))
            .gap_3()
            .p_5()
            .bg(theme.background)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .child(
                h_flex()
                    .items_center()
                    .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child("Add to dashboard"))
                    .child(div().flex_1())
                    .child(Button::new("pick-done").with_size(btn()).primary().label(i18n::tr("Done")).on_click(cx.listener(|this, _, _, cx| {
                        this.picker = false;
                        cx.notify();
                    }))),
            )
            .child(h_flex().gap_2().flex_wrap().children(tabs.into_iter().map(|(id, label)| {
                Button::new(SharedString::from(format!("pt-{}", id.unwrap_or(0))))
                    .label(label)
                    .when(id == tab, |b| b.primary())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.picker_tab = id;
                        this.param_limit = 60;
                        cx.notify();
                    }))
            })))
            .child(Input::new(&search))
            .when(tab.is_some(), |this| {
                let tx = tab.unwrap_or(0);
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .child(match probing {
                            Some(_) => Button::new("pk-stop").label("Stop").on_click(cx.listener(|this, _, _, cx| {
                                let _ = this.backend.cmds.send(Cmd::CancelSweep);
                                cx.notify();
                            })),
                            None => Button::new("pk-probe").label("Check which this module has").on_click(cx.listener(move |this, _, _, cx| {
                                let mut dids: Vec<u16> = params::get().for_module(tx).iter().map(|p| p.did).collect();
                                dids.sort_unstable();
                                dids.dedup();
                                this.probe = Some((tx, 0, dids.len()));
                                let _ = this.backend.cmds.send(Cmd::ProbeDids { tx_id: tx, dids });
                                cx.notify();
                            })),
                        })
                        .when(probed, |this| {
                            this.child(Button::new("pk-only").label(if self.picker_supported_only { "Showing only what it has" } else { "Showing everything: show only what it has" }).when(self.picker_supported_only, |b| b.primary()).on_click(cx.listener(|this, _, _, cx| {
                                this.picker_supported_only = !this.picker_supported_only;
                                cx.notify();
                            })))
                        })
                        .child(div().text_sm().text_color(theme.muted_foreground).child(match (probing, probed) {
                            (Some((_, done, total)), _) => format!("Asking the module… {done} of {total} (read-only)"),
                            (None, true) => "Done: switch to 'only what it has' to hide the rest.".into(),
                            _ => "A module only answers for values it has. This asks it, read-only (a minute or two for the engine ECU).".into(),
                        })),
                )
            })
            .child(div().text_sm().text_color(theme.muted_foreground).child(note))
            .child(v_flex().id("pick-list").flex_1().min_h_0().overflow_y_scroll().children(rows).when(total_hint, |this| {
                this.child(h_flex().p_2().child(Button::new("pk-more").label("Show more").on_click(cx.listener(|this, _, _, cx| {
                    this.param_limit += 60;
                    cx.notify();
                }))))
            }));
        Some(
            div()
                .id("picker-modal")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::hsla(0., 0., 0., 0.6))
                .child(card)
                .into_any_element(),
        )
    }

    /// What to show when there is nothing to show yet: why, and what the mechanic can do about it.
    fn empty_state(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (title, hint) = match &self.link {
            Link::Offline(reason) => (
                format!("Adapter offline: {reason}"),
                format!(
                    "Check that the adapter is plugged into the car's OBD-II port (under the dash) and the cable is secure, and that the ignition is ON. Still trying (attempt {}).",
                    self.offline_attempts
                ),
            ),
            Link::Connecting => ("Connecting to the adapter…".to_string(), "This takes a few seconds.".to_string()),
            Link::Online { .. } => (
                "Waiting for the engine ECU to answer…".to_string(),
                "Turn the ignition ON (some cars need the engine running). If nothing appears, the car may not support OBD-II live data.".to_string(),
            ),
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .px_8()
            .child(div().text_xl().font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child(title))
            .child(div().text_base().text_color(theme.muted_foreground).child(hint))
            .into_any_element()
    }





    /// Title and one line of context under it.
    fn page_subtitle(&self, cx: &App) -> String {
        match self.page {
            Page::Overview => match &self.link {
                Link::Online { .. } if self.scanning => "Scanning modules…".into(),
                Link::Online { .. } => format!("{} module(s) scanned", self.modules.len()),
                Link::Connecting => "Connecting…".into(),
                Link::Offline(reason) => format!("Adapter offline: {reason}"),
            },
            Page::Codes => match self.faults(cx).len() {
                0 if self.modules.is_empty() => "No codes in the engine ECU (other modules not scanned)".into(),
                0 => "No stored or pending trouble codes in any module".into(),
                n => format!("{n} trouble code(s)"),
            },
            Page::Live => format!("{} on the dashboard", self.widgets.len()),
            Page::Readiness => match &self.readiness {
                Some(r) => format!("{} of {} monitors ready", r.monitors.iter().filter(|m| m.supported && m.complete).count(), r.monitors.iter().filter(|m| m.supported).count()),
                None => "Not read yet".into(),
            },
            Page::Modules => self.notice.clone().unwrap_or_else(|| {
                if self.modules.is_empty() { "Scan the vehicle's diagnostic network for modules (read-only).".into() } else { format!("{} module(s)", self.modules.len()) }
            }),
            Page::Functions => i18n::tr("Tests and procedures each module offers. Nothing runs until you confirm.").into(),
            Page::Vehicles => format!("{} saved · the VIN is read from the car when possible", self.vehicles.len()),
            Page::Contribute => "Name modules and describe codes the shared database does not know yet.".into(),
            Page::Procedures => i18n::tr("Guided jobs, step by step. Every step that acts on the car asks you first.").into(),
            Page::History => format!("{} earlier visit(s) · {} backup(s) for this vehicle", self.ws.history.len(), self.ws.backups.len()),
            Page::Trace => "Every message sent to and received from the car, for finding out why a module does not answer.".into(),
        }
    }

    fn render_vehicles(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let rows = self.vehicles.iter().enumerate().map(|(ix, v)| {
            let on = self.current == Some(ix);
            h_flex()
                .gap_3()
                .px_3()
                .h(px(56.))
                .items_center()
                .border_b_1()
                .border_color(theme.border.opacity(0.7))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(v.name.clone()))
                        .child(div().text_sm().text_color(theme.muted_foreground).child(format!(
                            "{}  ·  {}",
                            v.make,
                            v.vin.as_deref().map_or("no VIN".to_string(), health::mask_vin)
                        ))),
                )
                .children(v.vin.clone().map(|vin| {
                    Button::new(SharedString::from(format!("copy-vin-{ix}"))).ghost().icon(IconName::Copy).label(i18n::tr("Copy VIN")).on_click(cx.listener(
                        move |this, _, _, cx| this.copy_text(vin.clone(), "the VIN", cx),
                    ))
                }))
                .child(if on {
                    div().text_sm().text_color(theme.green).child("In use").into_any_element()
                } else {
                    Button::new(SharedString::from(format!("use-{ix}"))).label("Use").on_click(cx.listener(
                        move |this, _, window, cx| this.select_vehicle(ix, window, cx),
                    )).into_any_element()
                })
                .child(
                    Button::new(SharedString::from(format!("remove-{ix}"))).ghost().icon(IconName::Delete).on_click(cx.listener(
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
                            .child(div().w(px(80.)).text_sm().text_color(theme.muted_foreground).child("Name"))
                            .child(div().flex_1().child(Input::new(&self.name_input))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().w(px(80.)).text_sm().text_color(theme.muted_foreground).child("Make"))
                            .child(div().w(px(240.)).child(Select::new(&self.make_select).placeholder("Select make…").menu_width(px(240.)))),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().w(px(80.)).text_sm().text_color(theme.muted_foreground).child("VIN"))
                            .child(div().flex_1().child(Input::new(&self.vin_input))),
                    )
                    .child(h_flex().pt_1().child(Button::new("add-vehicle").with_size(btn()).primary().label("Add vehicle").on_click(cx.listener(
                        |this, _, window, cx| this.add_vehicle(window, cx),
                    )))),
            )
    }
}

/// Tablet mode: everything bigger, for touch use at the car. Kept in `<data dir>/size`.
static TABLET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn tablet() -> bool {
    TABLET.load(std::sync::atomic::Ordering::Relaxed)
}

/// Button size for the main actions.
fn btn() -> Size {
    if tablet() { Size::Large } else { Size::Medium }
}

fn load_tablet() {
    let on = dtcdb::data_dir().and_then(|d| std::fs::read_to_string(d.join("size")).ok()).is_some_and(|t| t.trim() == "tablet");
    TABLET.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The root view derives the rem size from the theme's font size on every frame, so that is what
/// has to change (setting the window's rem size directly is overwritten).
fn set_font_size(tablet_mode: bool, window: &mut Window, cx: &mut App) {
    let size = px(if tablet_mode { 20. } else { 14. });
    Theme::global_mut(cx).font_size = size;
    window.set_rem_size(size);
}

/// Applies the mode: text, spacing and rows are sized in rems, so one number scales them all.
fn apply_size(tablet_mode: bool, window: &mut Window, cx: &mut App) {
    TABLET.store(tablet_mode, std::sync::atomic::Ordering::Relaxed);
    set_font_size(tablet_mode, window, cx);
    if let Some(dir) = dtcdb::data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("size"), if tablet_mode { "tablet" } else { "normal" });
    }
}

fn level_color(level: health::Level, theme: &gpui_kit::component::Theme) -> Hsla {
    match level {
        health::Level::Good => theme.green,
        health::Level::Warn => theme.yellow,
        health::Level::Bad => theme.red,
        health::Level::Unknown => theme.muted_foreground,
    }
}

fn fault_color(status: &str, theme: &gpui_kit::component::Theme) -> Hsla {
    if status == "pending" { theme.yellow } else { theme.red }
}

/// Fewer decimals for wide ranges (rpm, km/h), more for narrow ones (volts, bar).
fn format_value(v: f64, span: f64) -> String {
    let decimals = if span >= 50.0 { 0 } else if span >= 5.0 { 1 } else { 2 };
    format!("{v:.decimals$}")
}

/// Hot reload (`scripts/hot.sh`, feature `hot`): the whole UI is rebuilt inside a subsecond call,
/// so a patched `render_ui` or any page it calls takes effect on the next frame.
#[cfg(feature = "hot")]
fn hot<R>(f: impl FnMut() -> R) -> R {
    subsecond::call(f)
}

#[cfg(not(feature = "hot"))]
fn hot<R>(mut f: impl FnMut() -> R) -> R {
    f()
}

/// Set by the subsecond handler when a patch has been applied, so the next pump redraws.
static PATCHED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Render for Diagnostics {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        hot(|| self.render_frame(window, cx))
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

/// `CANFORGE_WINDOW=1180x820` opens the window at that size (to look at the other layouts).
fn dev_window_size() -> gpui_kit::Size<Pixels> {
    std::env::var("CANFORGE_WINDOW")
        .ok()
        .and_then(|v| v.split_once('x').map(|(w, h)| (w.parse::<f32>().ok(), h.parse::<f32>().ok())))
        .and_then(|(w, h)| Some(size(px(w?), px(h?))))
        .unwrap_or(size(px(1280.), px(800.)))
}

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

    trace::install();
    i18n::load();
    units::load();
    #[cfg(feature = "hot")]
    {
        dioxus_devtools::connect_subsecond();
        subsecond::register_handler(std::sync::Arc::new(|| PATCHED.store(true, std::sync::atomic::Ordering::Relaxed)));
    }
    gpui_kit::application().with_assets(ui::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.bind_keys([KeyBinding::new("ctrl-k", OpenPalette, None), KeyBinding::new("cmd-k", OpenPalette, None)]);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(dev_window_size(), cx)),
            ..TitleBar::window_options()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            window.set_window_title("canforge — Diagnostics");
            load_tablet();
            ui::apply_theme(ui::ThemePref::load(), window, cx);
            let backend = backend::spawn(source.take().expect("window opened once"));
            cx.new(|cx| Diagnostics::new(backend, make.clone(), window, cx))
        })
        .expect("failed to open window");
    });
}
