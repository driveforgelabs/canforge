//! Verifying a repair: what the car showed before codes were cleared, what it shows now, and
//! whether the repair held. After a clear the app re-reads the codes now and then while the car is
//! driven, and says when every monitor has finished and nothing came back.
//!
//! This is a child module of `main`, so it can use the `Diagnostics` view's state directly.

use super::*;

use crate::i18n::tr;
use crate::workshop::unix_now;
use crate::ui::{self, Tone, r};

/// How often the codes are read again while a repair is being verified.
const REREAD: Duration = Duration::from_secs(30);

/// What the car showed when the codes were cleared.
#[derive(Clone)]
pub struct Verify {
    pub at: u64,
    pub codes_before: Vec<String>,
    /// (ready, supported) monitors before the clear.
    pub ready_before: (usize, usize),
    /// The first read after the clear has arrived, so "now" means after.
    pub cleared: bool,
    pub last_read: std::time::Instant,
    /// The result has been announced (toast, notification, visit log).
    pub announced: bool,
    pub dismissed: bool,
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// These cleared codes are back.
    CameBack(Vec<String>),
    /// Nothing came back yet, but monitors are still to finish.
    Waiting { ready: usize, total: usize },
    /// Nothing came back and every monitor finished.
    Verified,
    /// Nothing came back; the car reports no readiness monitors to wait for.
    NoMonitors,
}

/// Whether the repair held, from the codes before and now and the monitors now.
pub fn verdict(before: &[String], now: &[String], ready: (usize, usize)) -> Verdict {
    let back: Vec<String> = before.iter().filter(|c| now.contains(c)).cloned().collect();
    if !back.is_empty() {
        Verdict::CameBack(back)
    } else if ready.1 == 0 {
        Verdict::NoMonitors
    } else if ready.0 < ready.1 {
        Verdict::Waiting { ready: ready.0, total: ready.1 }
    } else {
        Verdict::Verified
    }
}

fn notify(title: &str, body: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["--app-name=canforge", title, body])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

impl Diagnostics {
    pub fn monitor_counts(&self) -> (usize, usize) {
        self.readiness.as_ref().map_or((0, 0), |r| (r.monitors.iter().filter(|m| m.supported && m.complete).count(), r.monitors.iter().filter(|m| m.supported).count()))
    }

    /// Remembers what the car shows now, as the "before" of a repair.
    pub fn start_verify(&mut self, cx: &App) {
        self.ws.verify = Some(Verify {
            at: unix_now(),
            codes_before: self.obd_codes(cx),
            ready_before: self.monitor_counts(),
            cleared: false,
            last_read: std::time::Instant::now(),
            announced: false,
            dismissed: false,
        });
    }

    pub fn current_verdict(&self, cx: &App) -> Option<(Verdict, &Verify)> {
        let v = self.ws.verify.as_ref().filter(|v| v.cleared && !v.dismissed)?;
        Some((verdict(&v.codes_before, &self.obd_codes(cx), self.monitor_counts()), v))
    }

    /// Called every tick: reads the codes again while monitors are finishing, and announces the
    /// result once. Returns whether anything changed.
    pub fn verify_tick(&mut self, cx: &App) -> bool {
        let Some((verdict, v)) = self.current_verdict(cx) else { return false };
        let online = matches!(self.link, Link::Online { .. });
        match verdict {
            Verdict::Waiting { .. } | Verdict::NoMonitors if online && !self.reading_codes && v.last_read.elapsed() >= REREAD => {
                self.start_read_codes();
                if let Some(v) = &mut self.ws.verify {
                    v.last_read = std::time::Instant::now();
                }
                false
            }
            Verdict::Verified | Verdict::CameBack(_) if !v.announced => {
                let (title, body) = match &verdict {
                    Verdict::CameBack(c) => (tr("A cleared code came back").to_string(), c.join(", ")),
                    _ => (tr("Repair verified").to_string(), tr("No code came back and every monitor is ready.").to_string()),
                };
                let text = format!("{title}: {body}");
                notify(&title, &body);
                self.toast = Some(text.clone());
                if let Some(v) = &mut self.ws.verify {
                    v.announced = true;
                }
                self.record_action(text, cx);
                true
            }
            _ => false,
        }
    }

