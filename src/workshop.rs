//! Workshop features that sit on top of the diagnostics: visit history and "what changed since last
//! time", backups before writes and their restore, guided procedures, the frame trace, the OBD
//! self-test (mode 06) panel, the connection card, and the Ctrl+K command palette.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::{backup, history, notes, i18n::tr, monitors, procedures, trace};

actions!(canforge, [OpenPalette]);

/// A guided procedure being worked through.
pub struct ProcRun {
    pub index: usize,
    /// The step in front of the user. Equal to the step count when the procedure is finished.
    pub step: usize,
    /// A function started from this procedure, whose answer the step waits for.
    pub waiting: Option<(u32, u16)>,
    pub skipped: BTreeSet<usize>,
}

#[derive(Clone)]
enum Go {
    Page(Page),
    Module(u32),
    Function(u32, usize),
    Procedure(usize),
    Command(Command),
    Code(String),
}

#[derive(Clone, Copy)]
enum Command {
    ScanAll,
    ReadCodes,
    SaveReport,
    SavePdf,
    CustomerPdf,
    Theme,
    Tablet,
    Language,
    Reconnect,
    Settings,
}

struct Entry {
    label: String,
    hint: String,
    go: Go,
}

pub struct Workshop {
    pub palette_open: bool,
    pub palette_input: Entity<InputState>,
    /// Earlier visits of the current vehicle, oldest first (this visit is not in it).
    pub history: Vec<history::Visit>,
    pub backups: Vec<backup::Backup>,
    loaded_key: Option<String>,
    /// Start of this visit: the name of its history file, so saving again overwrites it.
    pub started: u64,
    visit_saved: bool,
    /// What was done to the car this visit.
    pub actions: Vec<String>,
    /// The codes the engine ECU held when a clear was confirmed.
    pub clear_before: Option<Vec<String>>,
    /// What the car showed when the codes were cleared, and whether the repair held.
    pub verify: Option<crate::verify::Verify>,
    pub clear_result: Option<String>,
    /// Module title and what is about to run, for naming the backup that comes back.
    pub run_context: Option<(u32, u16, String)>,
    pub confirm_restore: Option<usize>,
    pub restore_msg: Option<String>,
    pub monitors: Option<Vec<monitors::Test>>,
    pub proc: Option<ProcRun>,
    pub trace_seen: u64,
    pub trace_frozen: Option<u64>,
    pub trace_from: u64,
    pub trace_hide_obd: bool,
    pub trace_filter: Entity<InputState>,
    /// Trace position when the recording started, so its trace can be saved with it.
    pub record_trace_from: Option<u64>,
    pub bitrate: Option<u32>,
    /// The shop's name (printed on reports), and for this visit: a note, the recommendation to the
    /// customer and the estimate.
    pub shop_input: Entity<InputState>,
    pub note_input: Entity<InputState>,
    pub reco_input: Entity<InputState>,
    pub est_input: Entity<InputState>,
    /// The shop's note for the fault open on the Fault codes page, and which fault it was loaded for.
    pub code_note_input: Entity<InputState>,
    pub code_note_for: Option<String>,
    pub shop_notes: Vec<notes::Note>,
    /// Suggested checks ticked this visit, as "P0113: Check the connector".
    pub checks_done: BTreeSet<String>,
    /// Photos attached to this visit (file names).
    pub photos: Vec<String>,
}

