//! The Readiness page: how many emission monitors are ready and whether the car would pass
//! inspection, each monitor with what finishes it, and the car's own self-tests (OBD mode 06) with
//! a bar that shows the pass zone and where the value sits in it.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{self, Tone, r};

/// What it takes for a monitor to finish, as general guidance.
pub fn monitor_hint(name: &str) -> &'static str {
    match name {
        "Misfire" | "Fuel system" | "Comprehensive components" => tr("Finishes within minutes of normal driving."),
        "Oxygen sensor heater" => tr("Completes after a cold start."),
        "EGR / VVT" => tr("Mixed driving with gentle acceleration and braking."),
        "Catalyst" | "Heated catalyst" => tr("Warm engine, then a steady 80–100 km/h cruise for several minutes."),
        "Evaporative system" => tr("Fuel between a quarter and three-quarters, parked for some hours, then a drive."),
        "Oxygen sensor" => tr("Warm engine, steady cruise, then a few minutes at idle."),
        _ => tr("Needs a longer drive. It finishes by itself."),
    }
}

impl Diagnostics {
    /// One self-test: name, verdict, the limits bar and the numbers.
    fn self_test_row(&self, t: &monitors::Test, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let signed = t.uasid >= 0x80;
        let (val, lo, hi) = if signed {
            (f64::from(t.value as i16), f64::from(t.min as i16), f64::from(t.max as i16))
        } else {
            (f64::from(t.value), f64::from(t.min), f64::from(t.max))
        };
        let open_ended = !signed && t.max == 0xffff;
        let near = t.passed() && t.margin().is_some_and(|m| m < 0.15);
        let (tone, word) = if !t.passed() { (Tone::Bad, tr("FAIL")) } else if near { (Tone::Warn, tr("close to limit")) } else { (Tone::Ok, tr("pass")) };
        let color = tone.color(&theme);
        // the axis shows the pass zone and the value, with a little room either side
        let a_lo = lo.min(val);
        let a_hi = if open_ended { val.max(lo * 2.0).max(lo + 1.0) } else { hi.max(val) };
        let pad = ((a_hi - a_lo).abs() * 0.08).max(0.5);
        let (a_lo, a_hi) = (a_lo - if a_lo < lo { pad } else { 0.0 }, a_hi + pad);
        let at = |v: f64| (((v - a_lo) / (a_hi - a_lo)).clamp(0.0, 1.0)) as f32;
        let zone_hi = if open_ended { a_hi } else { hi };
        v_flex()
            .gap_0p5()
            .py_2p5()
            .border_b_1()
            .border_color(theme.border.opacity(0.7))
            .child(
                h_flex()
                    .gap_3()
                    .child(div().flex_1().child(format!("{} · {} {:02X}", monitors::mid_name(t.mid), tr("test"), t.tid)))
                    .child(ui::verdict(tone, word, 16., &theme)),
            )
            .child(
                div()
                    .relative()
                    .h(px(10.))
                    .my_2()
                    .rounded_full()
                    .bg(theme.muted)
                    .child(div().absolute().top_0().bottom_0().left(relative(at(lo))).w(relative(at(zone_hi) - at(lo))).rounded_full().bg(theme.green.opacity(0.35)))
                    .child(div().absolute().top(px(-5.)).left(relative(at(val))).ml(px(-2.)).w(px(4.)).h(px(20.)).rounded(px(2.)).bg(color)),
            )
            .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(format!("{val} ({} {lo}–{})", tr("limits"), if open_ended { "∞".to_string() } else { hi.to_string() })))
            .into_any_element()
    }

    pub fn render_readiness_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let online = matches!(self.link, Link::Online { .. });
        let monitors: Vec<monitors::Test> = self.ws.monitors.clone().unwrap_or_default();
        let summary = match &self.readiness {
            None => ui::card(&theme).child(div().text_color(theme.muted_foreground).child(tr("Not read yet."))).into_any_element(),
            Some(rd) => {
                let list: Vec<_> = rd.monitors.iter().filter(|m| m.supported).collect();
                let ready = list.iter().filter(|m| m.complete).count();
                let all = !list.is_empty() && ready == list.len();
                let pct = if list.is_empty() { 0. } else { ready as f32 / list.len() as f32 * 100. };
                let grid = h_flex().flex_wrap().gap_x_6().gap_y_2().children(list.iter().map(|m| {
                    let tone = if m.complete { Tone::Ok } else { Tone::Warn };
                    h_flex()
                        .flex_1()
                        .min_w(px(300.))
                        .gap_2p5()
                        .items_start()
                        .py_2()
                        .border_b_1()
                        .border_color(theme.border.opacity(0.7))
                        .child(if m.complete { Icon::new(IconName::CircleCheck) } else { ui::clock_icon() }.size(px(18.)).mt_0p5().text_color(tone.color(&theme)))
                        .child(
                            v_flex()
                                .flex_1()
                                .child(h_flex().gap_1().child(div().font_weight(FontWeight::SEMIBOLD).child(m.name)).child(div().text_color(tone.color(&theme)).child(format!("· {}", tr(if m.complete { "ready" } else { "NOT ready" })))))
                                .when(!m.complete, |d| d.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(monitor_hint(m.name)))),
                        )
                }));
                v_flex()
                    .gap_4()
                    .child(
                        ui::card(&theme)
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .justify_between()
                                    .gap_2()
                                    .child(div().text_size(r(18.)).line_height(r(26.)).font_weight(FontWeight::SEMIBOLD).child(tr("{n} of {m} monitors ready").replace("{n}", &ready.to_string()).replace("{m}", &list.len().to_string())))
                                    .child(ui::verdict(if all { Tone::Ok } else { Tone::Bad }, tr(if all { "Ready for inspection" } else { "Not ready for inspection yet" }), 18., &theme)),
                            )
                            .child(ui::bar(pct, 8., theme.foreground, &theme))
                            .when(!all, |d| d.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Monitors that are not ready finish by themselves during a drive cycle. It is normal right after codes were cleared or the battery was disconnected.")))),
                    )
                    .child(ui::card(&theme).child(grid).child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("General guide: the exact drive cycle differs from car to car."))))
                    .into_any_element()
            }
        };
        let rows: Vec<AnyElement> = monitors.iter().map(|t| self.self_test_row(t, cx)).collect();
        let none_yet = self.ws.monitors.is_none();
        let empty = self.ws.monitors.as_ref().is_some_and(|m| m.is_empty());
        self.page_box("readiness-page").child(summary).child(
            ui::card(&theme)
                .child(
                    h_flex()
                        .flex_wrap()
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(tr("Self-test results (OBD mode 06)")))
                        .child(self.sized(Button::new("read-monitors").label(tr("Read self-tests"))).disabled(!online).on_click(cx.listener(|this, _, _, cx| {
                            let _ = this.backend.cmds.send(Cmd::ReadMonitors);
                            this.toast = Some("Reading the engine ECU's self-tests…".into());
                            cx.notify();
                        }))),
                )
                .when(none_yet, |d| d.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Not read yet. These are the car's own checks of catalyst, misfire, oxygen sensors and EVAP, with their limits: a value near its limit shows a part wearing out before a code is set."))))
                .when(empty, |d| d.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("The engine ECU reported no self-test results (many cars do not support mode 06)."))))
                .children(rows),
        )
    }
}
