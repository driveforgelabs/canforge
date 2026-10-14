//! The Fault codes page as a list and a detail, the live evidence behind a code, and clearing codes
//! with an inline confirmation and the verify card that follows.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{self, Layout, Tone, r};

/// What the car's live data says about a code: one line, a longer sentence, and the signal to watch.
pub struct Evidence {
    pub short: String,
    pub long: String,
    pub key: String,
    pub name: String,
}

/// The live channel that bears on a code, by its number: intake air temperature for P0110–P0114,
/// air flow for P0100–P0104, and so on. Only codes with an obvious signal are listed.
fn evidence_key(code: &str) -> Option<&'static str> {
    let n: u32 = code.strip_prefix('P').and_then(|d| d.get(..4)).and_then(|d| d.parse().ok())?;
    Some(match n {
        100..=104 => "pid:10",
        105..=109 => "pid:0b",
        110..=114 => "pid:0f",
        115..=119 | 217 => "pid:05",
        120..=124 => "pid:11",
        170..=175 => "pid:07",
        300..=308 => "pid:0c",
        500..=503 => "pid:0d",
        562 | 563 => "pid:42",
        _ => return None,
    })
}

impl Diagnostics {
    /// The live reading behind `code`, in words, if the car reports it.
    pub fn evidence(&self, code: &str) -> Option<Evidence> {
        let key = evidence_key(code)?;
        let channel = self.channels.iter().find(|c| c.key == key)?;
        let name = channel.name.clone();
        if key == "pid:10" && self.engine_running() == Some(false) {
            return Some(Evidence {
                short: tr("Live: air flow needs the engine running").into(),
                long: tr("Air flow can only be read with the engine running. Start the engine, then rev it while watching the reading.").into(),
                key: key.into(),
                name,
            });
        }
        let value = self.readings.get(key).copied()?;
        let (n, u) = units::show_free(value, &channel.unit);
        let text = format!("{n} {u}").trim().to_string();
        let open_circuit = key == "pid:0f" && value <= -39.0;
        Some(Evidence {
            short: tr("Live: {name} reads {value}").replace("{name}", &name).replace("{value}", &text),
            long: format!(
                "{}{}",
                tr("{name} reads {value} on live data.").replace("{name}", &name).replace("{value}", &text),
                if open_circuit { format!(" {}", tr("That is the value a sensor shows when its circuit is open or unplugged. Start with its connector and wiring.")) } else { String::new() }
            ),
            key: key.into(),
            name,
        })
    }

    /// Opens the Live data page with `key` on the dashboard and picked out.
    pub fn watch_signal(&mut self, key: &str, cx: &mut Context<Self>) {
        let sig = dashboard::Sig::Obd(key.to_string());
        if !self.widgets.iter().any(|w| w.sig == sig) {
            self.toggle_signal(sig);
        }
        if !self.charted().iter().any(|k| k == key) {
            self.toggle_charted(key);
        }
        self.ui.hi_sig = Some(key.to_string());
        self.go(Page::Live, cx);
    }

    /// Opens the Fault codes page on `code`.
    pub fn open_code(&mut self, code: &str, cx: &mut Context<Self>) {
        self.ui.sel = Some(code.to_string());
        self.ui.detail = true;
        self.go(Page::Codes, cx);
    }

    /// Saves the report quietly (no browser) before something is erased, and says so in the visit.
    fn auto_save_report(&mut self, cx: &mut Context<Self>) {
        let report = self.build_report(cx);
        let result = dtcdb::data_dir().ok_or_else(|| "No data directory available (set CANFORGE_DATA_DIR).".to_string()).and_then(|dir| report::write(&dir, &report));
        match result {
            Ok(_) => self.record_action("Report saved automatically before clearing".into(), cx),
            Err(e) => self.toast = Some(e),
        }
    }

    fn do_clear(&mut self, cx: &mut Context<Self>) {
        self.confirm_clear = false;
        self.notice = None;
        self.auto_save_report(cx);
        self.begin_clear(cx);
        self.reading_codes = true;
        let _ = self.backend.cmds.send(Cmd::ClearDtcs);
        self.toast = Some(tr("Report saved, codes cleared.").into());
        cx.notify();
    }

