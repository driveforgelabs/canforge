//! The History page: this visit's work, earlier visits of the vehicle, and the backups taken before
//! functions ran, with restore.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::ui::{self, Tone, r};

impl Diagnostics {
    /// Where this visit's photos are kept.
    fn photo_dir(&self) -> Option<std::path::PathBuf> {
        dtcdb::data_dir().map(|d| d.join("attachments").join(self.vehicle_key()).join(self.ws.started.to_string()))
    }

    pub fn photo_paths(&self) -> Vec<std::path::PathBuf> {
        self.photo_dir().map(|d| self.ws.photos.iter().map(|n| d.join(n)).collect()).unwrap_or_default()
    }

    /// Asks which pictures to attach to this visit. The file chooser is the system's own.
    fn pick_photos(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: Some(tr("Attach").into()) });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                this.update(cx, |this, cx| this.attach_photos(paths, cx)).ok();
            }
        })
        .detach();
    }

    /// Copies the pictures into this visit's folder, so they stay with it if the originals move.
    fn attach_photos(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let Some(dir) = self.photo_dir() else {
            self.toast = Some("No data directory available (set CANFORGE_DATA_DIR).".into());
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.toast = Some(format!("{}: {e}", dir.display()));
            return;
        }
        let mut added = 0;
        for p in paths {
            let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
            let mut target = name.clone();
            let mut n = 1;
            while dir.join(&target).exists() {
                target = format!("{n}-{name}");
                n += 1;
            }
            if std::fs::copy(&p, dir.join(&target)).is_ok() {
                self.ws.photos.push(target);
                added += 1;
            }
        }
        self.toast = Some(tr("{n} photo(s) attached to this visit.").replace("{n}", &added.to_string()));
        self.touch_visit(cx);
        cx.notify();
    }

    fn remove_photo(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(dir) = self.photo_dir() {
            let _ = std::fs::remove_file(dir.join(name));
        }
        self.ws.photos.retain(|p| p != name);
        self.touch_visit(cx);
        cx.notify();
    }

    pub fn render_history(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let gate = self.write_gate();
        let muted = |text: String| div().text_size(r(12.)).text_color(theme.muted_foreground).child(text);
        let mut page = self.page_box("history");
        if let Some(since) = self.render_since_last(cx) {
            page = page.child(since);
        }
        let photos = self.photo_paths();
        let field = |label: &'static str, input: &Entity<InputState>| {
            v_flex().gap_1().child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr(label))).child(Input::new(input).when(self.lay().touch(), |i| i.h(px(ui::TAP))))
        };
        page = page.child(
            ui::card(&theme)
                .gap_3()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{} {}", tr("This visit · started"), report::stamp(self.ws.started))))
                .when(self.ws.actions.is_empty() && self.ws.checks_done.is_empty(), |d| d.child(muted(tr("Nothing has been done to the car yet. Clearing codes, running functions and restoring backups are listed here.").into())))
                .children(self.ws.actions.iter().map(|a| div().child(format!("• {a}"))))
                .children(self.ws.checks_done.iter().map(|c| div().child(format!("✓ {}: {c}", tr("Checked")))))
                .child(field("Note on this visit (for the shop)", &self.ws.note_input))
                .child(field("Recommendation to the customer", &self.ws.reco_input))
                .child(field("Estimate", &self.ws.est_input))
                .child(
                    v_flex()
                        .gap_2()
                        .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(tr("Photos")))
                        .when(!photos.is_empty(), |d| {
                            d.child(h_flex().flex_wrap().gap_2().children(photos.iter().zip(self.ws.photos.iter()).enumerate().map(|(i, (path, name))| {
                                let name = name.clone();
                                v_flex()
                                    .gap_1()
                                    .child(img(path.clone()).h(px(96.)).rounded(theme.radius))
                                    .child(self.sized(Button::new(SharedString::from(format!("photo-rm-{i}"))).ghost().small().label(tr("Remove"))).on_click(cx.listener(move |this, _, _, cx| this.remove_photo(&name, cx))))
                            })))
                        })
                        .child(h_flex().child(self.sized(Button::new("photo-add").label(tr("Attach photos"))).on_click(cx.listener(|this, _, _, cx| this.pick_photos(cx)))))
                        .child(muted(tr("Photos are copied into this visit's folder and printed at the end of the report.").into())),
                ),
        );
        page = page.child(div().text_size(r(16.)).font_weight(FontWeight::SEMIBOLD).child(tr("Earlier visits")));
        if self.ws.history.is_empty() {
            page = page.child(muted(tr("No earlier visits are saved for this vehicle. A visit is saved automatically once the modules have been scanned.").into()));
        }
        for (i, v) in self.ws.history.iter().enumerate().rev() {
            let before = i.checked_sub(1).and_then(|p| self.ws.history.get(p));
            let d = before.map(|b| history::diff(b, v));
            page = page.child(
                ui::card(&theme)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .justify_between()
                            .gap_2()
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(report::stamp(v.unix_time)))
                            .child(ui::verdict(if v.mil_on { Tone::Bad } else { Tone::Ok }, tr(if v.mil_on { "Check-engine light ON" } else { "Check-engine light off" }), 16., &theme).text_size(r(12.))),
                    )
                    .child(div().child(if v.codes.is_empty() { tr("No trouble codes").to_string() } else { format!("{}: {}", tr("Codes"), v.codes.join(", ")) }))
                    .child(muted(format!(
                        "{} {}{}",
                        tr("Battery"),
                        v.battery_volts.map_or(tr("not read").to_string(), |b| format!("{b:.1} V")),
                        if v.not_ready.is_empty() { String::new() } else { format!(" · {}: {}", tr("not ready"), v.not_ready.join(", ")) }
                    )))
                    .children(d.filter(|d| !d.new.is_empty() || !d.resolved.is_empty()).map(|d| {
                        muted(format!(
                            "{}: {} {} · {} {}",
                            tr("Compared with the visit before"),
                            tr("new"),
                            if d.new.is_empty() { tr("none").to_string() } else { d.new.join(", ") },
                            tr("gone"),
                            if d.resolved.is_empty() { tr("none").to_string() } else { d.resolved.join(", ") }
                        ))
                    }))
                    .children(v.actions.iter().map(|a| div().text_size(r(12.)).child(format!("• {a}")))),
            );
        }
        page = page.child(div().text_size(r(16.)).font_weight(FontWeight::SEMIBOLD).child(tr("Backups")));
        page = page.child(muted(tr("Taken automatically before a function runs. A restore writes back only the values that changed, and the module may refuse (many need security access).").into()));
        page = page.children(self.ws.restore_msg.clone().map(|m| div().font_weight(FontWeight::SEMIBOLD).child(m)));
        if self.ws.backups.is_empty() {
            page = page.child(muted(tr("No backups yet for this vehicle.").into()));
        }
        for (i, b) in self.ws.backups.iter().enumerate() {
            let confirming = self.ws.confirm_restore == Some(i);
            let (tx_id, values) = (b.tx_id, b.values());
            let online = matches!(self.link, Link::Online { .. });
            let (tone, text, blocked) = match &gate {
                health::Gate::Ok(t) => (Tone::Ok, t.to_string(), false),
                health::Gate::Warn(t) => (Tone::Warn, t.to_string(), false),
                health::Gate::Block(t) => (Tone::Bad, t.to_string(), true),
            };
            page = page.child(
                ui::card(&theme)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .child(
                                v_flex()
                                    .child(div().font_weight(FontWeight::SEMIBOLD).child(format!("{} · {}", b.module, report::stamp(b.unix_time))))
                                    .child(muted(format!("{}: {} · {} {}", tr("Before"), b.before, b.dids.len(), tr("value(s)")))),
                            )
                            .when(!confirming, |d| {
                                d.child(self.sized(Button::new(SharedString::from(format!("restore-{i}"))).label(format!("{}…", tr("Restore")))).disabled(!online).on_click(cx.listener(move |this, _, _, cx| {
                                    this.ws.confirm_restore = Some(i);
                                    cx.notify();
                                })))
                            }),
                    )
                    .when(confirming, |d| {
                        d.child(div().child(tr("This writes the saved values back into the module. Only values that differ are written.")))
                            .child(ui::verdict(tone, text, 18., &theme).font_weight(FontWeight::NORMAL).items_start())
                            .child(
                                h_flex()
                                    .gap_2()
                                    .justify_end()
                                    .flex_wrap()
                                    .child(self.sized(Button::new(SharedString::from(format!("restore-cancel-{i}"))).label(tr("Cancel"))).on_click(cx.listener(|this, _, _, cx| {
                                        this.ws.confirm_restore = None;
                                        cx.notify();
                                    })))
                                    .child(self.sized(Button::new(SharedString::from(format!("restore-go-{i}"))).danger().label(tr("Restore"))).disabled(blocked || self.engine_running() == Some(true) || !online).on_click(cx.listener(move |this, _, _, cx| {
                                        this.ws.confirm_restore = None;
                                        this.ws.restore_msg = Some("Restoring…".into());
                                        let _ = this.backend.cmds.send(Cmd::WriteDids { tx_id, values: values.clone() });
                                        cx.notify();
                                    }))),
                            )
                    }),
            );
        }
        page
    }
}
