//! The Functions page (module chips, search, filter chips, rows grouped by system) and the
//! confirmation shown before a function runs: a dialog on a laptop, a bottom sheet on touch layouts.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{self, Tone, r};

impl Diagnostics {
    pub fn render_functions(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let note = |text: &str| div().text_size(r(12.)).text_color(theme.muted_foreground).child(text.to_string());
        let known = functions::get();
        let page = self.page_box("function-list");
        if known.is_empty() {
            return page.child(note(tr("No function list is installed on this computer. It comes from your own FORScan database: see \"FORScan import\" in the README. The data stays on this computer.")));
        }
        if self.modules.is_empty() {
            return page.child(note(if self.scanning { tr("Scanning the modules…") } else { tr("No modules found yet. Press Scan modules on the Modules page.") }));
        }
        let mut tabs: Vec<(u32, String, usize)> = self
            .modules
            .iter()
            .filter(|m| !known.for_module(m.tx_id).is_empty())
            .map(|m| (m.tx_id, self.module_title(m.tx_id), known.for_module(m.tx_id).len()))
            .collect();
        if tabs.is_empty() {
            return page.child(note(tr("None of the scanned modules has functions in the import.")));
        }
        let selected = self.fn_module.filter(|id| tabs.iter().any(|t| t.0 == *id)).unwrap_or(tabs[0].0);
        tabs.sort_by_key(|t| t.0);
        let search = self.input_for("fn-search".into(), "Search, e.g. injector, EGR, steering angle", window, cx);
        let query = search.read(cx).value().trim().to_lowercase();
        let (only_run, need_run) = (self.ui.fn_only_run, self.ui.fn_need_run);
        let list = known.for_module(selected);
        // group by system; the same procedure listed for several engine variants appears once
        let mut groups: BTreeMap<&'static str, Vec<usize>> = BTreeMap::new();
        let mut seen: BTreeSet<(String, String, Vec<u8>, Option<u8>)> = BTreeSet::new();
        for (i, f) in list.iter().enumerate() {
            let words = query.split_whitespace().all(|w| f.title.to_lowercase().contains(w));
            if !words || (only_run && !(f.runnable && f.routine_id().is_some() && !f.needs_security())) || (need_run && f.engine_mode != Some(2)) {
                continue;
            }
            if seen.insert((f.title.clone(), f.routine.clone(), f.option.clone(), f.session)) {
                groups.entry(f.category()).or_default().push(i);
            }
        }
        let filtering = !query.is_empty() || only_run || need_run;
        let mut rows: Vec<AnyElement> = Vec::new();
        for (name, mut indexes) in groups {
            indexes.sort_by(|a, b| list[*a].title.cmp(&list[*b].title));
            let key = format!("{selected:03X}:{name}");
            let open = filtering || self.open_groups.contains(&key);
            let count = indexes.len();
            rows.push(
                h_flex()
                    .id(SharedString::from(format!("grp-{key}")))
                    .flex_none()
                    .gap_3()
                    .px_3()
                    .py_1p5()
                    .min_h(px(if self.lay().touch() { 56. } else { 36. }))
                    .items_center()
                    .bg(theme.muted)
                    .cursor_pointer()
                    .hover(|d| d.bg(theme.sidebar_accent))
                    .on_click(cx.listener({
                        let key = key.clone();
                        move |this, _, _, cx| {
                            if !this.open_groups.remove(&key) {
                                this.open_groups.insert(key.clone());
                            }
                            cx.notify();
                        }
                    }))
                    .child(div().flex_1().font_weight(FontWeight::SEMIBOLD).child(format!("{} {name}", if open { "▾" } else { "▸" })))
                    .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(count.to_string()))
                    .into_any_element(),
            );
            if open {
                for i in indexes {
                    rows.push(self.function_row(selected, i, &list[i], cx));
                }
            }
        }
        let empty = rows.is_empty();
        page.child(h_flex().gap_2().flex_wrap().children(tabs.iter().map(|(id, label, n)| {
            let id = *id;
            self.sized(Button::new(SharedString::from(format!("fm-{id}"))).label(format!("{label} ({n})")).when(id == selected, |b| b.primary())).on_click(cx.listener(move |this, _, _, cx| {
                this.fn_module = Some(id);
                cx.notify();
            }))
        })))
        .child(div().child(Input::new(&search).when(self.lay().touch(), |i| i.h(px(ui::TAP)))))
        .child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(self.chip_button("chip-only", tr("Runnable only"), only_run, cx, |t, _, _| t.ui.fn_only_run = !t.ui.fn_only_run))
                .child(self.chip_button("chip-need", tr("Needs engine running"), need_run, cx, |t, _, _| t.ui.fn_need_run = !t.ui.fn_need_run)),
        )
        .child(note(tr("Run… asks you to confirm first. Functions marked \"needs security access\" will be refused; procedures that need input the app cannot build are shown only.")))
        .child(v_flex().flex_none().border_1().border_color(theme.border).rounded(theme.radius_lg).overflow_hidden().children(rows).when(empty, |d| d.child(div().px_3().py_2().text_color(theme.muted_foreground).child(tr("Nothing matches.")))))
    }

    fn function_row(&self, tx_id: u32, i: usize, f: &functions::Function, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let mut detail = Vec::new();
        if !f.reads.is_empty() {
            detail.push(format!("{} {}", tr("reads"), f.reads));
        }
        if !f.writes.is_empty() {
            detail.push(format!("{} {}", tr("writes"), f.writes));
        }
        match f.engine_mode {
            Some(1) => detail.push(tr("engine OFF, ignition ON").to_string()),
            Some(2) => detail.push(tr("engine RUNNING").to_string()),
            _ => {}
        }
        if f.needs_security() {
            detail.push(tr("needs security access: will be refused").to_string());
        }
        let routine = f.routine_id();
        let result = routine.and_then(|r| self.run_results.get(&(tx_id, r)));
        let runnable = f.runnable && routine.is_some();
        v_flex()
            .flex_none()
            .px_3()
            .py_2()
            .gap_1()
            .min_h(px(if self.lay().touch() { 56. } else { 0. }))
            .border_t_1()
            .border_color(theme.border.opacity(0.7))
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().child(f.title.clone()))
                            .when(!detail.is_empty(), |d| d.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(detail.join(" · ")))),
                    )
                    .when(runnable, |d| {
                        d.child(self.sized(Button::new(SharedString::from(format!("run-{tx_id}-{i}"))).label(tr("Run…"))).disabled(!matches!(self.link, Link::Online { .. })).on_click(cx.listener(move |this, _, _, cx| {
                            this.confirm_run = Some((tx_id, i));
                            cx.notify();
                        })))
                    })
                    .when(!runnable, |d| d.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("display only")))),
            )
            .children(result.map(|(r_, after)| {
                let (tone, text) = match r_ {
                    Ok(bytes) => {
                        let hex = bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
                        (Tone::Ok, format!("{}{}. {}", tr("Module accepted the request"), if hex.is_empty() { String::new() } else { format!(" ({hex})") }, tr("It may still be running: check its result below, then re-check the codes.")))
                    }
                    Err(why) => (Tone::Warn, format!("{}: {why}", tr("Not done"))),
                };
                v_flex()
                    .gap_1()
                    .child(ui::verdict_at(tone, text, 16., 12., &theme))
                    .when(!after.is_empty(), |d| d.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(after.clone())))
                    .child(h_flex().child(self.sized(Button::new(SharedString::from(format!("recheck-{tx_id}-{i}"))).small().label(tr("Re-check codes"))).on_click(cx.listener(|this, _, _, cx| {
                        this.start_read_codes();
                        this.start_scan();
                        cx.notify();
                    }))))
            }))
            .into_any_element()
    }

    /// The confirmation shown before a function runs: what it does, whether the car is in the right
    /// state (checked against live data), and Start is unavailable while it is not.
    pub fn run_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (tx_id, i) = self.confirm_run?;
        let f = functions::get().for_module(tx_id).get(i)?;
        let routine = f.routine_id()?;
        let theme = cx.theme().clone();
        let running = self.engine_running();
        let rpm = self.rpm().map_or(String::new(), |r| format!(" ({} {r:.0} rpm)", tr("engine speed")));
        let state_check: Option<(String, Tone)> = match (f.engine_mode, running) {
            (Some(1), Some(true)) => Some((format!("{}{rpm}. {}", tr("The engine is running"), tr("Stop it and keep the ignition ON.")), Tone::Bad)),
            (Some(1), Some(false)) => Some((tr("Engine is stopped, as required.").into(), Tone::Ok)),
            (Some(2), Some(false)) => Some((tr("The engine is not running. Start it and let it idle.").into(), Tone::Bad)),
            (Some(2), Some(true)) => Some((tr("Engine is running, as required.").into(), Tone::Ok)),
            (Some(1), None) => Some((tr("Needs the engine stopped with ignition ON. The app cannot read engine speed to check, so make sure yourself.").into(), Tone::Warn)),
            (Some(2), None) => Some((tr("Needs the engine running. The app cannot read engine speed to check, so make sure yourself.").into(), Tone::Warn)),
            _ => None,
        };
        let gate = self.write_gate();
        let blocked = matches!(state_check, Some((_, Tone::Bad))) || matches!(gate, health::Gate::Block(_)) || !matches!(self.link, Link::Online { .. });
        let backup_dids = self.backup_dids(tx_id);
        let option_text = f.option.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
        let (session, option, title) = (f.session, f.option.clone(), f.title.clone());
        let module = format!("{}  ·  {:03X}", self.module_title(tx_id), tx_id);
        let row = |tone: Tone, text: String| ui::verdict(tone, text, 18., &theme).font_weight(FontWeight::NORMAL).items_start();
        let content = v_flex()
            .gap_3()
            .child(div().text_size(r(20.)).line_height(r(28.)).font_weight(FontWeight::SEMIBOLD).child(f.title.clone()))
            .child(div().text_color(theme.muted_foreground).child(module))
            .child(
                v_flex()
                    .gap_2()
                    .children(state_check.map(|(text, tone)| row(tone, text)))
                    .child(match &gate {
                        health::Gate::Ok(t) => row(Tone::Ok, format!("{} {t}", tr("Supply:"))),
                        health::Gate::Warn(t) => row(Tone::Warn, t.to_string()),
                        health::Gate::Block(t) => row(Tone::Bad, t.to_string()),
                    })
                    .child(match self.ws.backups.iter().find(|b| b.tx_id == tx_id) {
                        Some(b) => div().text_size(r(12.)).text_color(theme.muted_foreground).child(format!("{} {}", tr("Last backup of this module:"), report::stamp(b.unix_time))),
                        None => div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("No earlier backup of this module for this vehicle.")),
                    })
                    .child(if backup_dids.is_empty() {
                        row(Tone::Warn, tr("No readable data to back up for this module: it cannot be restored").into())
                    } else {
                        row(Tone::Ok, format!("{} ({} {})", tr("A backup of this module's data is saved first"), backup_dids.len(), tr("values")))
                    })
                    .when(f.needs_security(), |d| d.child(row(Tone::Warn, tr("Needs security access, which is not supported: the module will refuse.").into()))),
            )
            .child(div().child(tr("This starts a procedure in the module. It acts on the car and may change what is stored in it.")))
            .when(!f.reads.is_empty(), |d| d.child(div().text_color(theme.muted_foreground).child(format!("{} {}", tr("Reads:"), f.reads))))
            .when(!f.writes.is_empty(), |d| d.child(div().font_weight(FontWeight::SEMIBOLD).text_color(theme.yellow).child(format!("{} {}", tr("Writes:"), f.writes))))
            .child(div().text_size(r(12.)).text_color(theme.muted_foreground).font_family("monospace").child(format!(
                "{} UDS routine {routine:04X}{}{}",
                tr("Technical:"),
                if option_text.is_empty() { String::new() } else { format!(" with option {option_text}") },
                session.map_or(String::new(), |s| format!(", diagnostic session {s:02X}"))
            )))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .flex_wrap()
                    .child(self.sized(Button::new("run-cancel").label(tr("Cancel"))).on_click(cx.listener(|this, _, _, cx| {
                        this.confirm_run = None;
                        cx.notify();
                    })))
                    .child(self.sized(Button::new("run-start").danger().label(tr("Start"))).disabled(blocked).on_click(cx.listener(move |this, _, _, cx| {
                        this.confirm_run = None;
                        this.run_results.remove(&(tx_id, routine));
                        this.ws.run_context = Some((tx_id, routine, title.clone()));
                        let _ = this.backend.cmds.send(Cmd::RunRoutine { tx_id, routine, session, option: option.clone(), backup: backup_dids.clone() });
                        this.toast = Some(format!("Started \"{title}\" on module {tx_id:03X}."));
                        cx.notify();
                    }))),
            );
        Some(self.overlay("run-modal", false, false, content, |t| t.confirm_run = None, cx))
    }
}