    /// The confirmation shown in place before codes are erased.
    fn render_clear_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.confirm_clear {
            return None;
        }
        let theme = cx.theme().clone();
        let engine_on = self.engine_running() == Some(true);
        Some(
            ui::focus_card(&theme)
                .gap_2()
                .child(div().text_size(r(18.)).line_height(r(26.)).font_weight(FontWeight::SEMIBOLD).child(tr("Clear the engine ECU's codes?")))
                .child(div().text_color(theme.muted_foreground).child(tr("Erases the codes, freeze frame and readiness monitors. It cannot be undone. Codes held by other modules are not touched.")))
                .child(ui::verdict(Tone::Ok, tr("A report is saved automatically first."), 18., &theme).font_weight(FontWeight::NORMAL).text_color(theme.foreground))
                .when(engine_on, |d| d.child(ui::verdict(Tone::Bad, tr("The engine is running. Stop it first."), 18., &theme)))
                .child(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .flex_wrap()
                        .child(self.sized(Button::new("cancel").label(tr("Cancel"))).on_click(cx.listener(|this, _, _, cx| {
                            this.confirm_clear = false;
                            cx.notify();
                        })))
                        .child(self.sized(Button::new("confirm").danger().label(tr("Clear codes"))).disabled(engine_on).on_click(cx.listener(|this, _, _, cx| this.do_clear(cx)))),
                )
                .into_any_element(),
        )
    }

    /// Loads the shop's note for the open fault into the note box, once per fault.
    pub fn sync_code_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let faults = self.faults(cx);
        let Some(code) = self.ui.sel.clone().filter(|c| faults.iter().any(|f| &f.code == c)).or_else(|| faults.first().map(|f| f.code.clone())) else { return };
        let key = format!("{}|{}", self.vehicle_name(), code);
        if self.ws.code_note_for.as_deref() != Some(key.as_str()) {
            let text = notes::find(&self.ws.shop_notes, &self.vehicle_name(), &code).map(|n| n.text.clone()).unwrap_or_default();
            self.ws.code_note_input.update(cx, |s, cx| s.set_value(text, window, cx));
            self.ws.code_note_for = Some(key);
        }
    }

    /// What this shop has written about the fault on this vehicle, and a place to write more.
    fn render_shop_note(&self, f: &report::Fault, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let code = f.code.clone();
        let vehicle = self.vehicle_name();
        let saved = notes::find(&self.ws.shop_notes, &vehicle, &code).map(|n| n.text.clone());
        v_flex()
            .gap_1p5()
            .pt_3()
            .border_t_1()
            .border_color(theme.border)
            .child(ui::section_title(format!("{} · {vehicle}", tr("Your shop's notes")), &theme))
            .children(saved.map(|t| div().p_2().rounded(theme.radius).bg(theme.muted).child(t)))
            .child(Input::new(&self.ws.code_note_input).when(self.lay().touch(), |i| i.h(px(ui::TAP))))
            .child(h_flex().child(self.sized(Button::new("code-note-save").label(tr("Save note"))).on_click(cx.listener(move |this, _, _, cx| {
                let text = this.ws.code_note_input.read(cx).value().to_string();
                let vehicle = this.vehicle_name();
                this.toast = Some(match dtcdb::data_dir() {
                    Some(dir) => match notes::save(&dir, &mut this.ws.shop_notes, &vehicle, &code, &text) {
                        Ok(()) => tr("Note saved on this computer.").to_string(),
                        Err(e) => e,
                    },
                    None => "No data directory available (set CANFORGE_DATA_DIR).".into(),
                });
                cx.notify();
            }))))
            .into_any_element()
    }

    /// Whether this code was not on the previous visit.
    pub fn is_new_code(&self, code: &str) -> bool {
        self.ws.history.last().is_some_and(|last| !last.codes.iter().any(|c| c == code))
    }

    fn fault_list_row(&self, f: &report::Fault, selected: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let color = fault_color(&f.status, &theme);
        let code = f.code.clone();
        h_flex()
            .id(SharedString::from(format!("frow-{}-{}", f.code, f.module)))
            .w_full()
            .min_h(px(68.))
            .gap_3()
            .px_3()
            .py(px(10.))
            .items_center()
            .border_1()
            .rounded(px(10.))
            .cursor_pointer()
            .map(|d| if selected { d.border_color(theme.foreground).bg(theme.accent) } else { d.border_color(theme.border).hover(|d| d.bg(theme.accent)) })
            .child(div().w(px(62.)).flex_none().font_weight(FontWeight::SEMIBOLD).text_color(color).child(f.code.clone()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().child(f.description.clone()))
                    .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(f.module.clone())),
            )
            .child(ui::fault_status(&f.status, 16., &theme))
            .child(Icon::new(IconName::ChevronRight).size(px(18.)).text_color(theme.muted_foreground))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.ui.sel = Some(code.clone());
                this.ui.detail = true;
                cx.notify();
            }))
            .into_any_element()
    }

    fn render_code_detail(&self, f: &report::Fault, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let color = fault_color(&f.status, &theme);
        let hint = checks::lookup(&f.code);
        let evidence = self.evidence(&f.code);
        let from_engine = f.module.ends_with("7E0");
        let freeze = self.freeze.as_ref().filter(|z| from_engine && z.dtc.to_string() == f.code.get(..5).unwrap_or(&f.code));
        let sec = |title: &'static str| v_flex().gap_1p5().pt_3().border_t_1().border_color(theme.border).child(ui::section_title(tr(title), &theme));
        let code5 = f.code.get(..5).unwrap_or(&f.code).to_string();
        let touch = self.lay().touch();
        let list = |title: &'static str, items: &'static [&'static str]| {
            v_flex()
                .flex_1()
                .min_w(px(240.))
                .max_w_full()
                .gap_1p5()
                .child(ui::section_title(tr(title), &theme))
                .children(items.iter().map(|i| h_flex().gap_2().items_start().child(div().child("•")).child(div().flex_1().min_w_0().child(*i))))
        };
        // the suggested checks can be ticked off; the ticks are kept with the visit and the report
        let check_list = |items: &'static [&'static str], cx: &mut Context<Self>| {
            v_flex().flex_1().min_w(px(240.)).max_w_full().gap_0p5().child(ui::section_title(tr("What to check"), &theme)).children(items.iter().enumerate().map(|(i, text)| {
                let key = format!("{code5}: {text}");
                let on = self.ws.checks_done.contains(&key);
                h_flex()
                    .id(SharedString::from(format!("chk-{code5}-{i}")))
                    .gap_2p5()
                    .items_start()
                    .min_h(px(if touch { ui::TAP } else { 28. }))
                    .py_1()
                    .cursor_pointer()
                    .child(
                        div()
                            .mt_0p5()
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if on { theme.primary } else { theme.input })
                            .when(on, |d| d.bg(theme.primary).text_color(theme.primary_foreground).child(Icon::new(IconName::Check).size_3p5())),
                    )
                    .child(div().flex_1().min_w_0().when(on, |d| d.text_color(theme.muted_foreground)).child(*text))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.ws.checks_done.remove(&key) {
                            this.ws.checks_done.insert(key.clone());
                        }
                        this.touch_visit(cx);
                        cx.notify();
                    }))
            }))
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(14.))
            .p(px(20.))
            .border_1()
            .border_color(theme.border)
            .rounded(px(10.))
            .when(self.lay() == Layout::Portrait, |d| {
                d.child(h_flex().child(self.sized(Button::new("back").icon(IconName::ChevronLeft).label(tr("All codes"))).on_click(cx.listener(|this, _, _, cx| {
                    this.ui.detail = false;
                    cx.notify();
                }))))
            })
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x_3()
                    .gap_y_2()
                    .items_center()
                    .child(div().text_size(r(30.)).line_height(r(36.)).font_weight(FontWeight::SEMIBOLD).text_color(color).child(f.code.clone()))
                    .child(ui::fault_status(&f.status, 18., &theme).text_size(r(14.)))
                    .when(self.is_new_code(&f.code), |d| d.child(ui::tag(tr("NEW"), theme.red, &theme))),
            )
            .child(div().text_size(r(18.)).line_height(r(26.)).child(f.description.clone()))
            .child(div().text_color(theme.muted_foreground).child(f.module.clone()))
            .child({
                let (sev, why) = checks::severity(&f.code);
                let tone = match sev {
                    checks::Severity::Stop => Tone::Bad,
                    checks::Severity::Soon => Tone::Warn,
                    checks::Severity::Monitor => Tone::Muted,
                };
                ui::verdict(tone, format!("{}: {}", tr(sev.label()), tr(why)), 16., &theme)
            })
            .when(checks::group(&f.code).is_some_and(|g| g.id == "comms") && self.battery_weak(), |d| {
                d.child(ui::verdict(Tone::Bad, tr("The battery is weak. Low voltage makes modules drop out and set codes that are not real faults. Charge it and read the codes again before chasing this one."), 16., &theme).items_start())
            })
            .children(evidence.map(|e| {
                let key = e.key.clone();
                let has_value = self.readings.contains_key(&e.key);
                sec("Live evidence").child(div().child(e.long)).child(h_flex().pt_1().child(
                    self.sized(Button::new("watch").primary().label(format!("{} {}", tr("Watch"), e.name.to_lowercase()))).disabled(!has_value && false).on_click(cx.listener(move |this, _, _, cx| this.watch_signal(&key, cx))),
                ))
            }))
            .child(
                sec("Freeze frame")
                    .when_some(freeze, |d, z| {
                        d.child(h_flex().flex_wrap().gap_x_4().gap_y_2().children(z.values.iter().filter_map(|(p, v)| {
                            let info = pid::info(*p)?;
                            Some(v_flex().min_w(px(170.)).child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(info.name.to_string())).child(div().font_weight(FontWeight::SEMIBOLD).child({ let (n, u) = units::show_free(*v, info.unit); format!("{n} {u}") })))
                        })))
                    })
                    .when(freeze.is_none(), |d| d.child(div().text_color(theme.muted_foreground).child(tr(if from_engine { "No freeze frame is stored for this code." } else { "Not available from this module." })))),
            )
            .children(hint.map(|h| {
                v_flex().pt_3().border_t_1().border_color(theme.border).child(h_flex().w_full().flex_wrap().gap_4().items_start().child(list("Likely causes", h.causes)).child(check_list(h.checks, cx)))
            }))
            .child(self.render_shop_note(f, cx))
            .children(hint.map(|_| {
                div().text_size(r(12.)).text_color(theme.muted_foreground).child(if checks::is_specific(&f.code) {
                    tr("General guidance for this kind of code, not a diagnosis.")
                } else {
                    tr("General guidance for this system, not specific to this code. It is not a diagnosis.")
                })
            }))
            .into_any_element()
    }

    pub fn render_codes(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let online = matches!(self.link, Link::Online { .. });
        let faults = self.faults(cx);
        let portrait = self.lay() == Layout::Portrait;
        let sel = self.ui.sel.as_ref().filter(|c| faults.iter().any(|f| &f.code == *c)).cloned().or_else(|| faults.first().map(|f| f.code.clone()));
        let note = if self.scanning {
            Some(tr("Scanning the other modules (ABS, airbag, body…) for faults…"))
        } else if online && self.modules.is_empty() {
            Some(tr("Other modules have not been scanned yet, so faults in ABS, airbag, body and so on are not listed. Use Scan modules."))
        } else {
            None
        };
        let show_detail = !portrait || self.ui.detail;
        let show_list = !portrait || !self.ui.detail;
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut last_group = "";
        for f in &faults {
            if let Some(g) = checks::group(&f.code) {
                let members = faults.iter().filter(|o| checks::group(&o.code).is_some_and(|og| og.id == g.id)).count();
                if members >= 2 && g.id != last_group {
                    rows.push(
                        v_flex()
                            .pt_1()
                            .child(ui::section_title(format!("{}: {}", tr("Often one cause"), tr(g.title)), &theme))
                            .into_any_element(),
                    );
                }
                last_group = if members >= 2 { g.id } else { "" };
            } else {
                last_group = "";
            }
            rows.push(self.fault_list_row(f, sel.as_deref() == Some(f.code.as_str()), cx));
        }
        let detail = faults.iter().find(|f| Some(&f.code) == sel.as_ref()).map(|f| self.render_code_detail(f, cx));
        let confirm = self.render_clear_confirm(cx);
        let verify = self.render_verify_card(false, cx);
        self.page_box("codes")
            .children(self.notice.clone().map(|n| div().text_color(theme.foreground).child(n)))
            .children(note.map(|n| div().text_color(theme.yellow).child(n)))
            .children(confirm)
            .children(verify)
            .child(if faults.is_empty() {
                ui::card(&theme)
                    .child(ui::verdict(Tone::Ok, tr(if self.modules.is_empty() { "No trouble codes in the engine ECU." } else { "No trouble codes in any module." }), 18., &theme))
                    .into_any_element()
            } else {
                h_flex()
                    .items_start()
                    .gap_4()
                    .when(show_list, |d| d.child(v_flex().when(portrait, |d| d.w_full()).when(!portrait, |d| d.w(px(400.)).flex_none()).gap_2().children(rows)))
                    .when(show_detail, |d| d.children(detail))
                    .into_any_element()
            })
    }
}
