//! The Live data page: value tiles with a sparkline, a combined chart of the tiles you tapped (each
//! line with its own dash pattern) with a time scrubber and a readout, a transport bar for pause and
//! record, and bench mode with very large numbers. The dashboard's own customisation (add, edit,
//! reorder, chart widgets) is unchanged and lives in `main.rs`.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{self, Layout, Series, r, TAP};

/// Samples in the combined chart: 60 s at one every 250 ms, both ends included.
const WINDOW: usize = 241;

/// One-tap sets of signals for the tests a mechanic does over and over: what is charted, in order.
pub const PRESETS: [(&str, &[&str]); 5] = [
    ("Misfire test", &["pid:0c", "pid:04", "pid:11", "pid:06"]),
    ("MAF check", &["pid:10", "pid:0c", "pid:0b", "pid:11"]),
    ("Fuel trims", &["pid:06", "pid:07", "pid:08", "pid:09"]),
    ("Charging system", &["pid:42", "pid:0c", "pid:04"]),
    ("Warm-up", &["pid:05", "pid:0f", "pid:0c", "pid:0d"]),
];

impl Diagnostics {
    /// Puts a preset's signals on the dashboard (those the car reports) and in the chart.
    pub fn apply_preset(&mut self, ix: usize) {
        let (name, keys) = PRESETS[ix];
        let have: Vec<String> = keys.iter().filter(|k| self.channels.iter().any(|c| c.key == **k)).map(|k| k.to_string()).collect();
        if have.is_empty() {
            self.toast = Some(tr("This car does not report the signals for this test.").into());
            return;
        }
        for key in &have {
            let sig = dashboard::Sig::Obd(key.clone());
            if !self.widgets.iter().any(|w| w.sig == sig) {
                self.toggle_signal(sig);
            }
        }
        self.toast = Some(format!("{}: {} {}", tr(name), have.len(), tr("signals")));
        self.ui.charted = Some(have);
        self.ui.hi_sig = None;
    }

    /// The signals in the combined chart. Until the user taps a tile, the first three cards.
    pub fn charted(&self) -> Vec<String> {
        match &self.ui.charted {
            Some(v) => v.clone(),
            None => self.widgets.iter().filter(|w| w.view == dashboard::View::Card).take(3).map(|w| w.sig.id()).collect(),
        }
    }

    pub fn toggle_charted(&mut self, id: &str) {
        let mut v = self.charted();
        if let Some(p) = v.iter().position(|x| x == id) {
            v.remove(p);
        } else {
            v.push(id.to_string());
        }
        self.ui.charted = Some(v);
        self.ui.hi_sig = None;
    }