impl Workshop {
    pub fn new(window: &mut Window, cx: &mut Context<Diagnostics>) -> (Self, Vec<Subscription>) {
        let palette_input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Search pages, modules, functions…")));
        let trace_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter, e.g. 7E0 or 22 F1"));
        let subs = vec![
            cx.subscribe_in(&palette_input, window, |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => {
                    let query = this.ws.palette_input.read(cx).value().to_string();
                    if let Some(first) = this.palette_entries(&query, cx).into_iter().next() {
                        this.palette_go(first.go, window, cx);
                    }
                }
                _ => {}
            }),
            cx.subscribe_in(&trace_filter, window, |_, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let shop_input = cx.new(|cx| {
            let mut s = InputState::new(window, cx).placeholder(tr("Shop name (printed on reports)"));
            if let Some(name) = dtcdb::data_dir().and_then(|d| std::fs::read_to_string(d.join("shop")).ok()) {
                s.set_value(name.trim().to_string(), window, cx);
            }
            s
        });
        let note_input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Note on this visit (for the shop)")));
        let reco_input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("What you recommend to the customer")));
        let est_input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("Estimate, e.g. 180 € parts and labour")));
        let code_note_input = cx.new(|cx| InputState::new(window, cx).placeholder(tr("What you know about this fault on this vehicle")));
        let mut subs = subs;
        for input in [&note_input, &reco_input, &est_input] {
            subs.push(cx.subscribe_in(input, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.touch_visit(cx);
                }
            }));
        }
        subs.push(cx.subscribe_in(&shop_input, window, |_this, input, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(dir) = dtcdb::data_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(dir.join("shop"), input.read(cx).value().to_string());
                }
                cx.notify();
            }
        }));
        let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        (
            Self {
                palette_open: false,
                palette_input,
                history: Vec::new(),
                backups: Vec::new(),
                loaded_key: None,
                started,
                visit_saved: false,
                actions: Vec::new(),
                clear_before: None,
                verify: None,
                clear_result: None,
                run_context: None,
                confirm_restore: None,
                restore_msg: None,
                monitors: None,
                proc: None,
                trace_seen: 0,
                trace_frozen: None,
                trace_from: 0,
                trace_hide_obd: true,
                trace_filter,
                record_trace_from: None,
                bitrate: None,
                shop_input,
                note_input,
                reco_input,
                est_input,
                code_note_input,
                code_note_for: None,
                shop_notes: dtcdb::data_dir().map(|d| notes::load(&d)).unwrap_or_default(),
                checks_done: BTreeSet::new(),
                photos: Vec::new(),
            },
            subs,
        )
    }
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Diagnostics {
    // ---- visits, history and actions -------------------------------------------------------

    pub fn vehicle_name(&self) -> String {
        self.current.and_then(|ix| self.vehicles.get(ix)).map(|v| v.name.clone()).or_else(|| self.make.clone()).unwrap_or_else(|| "Vehicle".into())
    }

    pub fn vehicle_key(&self) -> String {
        history::key(self.vin.as_deref(), &self.vehicle_name())
    }

    /// What the car shows right now, as a history entry.
    pub fn current_visit(&self, cx: &App) -> history::Visit {
        let mut codes: Vec<String> = self.faults(cx).into_iter().map(|f| f.code).collect();
        codes.sort();
        codes.dedup();
        history::Visit {
            unix_time: self.ws.started,
            vehicle: self.vehicle_name(),
            vin: self.vin.clone(),
            mil_on: self.mil_on,
            codes,
            battery_volts: self.readings.get("pid:42").copied(),
            not_ready: self.readiness.as_ref().map(|r| r.monitors.iter().filter(|m| m.supported && !m.complete).map(|m| m.name.to_string()).collect()).unwrap_or_default(),
            actions: self.ws.actions.clone(),
            checks_done: self.ws.checks_done.iter().cloned().collect(),
            note: self.ws.note_input.read(cx).value().to_string(),
            recommendation: self.ws.reco_input.read(cx).value().to_string(),
            estimate: self.ws.est_input.read(cx).value().to_string(),
            photos: self.ws.photos.clone(),
        }
    }

    /// Something about the visit changed (a note, a tick, a photo): keep the saved visit current.
    pub fn touch_visit(&mut self, cx: &App) {
        if self.ws.visit_saved {
            self.save_visit(cx);
        }
    }

    fn save_visit(&mut self, cx: &App) {
        if let Some(dir) = dtcdb::data_dir() {
            let visit = self.current_visit(cx);
            if let Err(e) = history::save(&dir, &self.vehicle_key(), &visit) {
                log::warn!("could not save the visit: {e}");
            }
        }
    }

    /// Something was done to the car: remember it for the report and the history.
    pub fn record_action(&mut self, text: String, cx: &App) {
        self.ws.actions.push(text);
        if self.ws.visit_saved {
            self.save_visit(cx);
        }
    }

    /// Loads the vehicle's history when the vehicle changes, and saves the visit once the car has
    /// been read. Returns whether anything changed.
    pub fn sync_history(&mut self, cx: &App) -> bool {
        let mut changed = false;
        let key = self.vehicle_key();
        if self.ws.loaded_key.as_deref() != Some(key.as_str()) {
            let dir = dtcdb::data_dir();
            self.ws.history = dir.as_deref().map(|d| history::load(d, &key)).unwrap_or_default().into_iter().filter(|v| v.unix_time < self.ws.started).collect();
            self.ws.backups = dir.as_deref().map(|d| backup::load(d, &key)).unwrap_or_default();
            self.ws.loaded_key = Some(key);
            changed = true;
        }
        let ready = matches!(self.link, Link::Online { .. }) && !self.modules.is_empty() && self.readiness.is_some() && !self.scanning;
        if ready && !self.ws.visit_saved {
            self.ws.visit_saved = true;
            self.save_visit(cx);
            changed = true;
        }
        changed
    }

    /// How the car differs from the previous visit.
    pub fn since_last(&self, cx: &App) -> Option<report::SinceLast> {
        let last = self.ws.history.last()?;
        let d = history::diff(last, &self.current_visit(cx));
        Some(report::SinceLast { when: report::stamp(last.unix_time), new: d.new, resolved: d.resolved })
    }

    // ---- clearing codes --------------------------------------------------------------------

    /// The engine ECU's codes, as shown in the table.
    pub fn obd_codes(&self, cx: &App) -> Vec<String> {
        self.codes.read(cx).delegate().codes.iter().map(|e| e.code.to_string()).collect()
    }

    /// A clear was confirmed: remember what was there so the answer can say what changed.
    pub fn begin_clear(&mut self, cx: &App) {
        self.ws.clear_before = Some(self.obd_codes(cx));
        self.ws.clear_result = None;
        self.start_verify(cx);
    }

    /// The codes were read again: if that followed a clear, say how it went.
    pub fn finish_clear(&mut self, cx: &App) {
        let Some(before) = self.ws.clear_before.take() else { return };
        let now = self.obd_codes(cx);
        let back: Vec<&String> = before.iter().filter(|c| now.contains(c)).collect();
        let cleared = before.len() - back.len();
        let mut text = format!("Cleared {cleared} of {} code(s) in the engine ECU.", before.len());
        if !back.is_empty() {
            text.push_str(&format!(" {} came back at once: {}. The fault is still present.", tr("came back"), back.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
        } else if cleared > 0 {
            text.push_str(" None came back yet. Readiness monitors were reset: drive the car and read the codes again to confirm the repair.");
        }
        self.ws.clear_result = Some(text.clone());
        if let Some(v) = &mut self.ws.verify {
            v.cleared = true;
            v.last_read = std::time::Instant::now();
        }
        self.record_action(text, cx);
    }

    // ---- backups and restore ---------------------------------------------------------------

    /// The data identifiers of a module that have been read, to keep before something is written.
    pub fn backup_dids(&self, tx_id: u32) -> Vec<u16> {
        let mut dids: BTreeSet<u16> = BTreeSet::new();
        for l in self.dids.get(&tx_id).into_iter().flatten().filter(|l| !l.value.starts_with("exists")) {
            dids.insert(l.did);
        }
        dids.extend(self.param_supported.get(&tx_id).into_iter().flatten().copied());
        dids.extend(self.param_values.iter().filter(|((t, _), r)| *t == tx_id && r.is_ok()).map(|((_, d), _)| *d));
        dids.into_iter().collect()
    }

    pub fn on_backup(&mut self, tx_id: u32, routine: u16, values: Vec<(u16, Vec<u8>)>) {
        let what = self.ws.run_context.as_ref().filter(|(t, r, _)| *t == tx_id && *r == routine).map(|(_, _, w)| w.clone()).unwrap_or_else(|| format!("routine {routine:04X}"));
        let b = backup::Backup::new(unix_now(), tx_id, self.module_title(tx_id), what, &values);
        let msg = match dtcdb::data_dir() {
            Some(dir) => match backup::save(&dir, &self.vehicle_key(), &b) {
                Ok(_) => format!("Backup saved: {} value(s) of {}.", values.len(), self.module_title(tx_id)),
                Err(e) => format!("Backup NOT saved: {e}"),
            },
            None => "Backup NOT saved: no data directory.".into(),
        };
        self.ws.backups.insert(0, b);
        self.toast = Some(msg);
    }

    pub fn on_restored(&mut self, tx_id: u32, results: Vec<(u16, Result<String, String>)>, cx: &App) {
        let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
        let failed: Vec<String> = results.iter().filter_map(|(d, r)| r.as_ref().err().map(|e| format!("{d:04X}: {e}"))).collect();
        let mut text = format!("Restore on {}: {ok} of {} value(s) in place.", self.module_title(tx_id), results.len());
        if !failed.is_empty() {
            text.push_str(&format!(" Not written: {}.", failed.join("; ")));
        }
        self.ws.restore_msg = Some(text.clone());
        self.record_action(text, cx);
    }

    pub fn on_routine_run(&mut self, tx_id: u32, routine: u16, ok: bool, cx: &App) {
        let title = self.ws.run_context.as_ref().filter(|(t, r, _)| *t == tx_id && *r == routine).map(|(_, _, w)| w.clone()).unwrap_or_else(|| format!("routine {routine:04X}"));
        self.record_action(format!("{}: {} ({})", self.module_title(tx_id), title, if ok { "accepted by the module" } else { "not done" }), cx);
    }

    // ---- pages -----------------------------------------------------------------------------

    fn card(&self, cx: &Context<Self>) -> gpui_kit::Div {
        v_flex().gap_2().p_4().border_1().border_color(cx.theme().border).rounded(cx.theme().radius_lg)
    }

    /// Adapter, bus speed and a way to reconnect.
    pub fn render_connection(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let (state, color, adapter, ecu) = match &self.link {
            Link::Online { adapter, ecu } => ("Connected".to_string(), theme.green, adapter.clone(), ecu.map(|e| format!("{e:03X}"))),
            Link::Connecting => ("Connecting…".to_string(), theme.muted_foreground, String::new(), None),
            Link::Offline(why) => (format!("Offline: {why}"), theme.red, String::new(), None),
        };
        let source = if adapter.starts_with("Simulator") {
            "Simulator"
        } else if adapter.starts_with("Replay") {
            "Recording"
        } else {
            "Vehicle"
        };
        let row = |k: &'static str, v: String| h_flex().gap_2().text_sm().child(div().w(px(170.)).text_color(theme.muted_foreground).child(tr(k))).child(div().child(v));
        let can_reconnect = source == "Vehicle";
        self.card(cx)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(div().text_base().font_weight(FontWeight::SEMIBOLD).text_color(color).child(state))
                    .child(Button::new("reconnect").with_size(btn()).icon(IconName::RefreshCw).label(tr("Reconnect")).disabled(!can_reconnect).on_click(cx.listener(|this, _, _, cx| {
                        let _ = this.backend.cmds.send(Cmd::Reconnect);
                        this.toast = Some("Reconnecting to the adapter…".into());
                        cx.notify();
                    }))),
            )
            .child(row("Adapter", if adapter.is_empty() { "—".into() } else { adapter }))
            .child(row("Source", source.into()))
            .child(row("Bus speed", self.ws.bitrate.map_or("—".into(), |b| format!("{} kbit/s", b / 1000))))
            .child(row("Engine ECU", ecu.unwrap_or_else(|| "—".into())))
            .child(row("Automatic reconnect", if can_reconnect { "every 3 s while offline".into() } else { "not needed".into() }))
    }

    /// The supply check shared by every action that writes to a module.
    pub fn write_gate(&self) -> health::Gate {
        health::write_gate(self.readings.get("pid:42").copied(), self.rpm())
    }

    pub fn proc_advance(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &mut self.ws.proc else { return };
        run.step += 1;
        run.waiting = None;
        self.ui.proc_tick = false;
        let (index, step, skipped) = (run.index, run.step, run.skipped.len());
        let p = &procedures::ALL[index];
        if step >= p.steps.len() {
            let text = format!("Procedure completed: {}{}", p.title, if skipped > 0 { format!(" ({skipped} step(s) skipped)") } else { String::new() });
            self.record_action(text, cx);
        }
        cx.notify();
    }

    // ---- trace page ------------------------------------------------------------------------

    pub fn trace_lines(&self, cx: &App) -> Vec<trace::Line> {
        let filter = self.ws.trace_filter.read(cx).value().trim().to_lowercase();
        let words: Vec<&str> = filter.split_whitespace().collect();
        let upto = self.ws.trace_frozen.unwrap_or(u64::MAX);
        trace::since(self.ws.trace_from)
            .into_iter()
            .filter(|l| l.seq <= upto)
            .filter(|l| !(self.ws.trace_hide_obd && l.is_obd()))
            .filter(|l| self.ui.trace_dir.is_none_or(|tx| l.is_tx() == tx))
            .filter(|l| {
                let t = l.text.to_lowercase();
                words.iter().all(|w| t.contains(w))
            })
            .collect()
    }

    // ---- command palette -------------------------------------------------------------------

    fn palette_entries(&self, query: &str, cx: &App) -> Vec<Entry> {
        let words: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
        let hit = |e: &Entry| {
            let hay = format!("{} {}", e.label, e.hint).to_lowercase();
            words.iter().all(|w| hay.contains(w))
        };
        let mut all: Vec<Entry> = Vec::new();
        let mut pages: Vec<Page> = Page::groups().into_iter().flat_map(|(_, p)| p).collect();
        pages.push(Page::Vehicles);
        all.extend(pages.into_iter().map(|p| Entry { label: p.label().to_string(), hint: tr("Page").into(), go: Go::Page(p) }));
        let commands: [(&str, Command); 10] = [
            ("Scan all modules", Command::ScanAll),
            ("Read codes", Command::ReadCodes),
            ("Save report", Command::SaveReport),
            ("Save PDF", Command::SavePdf),
            ("Customer PDF", Command::CustomerPdf),
            ("Switch theme", Command::Theme),
            ("Tablet mode", Command::Tablet),
            ("Language: English / Português", Command::Language),
            ("Reconnect the adapter", Command::Reconnect),
            ("Settings", Command::Settings),
        ];
        all.extend(commands.into_iter().map(|(l, c)| Entry { label: tr(l).to_string(), hint: tr("Command").into(), go: Go::Command(c) }));
        if !words.is_empty() {
            all.extend(procedures::ALL.iter().enumerate().map(|(i, p)| Entry { label: tr(p.title).to_string(), hint: tr("Procedure").into(), go: Go::Procedure(i) }));
            all.extend(self.modules.iter().map(|m| Entry { label: self.module_title(m.tx_id), hint: format!("{} {:03X}", tr("Module"), m.tx_id), go: Go::Module(m.tx_id) }));
        }
        let mut out: Vec<Entry> = all.into_iter().filter(|e| hit(e)).collect();
        if !words.is_empty() {
            // fault codes found on the car, then the module functions (there are hundreds)
            out.extend(self.faults(cx).into_iter().map(|f| Entry { label: format!("{}  {}", f.code, f.description), hint: tr("Fault code").into(), go: Go::Code(f.code.clone()) }).filter(|e| hit(e)));
            let known = functions::get();
            let mut seen = BTreeSet::new();
            'modules: for m in &self.modules {
                for (i, f) in known.for_module(m.tx_id).iter().enumerate() {
                    if !seen.insert(f.title.clone()) {
                        continue;
                    }
                    let e = Entry { label: f.title.clone(), hint: format!("{} · {}", tr("Function"), self.module_title(m.tx_id)), go: Go::Function(m.tx_id, i) };
                    if hit(&e) {
                        out.push(e);
                        if out.len() >= 40 {
                            break 'modules;
                        }
                    }
                }
            }
        }
        out.truncate(12);
        out
    }

    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ws.palette_open = true;
        self.ws.palette_input.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    fn palette_go(&mut self, go: Go, window: &mut Window, cx: &mut Context<Self>) {
        self.ws.palette_open = false;
        match go {
            Go::Page(p) => {
                self.page = p;
                self.ensure_modules();
                self.ensure_vin();
            }
            Go::Module(tx) => {
                self.page = Page::Modules;
                self.expanded_modules.insert(tx);
            }
            Go::Function(tx, ix) => {
                self.page = Page::Functions;
                self.fn_module = Some(tx);
                if let Some(f) = functions::get().for_module(tx).get(ix) {
                    self.open_groups.insert(format!("{tx:03X}:{}", f.category()));
                    let title = f.title.clone();
                    let search = self.input_for("fn-search".into(), "Search, e.g. injector, EGR, steering angle", window, cx);
                    search.update(cx, |s, cx| s.set_value(title, window, cx));
                }
            }
            Go::Procedure(i) => {
                self.page = Page::Procedures;
                self.ws.proc = Some(ProcRun { index: i, step: 0, waiting: None, skipped: BTreeSet::new() });
                self.ui.proc_open = true;
            }
            Go::Code(code) => self.open_code(&code, cx),
            Go::Command(c) => match c {
                Command::ScanAll => {
                    self.page = Page::Overview;
                    self.start_read_codes();
                    self.start_scan();
                }
                Command::ReadCodes => {
                    self.page = Page::Codes;
                    self.start_read_codes();
                }
                Command::SaveReport => self.save_report(cx),
                Command::SavePdf => self.save_pdf(false, cx),
                Command::CustomerPdf => self.save_pdf(true, cx),
                Command::Theme => {
                    let dark = cx.theme().is_dark();
                    self.set_theme(if dark { ui::ThemePref::Light } else { ui::ThemePref::Dark }, window, cx);
                }
                Command::Tablet => apply_size(!tablet(), window, cx),
                Command::Language => i18n::set_pt(!i18n::is_pt()),
                Command::Settings => self.ui.settings = true,
                Command::Reconnect => {
                    let _ = self.backend.cmds.send(Cmd::Reconnect);
                    self.toast = Some("Reconnecting to the adapter…".into());
                }
            },
        }
        cx.notify();
    }

    pub fn palette_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.ws.palette_open {
            return None;
        }
        let theme = cx.theme().clone();
        let query = self.ws.palette_input.read(cx).value().to_string();
        let entries = self.palette_entries(&query, cx);
        let empty = entries.is_empty();
        let portrait = self.lay() == ui::Layout::Portrait;
        let touch = self.lay().touch();
        let content = v_flex()
            .size_full()
            .gap_1()
            .child(Input::new(&self.ws.palette_input).when(touch, |i| i.h(px(ui::TAP))))
            .child(
                v_flex()
                    .id("palette-list")
                    .when(portrait, |d| d.flex_1())
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(entries.into_iter().enumerate().map(|(i, e)| {
                        let go = e.go.clone();
                        h_flex()
                            .id(SharedString::from(format!("pal-{i}")))
                            .gap_3()
                            .px_2()
                            .min_h(px(if touch { 52. } else { 34. }))
                            .items_center()
                            .rounded(theme.radius)
                            .cursor_pointer()
                            .when(i == 0, |this| this.bg(theme.sidebar_accent))
                            .hover(|this| this.bg(theme.sidebar_accent))
                            .on_click(cx.listener(move |this, _, window, cx| this.palette_go(go.clone(), window, cx)))
                            .child(div().flex_1().truncate().child(e.label))
                            .child(div().text_size(ui::r(12.)).text_color(theme.muted_foreground).child(e.hint))
                    }))
                    .when(empty, |this| this.child(div().p_2().text_size(ui::r(12.)).text_color(theme.muted_foreground).child(tr("Nothing matches.")))),
            )
            .child(div().px_2().pt_1().text_size(ui::r(12.)).text_color(theme.muted_foreground).child(tr("Enter opens the first result · tap a result · tap outside to close")));
        Some(self.overlay("palette", true, portrait, content, |t| t.ws.palette_open = false, cx))
    }

    // ---- reports ---------------------------------------------------------------------------

    pub fn save_pdf(&mut self, customer: bool, cx: &mut Context<Self>) {
        let report = self.build_report(cx);
        self.toast = Some(match dtcdb::data_dir() {
            None => "No data directory available (set CANFORGE_DATA_DIR).".into(),
            Some(dir) => match report::write_pdf(&dir, &report, customer) {
                Ok(path) => {
                    let _ = std::process::Command::new("xdg-open").arg(&path).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
                    format!("{} saved: {}", if customer { "Customer PDF" } else { "PDF report" }, path.display())
                }
                Err(e) => e,
            },
        });
        cx.notify();
    }
}