    /// The card that follows a clear: before and after, what is still to finish, and the verdict.
    pub fn render_verify_card(&self, with_codes_button: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let text = self.ws.clear_result.clone();
        let (verdict, v) = self.current_verdict(cx)?;
        let theme = cx.theme().clone();
        let now_codes = self.obd_codes(cx);
        let (ready, total) = self.monitor_counts();
        let (tone, title) = match &verdict {
            Verdict::CameBack(_) => (Tone::Bad, tr("A cleared code came back").to_string()),
            Verdict::Waiting { ready, total } => (Tone::Warn, tr("Drive cycle in progress: {n} of {m} monitors ready").replace("{n}", &ready.to_string()).replace("{m}", &total.to_string())),
            Verdict::Verified => (Tone::Ok, tr("Repair verified").to_string()),
            Verdict::NoMonitors => (Tone::Ok, tr("Nothing came back").to_string()),
        };
        let remaining: Vec<&'static str> = self.readiness.as_ref().map(|r| r.incomplete().map(|m| m.name).collect()).unwrap_or_default();
        let ago = v.last_read.elapsed().as_secs();
        let done = matches!(verdict, Verdict::Verified | Verdict::CameBack(_));
        let row = |label: String, value: String| h_flex().gap_2().child(div().w(px(150.)).flex_none().text_color(theme.muted_foreground).child(label)).child(div().font_weight(FontWeight::SEMIBOLD).child(value));
        Some(
            ui::focus_card(&theme)
                .child(ui::verdict_at(tone, title, 20., 18., &theme))
                .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(format!("{} {}", tr("Codes cleared"), report::stamp(v.at))))
                .children(text.map(|t| div().text_color(theme.muted_foreground).child(t)))
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(row(tr("Codes").to_string(), format!("{} → {}", v.codes_before.len(), now_codes.len())))
                        .child(row(tr("Monitors ready").to_string(), format!("{}/{} → {}/{}", v.ready_before.0, v.ready_before.1, ready, total)))
                        .when_some(matches!(verdict, Verdict::CameBack(_)).then(|| verdict_codes(&verdict)), |d, back| d.child(row(tr("Came back").to_string(), back))),
                )
                .when(!remaining.is_empty() && !done, |d| {
                    d.child(
                        v_flex()
                            .gap_1()
                            .child(ui::section_title(tr("Still to finish"), &theme))
                            .children(remaining.iter().map(|n| div().text_size(r(12.)).child(format!("• {n}: {}", crate::readiness::monitor_hint(n))))),
                    )
                })
                .child(div().text_size(r(12.)).text_color(theme.muted_foreground).child(if done {
                    tr("The result is saved in this visit's history and on the report.").to_string()
                } else {
                    tr("Drive the car through a drive cycle. The codes are read again every 30 s; last read {n} s ago.").replace("{n}", &ago.to_string())
                }))
                .child(
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(self.sized(Button::new("verify-reread").primary().label(tr("Read codes now"))).disabled(!matches!(self.link, Link::Online { .. }) || self.reading_codes).on_click(cx.listener(|this, _, _, cx| {
                            this.start_read_codes();
                            if let Some(v) = &mut this.ws.verify {
                                v.last_read = std::time::Instant::now();
                            }
                            cx.notify();
                        })))
                        .child(self.sized(Button::new("verify-ready").label(tr("Open readiness"))).on_click(cx.listener(|this, _, _, cx| this.go(Page::Readiness, cx))))
                        .when(with_codes_button, |d| d.child(self.sized(Button::new("verify-codes").label(tr("Fault codes"))).on_click(cx.listener(|this, _, _, cx| this.go(Page::Codes, cx)))))
                        .when(done, |d| {
                            d.child(self.sized(Button::new("verify-dismiss").ghost().label(tr("Dismiss"))).on_click(cx.listener(|this, _, _, cx| {
                                if let Some(v) = &mut this.ws.verify {
                                    v.dismissed = true;
                                }
                                cx.notify();
                            })))
                        }),
                )
                .into_any_element(),
        )
    }
}

fn verdict_codes(v: &Verdict) -> String {
    match v {
        Verdict::CameBack(c) => c.join(", "),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{verdict, Verdict};

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn a_code_that_returns_is_reported_first() {
        assert_eq!(verdict(&s(&["P0113", "P0101"]), &s(&["P0101"]), (8, 8)), Verdict::CameBack(s(&["P0101"])));
    }

    #[test]
    fn waits_until_every_monitor_finished() {
        assert_eq!(verdict(&s(&["P0113"]), &[], (2, 8)), Verdict::Waiting { ready: 2, total: 8 });
        assert_eq!(verdict(&s(&["P0113"]), &[], (8, 8)), Verdict::Verified);
    }

    #[test]
    fn a_car_without_monitors_has_nothing_to_wait_for() {
        assert_eq!(verdict(&s(&["P0113"]), &[], (0, 0)), Verdict::NoMonitors);
    }

    #[test]
    fn other_codes_do_not_count_as_coming_back() {
        assert_eq!(verdict(&s(&["P0113"]), &s(&["P0420"]), (8, 8)), Verdict::Verified);
    }
}
