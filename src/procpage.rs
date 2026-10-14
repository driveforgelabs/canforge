//! The Procedures page: the list of guided jobs, and one job worked through step by step. Going back
//! to the list keeps the place; Stop forgets it.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{self, Tone, r};
use crate::workshop::ProcRun;

impl Diagnostics {
    fn tick_box(&self, on: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        h_flex()
            .id("proc-tick")
            .gap_3()
            .min_h(px(ui::TAP))
            .items_center()
            .cursor_pointer()
            .child(
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if on { theme.primary } else { theme.input })
                    .when(on, |d| d.bg(theme.primary).text_color(theme.primary_foreground).child(Icon::new(IconName::Check).size_4())),
            )
            .child(tr("I have done this"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.ui.proc_tick = !this.ui.proc_tick;
                cx.notify();
            }))
            .into_any_element()
    }

    pub fn render_procedures(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let page = self.page_box("procedures");
        let active = self.ws.proc.as_ref().map(|r| (r.index, r.step));
        let Some(run) = self.ws.proc.as_ref().filter(|_| self.ui.proc_open) else {
            return page.children(procedures::ALL.iter().enumerate().map(|(i, p)| {
                let state = active.filter(|(a, _)| *a == i);
                let status = state.map(|(_, step)| {
                    if step >= p.steps.len() { tr("Completed").to_string() } else { tr("In progress: step {n} of {m}").replace("{n}", &(step + 1).to_string()).replace("{m}", &p.steps.len().to_string()) }
                });
                let label = match state {
                    None => tr("Open"),
                    Some((_, step)) if step >= p.steps.len() => tr("Open again"),
                    Some(_) => tr("Resume"),
                };
                ui::card(&theme).child(
                    h_flex()
                        .flex_wrap()
                        .gap_3()
                        .items_center()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(240.))
                                .child(div().text_size(r(16.)).font_weight(FontWeight::SEMIBOLD).child(tr(p.title)))
                                .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr(p.summary)))
                                .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(format!("{} {}", p.steps.len(), tr("steps"))))
                                .children(status.map(|s| div().text_size(r(12.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.info).child(s))),
                        )
                        .child(self.sized(Button::new(SharedString::from(format!("proc-{i}"))).primary().label(label)).on_click(cx.listener(move |this, _, _, cx| {
                            let resume = this.ws.proc.as_ref().is_some_and(|r| r.index == i && r.step < procedures::ALL[i].steps.len());
                            if !resume {
                                this.ws.proc = Some(ProcRun { index: i, step: 0, waiting: None, skipped: BTreeSet::new() });
                            }
                            this.ui.proc_open = true;
                            this.ui.proc_tick = false;
                            cx.notify();
                        }))),
                )
            }).collect::<Vec<_>>());
        };
        let proc = &procedures::ALL[run.index];
        let (current, waiting) = (run.step, run.waiting);
        let skipped = run.skipped.clone();
        let volts = self.readings.get("pid:42").copied();
        let rpm = self.rpm();
        let finished = current >= proc.steps.len();
        let total = proc.steps.len();
        let modules: Vec<(u32, String)> = self.modules.iter().map(|m| (m.tx_id, self.module_title(m.tx_id))).collect();
        let known = functions::get();
        let tick = self.ui.proc_tick;
        let online = matches!(self.link, Link::Online { .. });
        let mut page = page
            .child(h_flex().child(self.sized(Button::new("proc-back").icon(IconName::ChevronLeft).label(tr("Back to the list"))).on_click(cx.listener(|this, _, _, cx| {
                this.ui.proc_open = false;
                cx.notify();
            }))))
            .child(
                h_flex()
                    .flex_wrap()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(div().text_size(r(20.)).line_height(r(28.)).font_weight(FontWeight::SEMIBOLD).child(tr(proc.title)))
                    .child(self.sized(Button::new("proc-stop").label(tr(if finished { "Done" } else { "Stop" }))).on_click(cx.listener(|this, _, _, cx| {
                        this.ws.proc = None;
                        this.ui.proc_open = false;
                        cx.notify();
                    }))),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(if finished { tr("All {m} steps done").replace("{m}", &total.to_string()) } else { tr("Step {n} of {m}").replace("{n}", &(current + 1).to_string()).replace("{m}", &total.to_string()) }))
                    .child(ui::bar(current as f32 / total as f32 * 100., 8., theme.foreground, &theme)),
            );
        for (n, step) in proc.steps.iter().enumerate() {
            let state = if skipped.contains(&n) { "skipped" } else if n < current { "done" } else if n == current { "now" } else { "later" };
            let mark = match state {
                "done" => Icon::new(IconName::CircleCheck).size_4().text_color(theme.green).into_any_element(),
                "skipped" => Icon::new(IconName::Minus).size_4().text_color(theme.muted_foreground).into_any_element(),
                "now" => Icon::new(IconName::Play).size_4().into_any_element(),
                _ => div().w(px(16.)).text_color(theme.muted_foreground).child("○").into_any_element(),
            };
            let title = match step {
                procedures::Step::Do(t) => tr(t).to_string(),
                procedures::Step::Require(need) => match need {
                    procedures::Need::EngineOff => tr("Engine stopped, ignition on").to_string(),
                    procedures::Need::EngineRunning => tr("Engine running").to_string(),
                    procedures::Need::VoltageAtLeast(v) => tr("Battery at least {v} V").replace("{v}", &format!("{v:.1}")),
                },
                procedures::Step::Run { label, .. } => tr(label).to_string(),
            };
            let mut card = v_flex()
                .gap_2()
                .px_4()
                .py_3()
                .border_1()
                .rounded(theme.radius_lg)
                .border_color(if state == "now" { theme.foreground } else { theme.border })
                .when(state == "later", |d| d.opacity(0.6))
                .child(h_flex().gap_2().items_start().child(div().mt_0p5().child(mark)).child(div().flex_1().when(state == "now", |d| d.font_weight(FontWeight::SEMIBOLD)).child(title)));
            if state == "now" {
                match step {
                    procedures::Step::Do(_) => {
                        card = card.child(self.tick_box(tick, cx));
                        if n + 1 == total {
                            let rpm_text = rpm.map_or("—".to_string(), |r| format!("{r:.0}"));
                            let volts_text = volts.map_or("—".to_string(), |v| format!("{v:.1}"));
                            card = card
                                .child(
                                    h_flex()
                                        .flex_wrap()
                                        .gap_x_6()
                                        .gap_y_1()
                                        .px_3()
                                        .py_2p5()
                                        .rounded(theme.radius)
                                        .bg(theme.muted)
                                        .child(h_flex().gap_1().child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Engine speed"))).child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{rpm_text} rpm"))))
                                        .child(h_flex().gap_1().child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Battery"))).child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{volts_text} V")))),
                                )
                                .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Keep this screen open. The procedure resumes at this step if the connection drops.")));
                        }
                        card = card.child(h_flex().child(self.sized(Button::new(SharedString::from(format!("step-{n}"))).primary().label(tr(if n + 1 == total { "Finish" } else { "Continue" }))).disabled(!tick).on_click(cx.listener(|this, _, _, cx| this.proc_advance(cx)))));
                    }
                    procedures::Step::Require(need) => {
                        let holds = procedures::holds(*need, volts, rpm);
                        let (tone, text) = match holds {
                            Some(true) => (Tone::Ok, tr("Satisfied.")),
                            Some(false) => (Tone::Bad, tr("Not satisfied yet. This updates by itself from the car's live data.")),
                            None => (Tone::Warn, tr("The car does not report this, so check it yourself.")),
                        };
                        card = card.child(ui::verdict_at(tone, text, 16., 12., &theme)).child(h_flex().child(
                            self.sized(Button::new(SharedString::from(format!("step-{n}"))).primary().label(tr(if holds.is_none() { "I checked it" } else { "Continue" }))).disabled(holds == Some(false)).on_click(cx.listener(|this, _, _, cx| this.proc_advance(cx))),
                        ));
                    }
                    procedures::Step::Run { module, function, .. } => {
                        let found = procedures::find(module, function, &modules, &|tx| known.for_module(tx).iter().map(|f| (f.title.clone(), f.runnable && f.routine_id().is_some())).collect());
                        let skip = move |this: &mut Self, cx: &mut Context<Self>| {
                            if let Some(r) = &mut this.ws.proc {
                                r.skipped.insert(n);
                            }
                            this.proc_advance(cx);
                        };
                        match found {
                            None => {
                                card = card
                                    .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("This car has no matching function in your function list, so the app cannot do this step. Do it with another tool, then skip it here.")))
                                    .child(h_flex().child(self.sized(Button::new(SharedString::from(format!("skip-{n}"))).label(tr("Skip"))).on_click(cx.listener(move |this, _, _, cx| skip(this, cx)))));
                            }
                            Some((tx, ix)) => {
                                let f = &known.for_module(tx)[ix];
                                let routine = f.routine_id().unwrap_or(0);
                                let result = if waiting == Some((tx, routine)) { self.run_results.get(&(tx, routine)) } else { None };
                                card = card.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(format!("{} · {}", f.title, self.module_title(tx))));
                                match result {
                                    None => {
                                        let confirming = self.confirm_run.is_some();
                                        card = card.child(h_flex().child(self.sized(Button::new(SharedString::from(format!("run-{n}"))).primary().label(tr("Run…"))).disabled(confirming || !online).on_click(cx.listener(move |this, _, _, cx| {
                                            this.run_results.remove(&(tx, routine));
                                            if let Some(r) = &mut this.ws.proc {
                                                r.waiting = Some((tx, routine));
                                            }
                                            this.confirm_run = Some((tx, ix));
                                            cx.notify();
                                        }))));
                                    }
                                    Some((Ok(_), after)) => {
                                        card = card
                                            .child(ui::verdict_at(Tone::Ok, tr("The module accepted the request."), 16., 12., &theme))
                                            .when(!after.is_empty(), |c| c.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(after.clone())))
                                            .child(h_flex().child(self.sized(Button::new(SharedString::from(format!("next-{n}"))).primary().label(tr("Next"))).on_click(cx.listener(|this, _, _, cx| this.proc_advance(cx)))));
                                    }
                                    Some((Err(why), _)) => {
                                        card = card.child(ui::verdict_at(Tone::Warn, format!("{}: {why}", tr("Not done")), 16., 12., &theme)).child(
                                            h_flex()
                                                .gap_2()
                                                .child(self.sized(Button::new(SharedString::from(format!("retry-{n}"))).label(tr("Try again"))).on_click(cx.listener(move |this, _, _, cx| {
                                                    this.run_results.remove(&(tx, routine));
                                                    cx.notify();
                                                })))
                                                .child(self.sized(Button::new(SharedString::from(format!("skip-{n}"))).label(tr("Skip"))).on_click(cx.listener(move |this, _, _, cx| skip(this, cx)))),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            page = page.child(card);
        }
        if finished {
            page = page.child(
                ui::card(&theme)
                    .child(ui::verdict(Tone::Ok, tr("Procedure finished. It is listed under this visit's work. Read the codes again to confirm nothing new appeared."), 18., &theme).items_start())
                    .child(h_flex().child(self.sized(Button::new("proc-read").label(tr("Read codes"))).on_click(cx.listener(|this, _, _, cx| {
                        this.ws.proc = None;
                        this.ui.proc_open = false;
                        this.start_read_codes();
                        this.go(Page::Codes, cx);
                    })))),
            );
        }
        page
    }
}
