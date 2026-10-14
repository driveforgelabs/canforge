//! The Modules page: one expandable card per module, faults first.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{self, Tone, r};

impl Diagnostics {
    pub fn render_modules(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let make = self.make.clone();
        let mut order: Vec<&ModuleView> = self.modules.iter().collect();
        // modules with faults first, then by request ID
        order.sort_by_key(|m| (m.dtcs.is_empty(), m.tx_id));
        let cards: Vec<AnyElement> = order.into_iter().map(|m| self.module_card(m, make.as_deref(), cx)).collect();
        self.page_box("module-list")
            .when(self.modules.is_empty(), |d| {
                d.child(div().text_color(theme.muted_foreground).child(tr(if self.scanning {
                    "Scanning the vehicle's modules…"
                } else {
                    "No modules yet. Press Scan modules (read-only: it only listens and asks each module who it is)."
                })))
            })
            .children(cards)
    }

    fn module_card(&self, m: &ModuleView, make: Option<&str>, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tx_id = m.tx_id;
        let title = self.module_title(tx_id);
        let open = self.expanded_modules.contains(&tx_id);
        let row = |label: String, value: String, color: Hsla| {
            h_flex()
                .gap_2()
                .items_start()
                .child(div().w(px(150.)).flex_none().text_color(theme.muted_foreground).child(label))
                .child(div().flex_1().min_w_0().text_color(color).child(value))
        };
        let db = dtcdb::get();
        let (tone, badge) = match (m.dtcs.len(), &m.dtc_note) {
            (0, Some(_)) => (Tone::Muted, tr("Fault list unavailable").to_string()),
            (0, None) => (Tone::Ok, tr("No faults").to_string()),
            (1, _) => (Tone::Bad, tr("1 fault").to_string()),
            (n, _) => (Tone::Bad, format!("{n} {}", tr("faults"))),
        };
        let first_code = m.dtcs.first().map(|d| d.code.get(..5).unwrap_or(&d.code).to_string());
        v_flex()
            .gap_2p5()
            .px_4()
            .py_3p5()
            .border_1()
            .border_color(if m.dtcs.is_empty() { theme.border } else { theme.red })
            .rounded(px(10.))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_3()
                    .items_center()
                    .child(div().text_size(r(16.)).font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(ui::verdict(tone, badge, 16., &theme))
                    .child(div().flex_1())
                    .child(self.sized(Button::new(SharedString::from(format!("details-{tx_id}"))).ghost().label(tr(if open { "Hide details" } else { "Details" }))).on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded_modules.remove(&tx_id) {
                            this.expanded_modules.insert(tx_id);
                        }
                        cx.notify();
                    }))),
            )
            .children(m.dtcs.iter().map(|d| {
                let code = d.code.get(..5).unwrap_or(&d.code);
                let info = db.lookup(code, make);
                let description = match (&info.scope, self.own_description(code)) {
                    (dtcdb::Scope::Unknown, Some(own)) => own.to_string(),
                    _ => info.description,
                };
                let status = if d.status == "confirmed" { "stored" } else { "pending" };
                let color = fault_color(status, &theme);
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(div().w(px(110.)).flex_none().font_family("monospace").font_weight(FontWeight::SEMIBOLD).text_color(color).child(d.code.clone()))
                    .child(div().flex_1().child(description))
                    .child(div().text_size(r(12.)).font_weight(FontWeight::SEMIBOLD).text_color(color).child(tr(d.status)))
            }))
            .when(open, |this| {
                this.child(
                    v_flex()
                        .gap_1p5()
                        .pt_3()
                        .border_t_1()
                        .border_color(theme.border)
                        .child(row(tr("Address").to_string(), format!("{:03X} → {:03X}", m.tx_id, m.rx_id), theme.foreground).font_family("monospace"))
                        .children(m.identity.iter().filter(|(k, _)| k != "System name").map(|(k, v)| row(k.clone(), v.clone(), theme.foreground)))
                        .children(m.dtc_note.iter().map(|n| row(tr("Trouble codes").to_string(), n.clone(), theme.muted_foreground)))
                        .child(row(tr("Supported requests").to_string(), m.services.iter().map(|(n, s)| format!("{n}: {s}")).collect::<Vec<_>>().join(" · "), theme.muted_foreground))
                        .child(
                            h_flex()
                                .flex_wrap()
                                .gap_2()
                                .pt_1p5()
                                .children(first_code.map(|code| self.sized(Button::new(SharedString::from(format!("tocodes-{tx_id}"))).label(tr("View fault codes"))).on_click(cx.listener(move |this, _, _, cx| this.open_code(&code, cx)))))
                                .child(self.sized(Button::new(SharedString::from(format!("tofns-{tx_id}"))).label(tr("Open functions"))).on_click(cx.listener(move |this, _, _, cx| {
                                    this.fn_module = Some(tx_id);
                                    this.go(Page::Functions, cx);
                                })))
                                .child(self.sized(Button::new(SharedString::from(format!("sweep-{tx_id}"))).label(tr("Read this module's data IDs"))).on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = this.backend.cmds.send(Cmd::SweepDids { tx_id });
                                    cx.notify();
                                }))),
                        )
                        .children(self.dids.get(&tx_id).map(|lines| {
                            v_flex()
                                .mt_1()
                                .gap_0p5()
                                .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(format!("{} F100–F1FF: {} {}", tr("Data identifiers"), lines.len(), tr("found"))))
                                .children(lines.iter().map(|l| {
                                    let label = if l.name.is_empty() { format!("{:04X}", l.did) } else { format!("{:04X}  {}", l.did, l.name) };
                                    row(label, l.value.clone(), theme.foreground)
                                }))
                        })),
                )
            })
            .into_any_element()
    }
}