    /// The last `n` samples of a signal, scaled to 0..1 over its own range (or over the data when it
    /// has none), newest last.
    fn scaled(&self, sig: &dashboard::Sig, info: &Channel, n: usize) -> Vec<f32> {
        let Some(q) = self.series.get(&sig.id()) else { return Vec::new() };
        let vals: Vec<f64> = q.iter().rev().take(n).rev().copied().collect();
        let (lo, hi) = if info.max > info.min {
            (info.min, info.max)
        } else {
            vals.iter().fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)))
        };
        let span = if hi > lo { hi - lo } else { 1.0 };
        vals.iter().map(|v| ((v - lo) / span) as f32).collect()
    }

    fn line_color(&self, ix: usize, theme: &gpui_kit::component::Theme) -> Hsla {
        [theme.blue, theme.yellow, theme.green, theme.info, theme.foreground, theme.muted_foreground][ix % 6]
    }

    /// One widget as a tile: value, range and sparkline. Tapping it puts it in the chart or takes
    /// it out; the line it gets there has its own dash pattern.
    pub fn tile(&self, ix: usize, w: &dashboard::Widget, info: &Channel, value: &Result<f64, String>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let id = w.sig.id();
        let charted = self.charted().contains(&id);
        let hl = self.ui.hi_sig.as_deref() == Some(id.as_str());
        let bench = self.ui.bench;
        let span = info.max - info.min;
        let warn = value.as_ref().is_ok_and(|v| info.warn_above.is_some_and(|w| *v > w));
        let spark = (!bench).then(|| self.scaled(&w.sig, info, WINDOW));
        let color = self.line_color(ix, &theme);
        let dash: Vec<f32> = ui::DASHES[ix % 6].to_vec();
        let key = id.clone();
        v_flex()
            .id(SharedString::from(format!("tile-{ix}")))
            .size_full()
            .min_h(px(if bench { 190. } else { 150. }))
            .gap_1p5()
            .p(px(14.))
            .border_1()
            .rounded(px(10.))
            .cursor_pointer()
            .map(|d| {
                if hl {
                    d.border_color(theme.foreground).shadow_xs()
                } else if charted {
                    d.border_color(theme.muted_foreground)
                } else {
                    d.border_color(theme.border)
                }
            })
            .hover(|d| d.bg(theme.accent))
            .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(info.name.clone()))
            .child(
                h_flex()
                    .gap_1()
                    .items_baseline()
                    .child(
                        div()
                            .text_size(r(if bench { if self.lay() == Layout::Portrait { 60. } else { 72. } } else { 30. }))
                            .line_height(r(if bench { if self.lay() == Layout::Portrait { 68. } else { 80. } } else { 36. }))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if warn { theme.red } else { theme.foreground })
                            .child(match value {
                                Ok(v) => units::show(*v, span, &info.unit).0,
                                Err(_) => "—".into(),
                            }),
                    )
                    .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(units::label(&info.unit)))
                    .when(warn, |d| d.child(div().ml_auto().text_size(r(12.)).font_weight(FontWeight::BOLD).text_color(theme.red).child("▲ HIGH"))),
            )
            .when_some(value.as_ref().err().filter(|_| !bench), |d, why| d.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(why.clone())))
            .when(!bench, |d| {
                d.children(self.extremes.get(&info.key).map(|(lo, hi)| {
                    div().text_size(r(12.)).text_color(theme.muted_foreground).child(format!("{} {} · {} {}", tr("min"), units::show(*lo, span, &info.unit).0, tr("max"), units::show(*hi, span, &info.unit).0))
                }))
                .child(div().w_full().h(px(34.)).mt_auto().child(ui::plot(vec![Series { values: spark.unwrap_or_default(), slots: 2, color, width: 1.6, dash }], None, None, 0.).size_full()))
                .when(charted, |d| d.child(h_flex().gap_1().items_center().text_size(r(12.)).font_weight(FontWeight::SEMIBOLD).child(Icon::new(IconName::Check).size_3p5()).child(tr("In chart"))))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_charted(&key);
                cx.notify();
            }))
            .into_any_element()
    }

    /// The combined chart: the tapped signals over the last minute, a scrubber, and the values at
    /// the scrubbed moment.
    fn render_live_chart(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let ids = self.charted();
        let picked: Vec<(usize, &dashboard::Widget)> = self.widgets.iter().enumerate().filter(|(_, w)| ids.contains(&w.sig.id())).collect();
        let scrub = self.ui.scrub.clamp(0., 60.);
        let at = scrub / 60.;
        let series: Vec<Series> = picked
            .iter()
            .map(|(ix, w)| {
                let info = self.sig_info(&w.sig);
                Series { values: self.scaled(&w.sig, &info, WINDOW), slots: WINDOW, color: self.line_color(*ix, &theme), width: 2.2, dash: ui::DASHES[*ix % 6].to_vec() }
            })
            .collect();
        let readout = picked.iter().map(|(ix, w)| {
            let info = self.sig_info(&w.sig);
            let q = self.series.get(&w.sig.id());
            let shown: Vec<f64> = q.map(|q| q.iter().rev().take(WINDOW).rev().copied().collect()).unwrap_or_default();
            let slot = (WINDOW as f32 - 1.) * at;
            let ix_in = (slot.round() as usize).saturating_sub(WINDOW.saturating_sub(shown.len()));
            let val = shown.get(ix_in).or(shown.last()).map(|v| units::show(*v, if info.max > info.min { info.max - info.min } else { 10.0 }, &info.unit).0);
            h_flex()
                .gap_2()
                .items_center()
                .child(div().w(px(28.)).h(px(10.)).child(ui::plot(vec![Series { values: vec![0.5, 0.5], slots: 2, color: self.line_color(*ix, &theme), width: 2.2, dash: ui::DASHES[*ix % 6].to_vec() }], None, None, 0.).size_full()))
                .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(info.name.clone()))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{} {}", val.unwrap_or_else(|| "—".into()), units::label(&info.unit))))
        });
        ui::card(&theme)
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(tr("Last 60 s")))
                    .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(if scrub >= 60. { tr("now").to_string() } else { format!("−{} s", 60 - scrub.round() as i32) })),
            )
            .child(if picked.is_empty() {
                div().py_6().text_color(theme.muted_foreground).child(tr("Tap a tile to add it to the chart.")).into_any_element()
            } else {
                div()
                    .relative()
                    .w_full()
                    .h(px(230.))
                    .child(ui::plot(series, Some((at, theme.foreground)), Some(theme.border), 44.).size_full())
                    .child(div().absolute().top(px(0.)).left_0().w(px(38.)).text_size(r(11.)).text_color(theme.muted_foreground).text_right().child(tr("max")))
                    .child(div().absolute().bottom(px(0.)).left_0().w(px(38.)).text_size(r(11.)).text_color(theme.muted_foreground).text_right().child(tr("min")))
                    .into_any_element()
            })
            .child(div().h(px(if self.lay().touch() { TAP } else { 28. })).flex().items_center().child(div().flex_1().child(Slider::new(&self.ui.scrub_state))))
            .child(h_flex().flex_wrap().gap_x_5().gap_y_2().children(readout))
            .into_any_element()
    }

    pub fn render_dashboard(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.channels.is_empty() {
            return self.empty_state(cx);
        }
        let theme = cx.theme().clone();
        let paused = self.paused.is_some();
        let bench = self.ui.bench;
        let shown = self.paused.as_ref().unwrap_or(&self.readings);
        let recording = self.recording.as_ref().map(|(_, rows)| rows.len());
        let gutter = self.lay().gutter();
        let (cards, charts): (Vec<usize>, Vec<usize>) = (0..self.widgets.len()).partition(|i| self.widgets[*i].view == dashboard::View::Card);
        let card_els: Vec<AnyElement> = cards.iter().map(|i| self.render_widget(*i, &self.widgets[*i], shown, cx)).collect();
        let chart_els: Vec<AnyElement> = charts.iter().map(|i| self.render_widget(*i, &self.widgets[*i], shown, cx)).collect();
        let chart_card = (!bench).then(|| self.render_live_chart(cx));

        let toolbar = (!bench).then(|| {
            h_flex()
                .gap_2()
                .items_center()
                .flex_wrap()
                .child(self.sized(Button::new("dash-add").label(tr("Add signal"))).on_click(cx.listener(|this, _, _, cx| {
                    this.picker = true;
                    cx.notify();
                })))
                .child(self.sized(Button::new("dash-edit").label(tr(if self.editing { "Done editing" } else { "Edit dashboard" })).when(self.editing, |b| b.primary())).on_click(cx.listener(|this, _, _, cx| {
                    this.editing = !this.editing;
                    cx.notify();
                })))
                .child(self.sized(Button::new("live-reset").label(tr("Reset min/max"))).on_click(cx.listener(|this, _, _, cx| {
                    this.extremes.clear();
                    cx.notify();
                })))
                .child(div().flex_1().min_w(px(240.)).text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Tap a tile to add it to the chart or remove it. Each trace is scaled to its own range.")))
        });

        let chart_now = self.charted();
        let presets = (!bench).then(|| {
            h_flex().gap_2().items_center().flex_wrap().child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Quick tests"))).children(PRESETS.iter().enumerate().map(|(i, (name, keys))| {
                let on = !chart_now.is_empty() && keys.iter().filter(|k| self.channels.iter().any(|c| c.key == **k)).all(|k| chart_now.iter().any(|c| c == k)) && chart_now.len() == keys.iter().filter(|k| self.channels.iter().any(|c| c.key == **k)).count();
                self.chip_button(format!("preset-{i}"), tr(name), on, cx, move |t, _, _| t.apply_preset(i))
            }))
        });
        let scroll = v_flex()
            .id("dashboard")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(gutter))
            .pt_4()
            .pb_6()
            .gap_4()
            .children(toolbar)
            .children(presets)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(self.sized(Button::new("bench").label(tr(if bench { "Exit bench mode" } else { "Bench mode" })).when(bench, |b| b.primary())).on_click(cx.listener(|this, _, _, cx| {
                        this.ui.bench = !this.ui.bench;
                        cx.notify();
                    })))
                    .when(paused, |d| d.child(ui::tag(tr("Paused"), theme.yellow, &theme))),
            )
            .when(self.widgets.is_empty(), |d| d.child(div().text_color(theme.muted_foreground).child(tr("Nothing on the dashboard yet. Press Add signal and pick what you want to watch."))))
            .child(h_flex().flex_wrap().items_start().gap_3().children(card_els))
            .children(chart_card)
            .when(!bench && !chart_els.is_empty(), |d| d.child(h_flex().flex_wrap().items_start().gap_3().children(chart_els)));

        let transport = h_flex()
            .flex_none()
            .flex_wrap()
            .gap_2()
            .items_center()
            .px(px(gutter))
            .py_3()
            .bg(theme.background)
            .border_t_1()
            .border_color(theme.border)
            .child(self.sized(Button::new("live-pause").icon(if paused { IconName::Play } else { IconName::Pause }).label(tr(if paused { "Resume" } else { "Pause" }))).on_click(cx.listener(|this, _, _, cx| {
                this.paused = if this.paused.is_some() { None } else { Some(this.readings.clone()) };
                cx.notify();
            })))
            .child(match recording {
                Some(_) => self
                    .sized(Button::new("live-rec").outline().danger().label(tr("Stop recording")))
                    .on_click(cx.listener(|this, _, _, cx| this.stop_recording(cx)))
                    .into_any_element(),
                None => self
                    .sized(Button::new("live-rec").label(tr("Record")))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.recording = Some((std::time::Instant::now(), Vec::new()));
                        this.ws.record_trace_from = Some(trace::latest());
                        cx.notify();
                    }))
                    .into_any_element(),
            })
            .when_some(recording, |d, n| d.child(h_flex().gap_2().items_center().font_weight(FontWeight::SEMIBOLD).child(div().size(px(10.)).rounded_full().bg(theme.red)).child(format!("{} · {n} {}", tr("Recording"), tr("rows")))));

        v_flex().size_full().child(scroll).child(transport).into_any_element()
    }
}
