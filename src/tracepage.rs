//! The Trace page: every message sent to and received from the car, with a filter, quick chips for
//! the common addresses and directions, and pause, clear and copy.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{self, Layout, r};

impl Diagnostics {
    pub fn render_trace(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let lines = self.trace_lines(cx);
        let total = lines.len();
        let shown: Vec<trace::Line> = lines.into_iter().rev().take(400).collect();
        let paused = self.ws.trace_frozen.is_some();
        let hide_obd = self.ws.trace_hide_obd;
        let filter = self.ws.trace_filter.read(cx).value().trim().to_lowercase();
        let addr_chip = |id: &'static str, text: &'static str, cx: &mut Context<Self>| {
            let on = filter == text.to_lowercase();
            self.chip_button(id, text, on, cx, move |this, window, cx| {
                this.ws.trace_filter.update(cx, |s, cx| s.set_value(if on { "" } else { text }, window, cx));
            })
        };
        let dir_chip = |id: &'static str, text: &'static str, dir: bool, cx: &mut Context<Self>| {
            let on = self.ui.trace_dir == Some(dir);
            self.chip_button(id, text, on, cx, move |this, _, _| this.ui.trace_dir = if on { None } else { Some(dir) })
        };
        let g = self.lay().gutter();
        v_flex()
            .size_full()
            .px(px(g))
            .pt_4()
            .pb_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(div().w(px(260.)).child(Input::new(&self.ws.trace_filter).when(self.lay().touch(), |i| i.h(px(ui::TAP)))))
                    .child(addr_chip("tc-7e0", "7E0", cx))
                    .child(addr_chip("tc-7e8", "7E8", cx))
                    .child(dir_chip("tc-tx", "TX", true, cx))
                    .child(dir_chip("tc-rx", "RX", false, cx)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(self.seg("trace-seg", [(tr("OBD and UDS"), !hide_obd, |t, _, _| t.ws.trace_hide_obd = false), (tr("UDS only"), hide_obd, |t, _, _| t.ws.trace_hide_obd = true)], cx))
                    .child(self.sized(Button::new("trace-pause").label(tr(if paused { "Resume" } else { "Pause" }))).on_click(cx.listener(|this, _, _, cx| {
                        this.ws.trace_frozen = if this.ws.trace_frozen.is_some() { None } else { Some(trace::latest()) };
                        cx.notify();
                    })))
                    .child(self.sized(Button::new("trace-clear").label(tr("Clear"))).on_click(cx.listener(|this, _, _, cx| {
                        this.ws.trace_from = trace::latest();
                        cx.notify();
                    })))
                    .child(self.sized(Button::new("trace-copy").label(tr("Copy"))).on_click(cx.listener(|this, _, _, cx| {
                        let text = trace::to_text(&this.trace_lines(cx));
                        this.copy_text(text, "the trace", cx);
                    })))
                    .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(format!("{total} {}{}", tr("line(s)"), if total > 400 { format!(", {}", tr("newest 400 shown")) } else { String::new() }))),
            )
            .child(
                v_flex()
                    .id("trace-list")
                    .flex_1()
                    .min_h(px(if self.lay() == Layout::Portrait { 320. } else { 160. }))
                    .overflow_y_scroll()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .p_2()
                    .gap_0p5()
                    .font_family("monospace")
                    .when(shown.is_empty(), |d| d.child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("No frames yet. Everything the app sends to the car, and each answer, is listed here (TX = sent, RX = received)."))))
                    .children(shown.into_iter().map(|l| {
                        h_flex()
                            .gap_3()
                            .items_start()
                            .child(div().w(px(64.)).flex_none().text_color(theme.muted_foreground).child(format!("{:.3}", l.ms as f64 / 1000.0)))
                            .child(div().flex_1().min_w_0().text_color(if l.is_tx() { theme.blue } else { theme.foreground }).child(l.text))
                    })),
            )
            .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("TX = sent, RX = received. A recording saves its trace beside the CSV.")))
    }
}
