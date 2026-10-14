//! The Overview page: what is wrong and the one thing to do about it, what changed since the last
//! visit, the faults with their live evidence, and the key numbers as tiles that open their page.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{self, Layout, Tone, r};

impl Diagnostics {
    /// The code to start with: one that is new, else one with live evidence, else a stored one.
    fn first_fault<'a>(&self, faults: &'a [report::Fault]) -> Option<&'a report::Fault> {
        faults
            .iter()
            .find(|f| self.is_new_code(&f.code))
            .or_else(|| faults.iter().find(|f| self.evidence(&f.code).is_some()))
            .or_else(|| faults.iter().find(|f| f.status != "pending"))
            .or_else(|| faults.first())
    }

    /// The status banner: the verdict in one sentence and the primary action.
    fn render_banner(&self, faults: &[report::Fault], cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let online = matches!(self.link, Link::Online { .. });
        let modules: BTreeSet<&str> = faults.iter().map(|f| f.module.as_str()).collect();
        // (tone, title, detail, button label, action)
        type Act = Box<dyn Fn(&mut Diagnostics, &mut Context<Diagnostics>)>;
        let (tone, title, detail, button, act): (Tone, String, String, String, Act) = match self.first_fault(faults) {
            Some(f) => {
                let code = f.code.clone();
                let evidence = self.evidence(&f.code).filter(|e| self.readings.contains_key(&e.key));
                let title = tr("{n} fault(s) in {m} module(s). Start with {code}.")
                    .replace("{n}", &faults.len().to_string())
                    .replace("{m}", &modules.len().to_string())
                    .replace("{code}", &f.code);
                let (sev, _) = checks::severity(&f.code);
                let detail = match evidence {
                    Some(e) => format!("{}: {}. {}", tr(sev.label()), f.description, e.long),
                    None => format!("{}: {}", tr(sev.label()), f.description),
                };
                (Tone::Bad, title, detail, format!("{} {}", tr("Open"), f.code), Box::new(move |t: &mut Diagnostics, cx: &mut Context<Diagnostics>| t.open_code(&code, cx)))
            }
            None if self.modules.is_empty() || !online => (
                Tone::Warn,
                tr("No codes in the engine ECU yet.").into(),
                tr("Scan all modules to check ABS, airbag, body and the rest.").into(),
                tr("Scan all modules").into(),
                Box::new(|t: &mut Diagnostics, cx: &mut Context<Diagnostics>| {
                    t.start_read_codes();
                    t.start_scan();
                    cx.notify();
                }),
            ),
            None => (
                Tone::Ok,
                tr("No trouble codes in any module.").into(),
                tr("Check that the readiness monitors are finished before an inspection.").into(),
                tr("Open readiness").into(),
                Box::new(|t: &mut Diagnostics, cx: &mut Context<Diagnostics>| t.go(Page::Readiness, cx)),
            ),
        };
        let color = tone.color(&theme);
        let portrait = self.lay() == Layout::Portrait;
        h_flex()
            .gap_4()
            .px_5()
            .py_4()
            .items_center()
            .flex_wrap()
            .border_1()
            .border_color(color)
            .rounded(px(10.))
            .child(tone.icon().size(px(28.)).text_color(color))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(240.))
                    .child(div().text_size(r(18.)).line_height(r(26.)).font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().text_color(theme.muted_foreground).child(detail)),
            )
            .child(div().when(portrait, |d| d.w_full()).child(self.sized(Button::new("banner-act").primary().label(button).when(portrait, |b| b.w_full())).on_click(cx.listener(move |this, _, _, cx| act(this, cx)))))
            .into_any_element()
    }

    /// New, gone and unchanged codes since the previous visit.
    pub fn render_since_last(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let last = self.ws.history.last()?;
        let d = history::diff(last, &self.current_visit(cx));
        let list = |v: &[String]| if v.is_empty() { tr("none").to_string() } else { v.join(", ") };
        Some(
            ui::card(&theme)
                .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{} ({})", tr("Since the last visit"), report::stamp(last.unix_time))))
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_x_6()
                        .gap_y_1()
                        .child(div().text_color(if d.new.is_empty() { theme.foreground } else { theme.red }).child(format!("{}: {}", tr("New since last visit"), list(&d.new))))
                        .child(div().text_color(if d.resolved.is_empty() { theme.foreground } else { theme.green }).child(format!("{}: {}", tr("Gone since last visit"), list(&d.resolved))))
                        .child(div().child(format!("{}: {}", tr("Still there"), list(&d.unchanged)))),
                )
                .into_any_element(),
        )
    }

    fn kpi(&self, id: &'static str, label: &str, value: String, unit: &str, note: String, tone: Tone, cx: &mut Context<Self>, go: impl Fn(&mut Self, &mut Context<Self>) + 'static) -> AnyElement {
        let theme = cx.theme().clone();
        v_flex()
            .id(id)
            .flex_1()
            .min_w(px(210.))
            .min_h(px(44.))
            .gap_1()
            .px_4()
            .py(px(14.))
            .border_1()
            .border_color(theme.border)
            .rounded(px(10.))
            .cursor_pointer()
            .hover(|d| d.bg(theme.accent))
            .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(label.to_string()))
            .child(h_flex().gap_1().items_baseline().child(div().text_size(r(26.)).line_height(r(32.)).font_weight(FontWeight::SEMIBOLD).child(value)).child(div().text_color(theme.muted_foreground).child(unit.to_string())))
            .child(div().text_size(r(12.)).font_weight(FontWeight::SEMIBOLD).text_color(tone.color(&theme)).child(note))
            .on_click(cx.listener(move |this, _, _, cx| go(this, cx)))
            .into_any_element()
    }

    pub fn render_overview(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let faults = self.faults(cx);
        let pending = faults.iter().filter(|f| f.status == "pending").count();
        let reading = |key: &str| self.readings.get(key).copied();
        let (ready, supported) = self.readiness.as_ref().map_or((0, 0), |r| (r.monitors.iter().filter(|m| m.supported && m.complete).count(), r.monitors.iter().filter(|m| m.supported).count()));
        let battery = reading("pid:42").map(|v| health::battery(v, self.rpm()));
        let level_tone = |l: health::Level| match l {
            health::Level::Good => Tone::Ok,
            health::Level::Warn => Tone::Warn,
            health::Level::Bad => Tone::Bad,
            health::Level::Unknown => Tone::Muted,
        };

        let rows = faults.iter().take(6).map(|f| {
            let color = fault_color(&f.status, &theme);
            let code = f.code.clone();
            let evidence = self.evidence(&f.code).filter(|e| self.readings.contains_key(&e.key) || e.key == "pid:10");
            self.row_button(format!("orow-{}-{}", f.code, f.module), cx)
                .gap_3()
                .px_2()
                .py_2()
                .border_t_1()
                .border_color(theme.border.opacity(0.7))
                .rounded(px(6.))
                .child(div().w(px(64.)).flex_none().font_weight(FontWeight::SEMIBOLD).text_color(color).child(f.code.clone()))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().child(f.description.clone()))
                        .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(f.module.clone()))
                        .children(evidence.map(|e| div().text_size(r(12.)).text_color(theme.info).child(e.short))),
                )
                .child(ui::fault_status(&f.status, 16., &theme))
                .on_click(cx.listener(move |this, _, _, cx| this.open_code(&code, cx)))
        }).collect::<Vec<_>>();
        let more = faults.len().saturating_sub(6);

        let kpis = h_flex()
            .flex_wrap()
            .gap_3()
            .child(self.kpi(
                "kpi-codes",
                tr("Trouble codes"),
                faults.len().to_string(),
                "",
                if faults.is_empty() {
                    tr(if self.modules.is_empty() { "None in the engine ECU (modules not scanned)" } else { "None in any module" }).to_string()
                } else {
                    tr("{s} stored, {p} pending").replace("{s}", &(faults.len() - pending).to_string()).replace("{p}", &pending.to_string())
                },
                if faults.is_empty() { if self.modules.is_empty() { Tone::Muted } else { Tone::Ok } } else { Tone::Bad },
                cx,
                |t, cx| t.go(Page::Codes, cx),
            ))
            .child(self.kpi(
                "kpi-battery",
                tr("Battery"),
                reading("pid:42").map_or("—".into(), |v| units::localise(format!("{v:.1}"))),
                "V",
                battery.map_or(tr("Not reported").to_string(), |(t, _)| t.to_string()),
                battery.map_or(Tone::Muted, |(_, l)| level_tone(l)),
                cx,
                |t, cx| t.watch_signal("pid:42", cx),
            ))
            .child(self.kpi(
                "kpi-coolant",
                tr("Coolant temperature"),
                reading("pid:05").map_or("—".into(), |v| units::show(v, 120., "°C").0),
                &units::label("°C"),
                reading("pid:05").map_or(tr("Not reported"), |v| if v < 70.0 { tr("Warming up") } else if v > 110.0 { tr("Overheating") } else { tr("At operating temperature") }).to_string(),
                reading("pid:05").map_or(Tone::Muted, |v| if v < 70.0 { Tone::Warn } else if v > 110.0 { Tone::Bad } else { Tone::Ok }),
                cx,
                |t, cx| t.watch_signal("pid:05", cx),
            ))
            .child(self.kpi(
                "kpi-ready",
                tr("Readiness monitors"),
                if supported == 0 { "—".into() } else { ready.to_string() },
                if supported == 0 { "" } else { tr("ready") },
                if supported == 0 { tr("Not read yet").to_string() } else if ready == supported { tr("Ready for inspection").to_string() } else { tr("{n} of {m} not ready").replace("{n}", &(supported - ready).to_string()).replace("{m}", &supported.to_string()) },
                if supported == 0 { Tone::Muted } else if ready == supported { Tone::Ok } else { Tone::Warn },
                cx,
                |t, cx| t.go(Page::Readiness, cx),
            ));

        let banner = self.render_banner(&faults, cx);
        let verify = self.render_verify_card(true, cx);
        let since = self.render_since_last(cx);
        self.page_box("overview")
            .when(self.scanning, |d| {
                d.child(ui::card(&theme).child(h_flex().gap_2().items_center().child(Icon::new(IconName::Loader).size_4()).child(div().font_weight(FontWeight::SEMIBOLD).child(tr("Scanning the modules…")))))
            })
            .child(banner)
            .children(verify)
            .children(since)
            .child(
                v_flex()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius_lg)
                    .p_4()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .pb_2()
                            .child(div().text_size(r(16.)).font_weight(FontWeight::SEMIBOLD).child(tr("Fault codes")))
                            .child(self.sized(Button::new("all-codes").ghost().small().label(if more > 0 { format!("{} {}", tr("View all"), faults.len()) } else { tr("View all").to_string() })).on_click(cx.listener(|this, _, _, cx| this.go(Page::Codes, cx)))),
                    )
                    .children(rows)
                    .when(faults.is_empty(), |d| {
                        d.child(div().py_2().text_color(theme.muted_foreground).child(tr(if self.scanning {
                            "Scanning the modules…"
                        } else if self.modules.is_empty() {
                            "No codes in the engine ECU. Press Scan all modules to check ABS, airbag, body and the rest."
                        } else {
                            "No stored or pending trouble codes in any module."
                        })))
                    }),
            )
            .child(kpis)
            .child(v_flex().gap_2().child(div().text_size(r(16.)).font_weight(FontWeight::SEMIBOLD).child(tr("Connection"))).child(self.render_connection(cx)))
    }
}
