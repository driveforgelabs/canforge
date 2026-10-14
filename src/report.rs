//! A one-page job report a mechanic can print (the browser's Print → Save as PDF) or attach to a
//! work order. Written next to the other local data; nothing is uploaded.

use std::path::{Path, PathBuf};

use crate::checks;
use crate::i18n::tr;

pub struct Fault {
    pub code: String,
    pub description: String,
    /// Module name and request ID, e.g. `ABS module · 760`.
    pub module: String,
    pub status: String,
}

/// What changed since the vehicle's previous visit.
pub struct SinceLast {
    pub when: String,
    pub new: Vec<String>,
    pub resolved: Vec<String>,
}

/// What the repair did to the car, from the snapshot taken when the codes were cleared.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Repair {
    pub codes_before: usize,
    pub codes_now: usize,
    pub ready_before: (usize, usize),
    pub ready_now: (usize, usize),
    pub came_back: Vec<String>,
}

impl Repair {
    /// One sentence: before and after, and whether it held.
    pub fn line(&self) -> String {
        let base = tr("{b} code(s) before the repair, {a} now. Monitors ready: {rb} of {tb} before, {ra} of {ta} now.")
            .replace("{b}", &self.codes_before.to_string())
            .replace("{a}", &self.codes_now.to_string())
            .replace("{rb}", &self.ready_before.0.to_string())
            .replace("{tb}", &self.ready_before.1.to_string())
            .replace("{ra}", &self.ready_now.0.to_string())
            .replace("{ta}", &self.ready_now.1.to_string());
        let verdict = if !self.came_back.is_empty() {
            tr("Came back: {codes}.").replace("{codes}", &self.came_back.join(", "))
        } else if self.ready_now.1 > 0 && self.ready_now.0 == self.ready_now.1 {
            tr("Repair verified: nothing came back and every monitor is ready.").to_string()
        } else {
            tr("Nothing has come back yet; some monitors are still to finish.").to_string()
        };
        format!("{base} {verdict}")
    }
}

/// What the mechanic adds to a report beyond what the car says.
#[derive(Clone, Debug, Default)]
pub struct Extra {
    /// The shop's name, printed at the top.
    pub shop: String,
    /// The shop's own note on the visit (technical report only).
    pub note: String,
    pub recommendation: String,
    pub estimate: String,
    /// Suggested checks that were ticked off.
    pub checks: Vec<String>,
    pub photos: Vec<PathBuf>,
    pub repair: Option<Repair>,
}

pub struct Report {
    pub extra: Extra,
    pub unix_time: u64,
    pub vehicle: String,
    pub vin: Option<String>,
    pub adapter: String,
    pub mil_on: bool,
    pub battery: Option<String>,
    pub faults: Vec<Fault>,
    /// True when the module scan did not run, so module faults may be missing.
    pub modules_scanned: bool,
    /// (monitor, ready)
    pub readiness: Vec<(String, bool)>,
    pub freeze: Vec<(String, String)>,
    pub freeze_code: Option<String>,
    /// What was done to the car during this visit, in order.
    pub actions: Vec<String>,
    pub since_last: Option<SinceLast>,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `2026-10-11 14:05 UTC` from a Unix time (civil-from-days, no time-zone data needed).
pub fn stamp(unix: u64) -> String {
    let (days, secs) = ((unix / 86_400) as i64, unix % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02} UTC", secs / 3600, secs % 3600 / 60)
}

pub fn html(r: &Report) -> String {
    let mut o = String::new();
    o.push_str(
        "<!doctype html><meta charset=utf-8><title>Vehicle diagnostic report</title><style>\
         body{font:16px/1.4 system-ui,sans-serif;max-width:800px;margin:2em auto;padding:0 1em;color:#111}\
         h1{margin:0}h2{margin:1.5em 0 .3em;border-bottom:2px solid #111}table{border-collapse:collapse;width:100%}\
         td,th{text-align:left;padding:.35em .5em;border-bottom:1px solid #ccc;vertical-align:top}\
         .bad{color:#b00020;font-weight:600}.ok{color:#1b6e2d;font-weight:600}.warn{color:#8a5a00;font-weight:600}\
         .muted{color:#555}@media print{body{margin:0}}</style>",
    );
    if !r.extra.shop.trim().is_empty() {
        o.push_str(&format!("<p class=muted><b>{}</b></p>", esc(r.extra.shop.trim())));
    }
    o.push_str(&format!("<h1>{}</h1>", esc(&r.vehicle)));
    o.push_str(&format!("<p class=muted>{}", esc(&stamp(r.unix_time))));
    if let Some(vin) = &r.vin {
        o.push_str(&format!(" · VIN {}", esc(vin)));
    }
    if !r.adapter.is_empty() {
        o.push_str(&format!(" · {}", esc(&r.adapter)));
    }
    o.push_str("</p>");
    o.push_str(&format!(
        "<p>Check-engine light: <span class={}>{}</span>{}</p>",
        if r.mil_on { "bad" } else { "ok" },
        if r.mil_on { "ON" } else { "off" },
        r.battery.as_ref().map(|b| format!(" · Battery: {}", esc(b))).unwrap_or_default()
    ));

    o.push_str("<h2>Fault codes</h2>");
    if r.faults.is_empty() {
        o.push_str(&format!("<p class=ok>No trouble codes found{}.</p>", if r.modules_scanned { " in any module" } else { " in the engine ECU (modules were not scanned)" }));
    } else {
        o.push_str(&format!("<table><tr><th>Code<th>Description<th>Module<th>Status<th>{}</tr>", tr("Urgency")));
        for f in &r.faults {
            let (sev, why) = checks::severity(&f.code);
            o.push_str(&format!(
                "<tr><td class=bad>{}<td>{}<td>{}<td>{}<td><b>{}</b><br><span class=muted>{}</span></tr>",
                esc(&f.code),
                esc(&f.description),
                esc(&f.module),
                esc(&f.status),
                tr(sev.label()),
                tr(why)
            ));
        }
        o.push_str("</table>");
        if !r.modules_scanned {
            o.push_str("<p class=warn>Modules were not scanned: faults in other modules may be missing.</p>");
        }
    }

    let hinted: Vec<(&Fault, &checks::Hint)> = r.faults.iter().filter_map(|f| checks::lookup(&f.code).map(|h| (f, h))).collect();
    if !hinted.is_empty() {
        o.push_str(&format!("<h2>{}</h2><p class=muted>{}</p>", tr("What to check"), tr("General guidance for this kind of code, not a diagnosis.")));
        for (f, h) in hinted {
            o.push_str(&format!("<p><b>{}</b><br>{}: {}<br>{}: {}</p>", esc(&f.code), tr("Likely causes"), esc(&h.causes.join("; ")), tr("What to check"), esc(&h.checks.join("; "))));
        }
    }

    if let Some(s) = &r.since_last {
        o.push_str(&format!("<h2>Since the last visit ({})</h2>", esc(&s.when)));
        o.push_str(&format!("<p>{}: <span class=bad>{}</span></p>", tr("New since last visit"), esc(&list_or_none(&s.new))));
        o.push_str(&format!("<p>{}: <span class=ok>{}</span></p>", tr("Gone since last visit"), esc(&list_or_none(&s.resolved))));
    }

    if !r.actions.is_empty() || !r.extra.checks.is_empty() || r.extra.repair.is_some() {
        o.push_str(&format!("<h2>{}</h2><ul>", tr("Work done")));
        for a in &r.actions {
            o.push_str(&format!("<li>{}</li>", esc(a)));
        }
        for c in &r.extra.checks {
            o.push_str(&format!("<li>{}: {}</li>", tr("Checked"), esc(c)));
        }
        if let Some(rep) = &r.extra.repair {
            o.push_str(&format!("<li><b>{}</b></li>", esc(&rep.line())));
        }
        o.push_str("</ul>");
    }
    if !r.extra.note.trim().is_empty() {
        o.push_str(&format!("<h2>{}</h2><p>{}</p>", tr("Shop note"), esc(r.extra.note.trim()).replace('\n', "<br>")));
    }
    if !r.extra.recommendation.trim().is_empty() || !r.extra.estimate.trim().is_empty() {
        o.push_str(&format!("<h2>{}</h2>", tr("Our recommendation")));
        if !r.extra.recommendation.trim().is_empty() {
            o.push_str(&format!("<p>{}</p>", esc(r.extra.recommendation.trim()).replace('\n', "<br>")));
        }
        if !r.extra.estimate.trim().is_empty() {
            o.push_str(&format!("<p><b>{}:</b> {}</p>", tr("Estimate"), esc(r.extra.estimate.trim())));
        }
    }
    if !r.extra.photos.is_empty() {
        o.push_str(&format!("<h2>{}</h2><p>", tr("Photos")));
        for p in &r.extra.photos {
            o.push_str(&format!("<img src=\"file://{}\" style=\"max-width:48%;margin:.2em\">", esc(&p.display().to_string())));
        }
        o.push_str("</p>");
    }

    if !r.freeze.is_empty() {
        o.push_str(&format!("<h2>Freeze frame{}</h2><table>", r.freeze_code.as_ref().map(|c| format!(" (when {} was set)", esc(c))).unwrap_or_default()));
        for (k, v) in &r.freeze {
            o.push_str(&format!("<tr><td>{}<td>{}</tr>", esc(k), esc(v)));
        }
        o.push_str("</table>");
    }

    if !r.readiness.is_empty() {
        o.push_str("<h2>Readiness monitors</h2><table>");
        for (name, ready) in &r.readiness {
            o.push_str(&format!(
                "<tr><td>{}<td class={}>{}</tr>",
                esc(name),
                if *ready { "ok" } else { "warn" },
                if *ready { "Ready" } else { "Not ready" }
            ));
        }
        o.push_str("</table>");
    }
    o.push_str("<p class=muted>Generated by canforge. Read-only snapshot taken when the report was saved.</p>");
    o
}

fn list_or_none(v: &[String]) -> String {
    if v.is_empty() { "none".into() } else { v.join(", ") }
}

fn ready_for_inspection(r: &Report) -> Option<bool> {
    (!r.readiness.is_empty()).then(|| r.readiness.iter().all(|(_, ready)| *ready))
}

/// The report as a PDF. The customer version leaves out what a car owner has no use for (module
/// addresses, VIN, adapter, freeze frame) and says what is wrong in plain words.
pub fn pdf(r: &Report, customer: bool) -> Vec<u8> {
    use crate::pdf::Line;
    let mut l = Vec::new();
    if !r.extra.shop.trim().is_empty() {
        l.push(Line { text: r.extra.shop.trim().to_string(), size: 10.0, bold: true, gap: 0.0, indent: 0.0, pinned: false, image: None });
    }
    l.push(Line::title(if customer { r.vehicle.clone() } else { tr("Vehicle diagnostic report").to_string() }));
    let mut who = stamp(r.unix_time);
    if !customer {
        who = format!("{} · {}", r.vehicle, who);
        if let Some(vin) = &r.vin {
            who.push_str(&format!(" · VIN {vin}"));
        }
        if !r.adapter.is_empty() {
            who.push_str(&format!(" · {}", r.adapter));
        }
    }
    l.push(Line::muted(who));
    l.push(Line::body(format!(
        "{}: {}{}",
        tr("Check-engine light"),
        tr(if r.mil_on { "ON" } else { "off" }),
        r.battery.as_ref().map(|b| format!(" · {}: {b}", tr("Battery"))).unwrap_or_default()
    )));
    if let Some(ok) = ready_for_inspection(r) {
        l.push(Line::body(if ok { tr("Ready for inspection") } else { tr("Not ready for inspection") }));
    }

    l.push(Line::heading(tr("Fault codes found")));
    if r.faults.is_empty() {
        l.push(Line::body(tr("No trouble codes found.")));
    }
    for f in &r.faults {
        l.push(Line::body(if customer { format!("{} - {}", f.code, f.description) } else { format!("{}  {}  [{}, {}]", f.code, f.description, f.module, f.status) }));
        let (sev, why) = checks::severity(&f.code);
        l.push(Line::detail(format!("{}: {}", tr(sev.label()), tr(why))));
        if let Some(h) = checks::lookup(&f.code) {
            if customer {
                if let Some(cause) = h.causes.first() {
                    l.push(Line::detail(format!("{}: {}", tr("Likely causes"), cause)));
                }
            } else {
                l.push(Line::detail(format!("{}: {}", tr("Likely causes"), h.causes.join("; "))));
                l.push(Line::detail(format!("{}: {}", tr("What to check"), h.checks.join("; "))));
            }
        }
    }
    if !r.modules_scanned {
        l.push(Line::muted(tr("Modules were not scanned: faults in other modules may be missing.")));
    }
    if let Some(s) = &r.since_last {
        if !customer {
            l.push(Line::heading(format!("{} ({})", tr("Since the last visit"), s.when)));
            l.push(Line::body(format!("{}: {}", tr("New since last visit"), list_or_none(&s.new))));
            l.push(Line::body(format!("{}: {}", tr("Gone since last visit"), list_or_none(&s.resolved))));
        }
    }
    if !r.actions.is_empty() || !r.extra.checks.is_empty() || r.extra.repair.is_some() {
        l.push(Line::heading(tr("Work done")));
        l.extend(r.actions.iter().map(|a| Line::body(format!("- {a}"))));
        l.extend(r.extra.checks.iter().map(|c| Line::body(format!("- {}: {c}", tr("Checked")))));
        if let Some(rep) = &r.extra.repair {
            l.push(Line::body(format!("- {}", rep.line())));
        }
    }
    if !customer && !r.extra.note.trim().is_empty() {
        l.push(Line::heading(tr("Shop note")));
        l.push(Line::body(r.extra.note.trim().to_string()));
    }
    if !r.extra.recommendation.trim().is_empty() || !r.extra.estimate.trim().is_empty() {
        l.push(Line::heading(tr("Our recommendation")));
        if !r.extra.recommendation.trim().is_empty() {
            l.push(Line::body(r.extra.recommendation.trim().to_string()));
        }
        if !r.extra.estimate.trim().is_empty() {
            l.push(Line::body(format!("{}: {}", tr("Estimate"), r.extra.estimate.trim())));
        }
    }
    if !customer {
        if !r.freeze.is_empty() {
            l.push(Line::heading(format!("{}{}", tr("Freeze frame"), r.freeze_code.as_ref().map(|c| format!(" ({})", tr("when {code} was set").replace("{code}", c))).unwrap_or_default())));
            l.extend(r.freeze.iter().map(|(k, v)| Line::body(format!("{k}: {v}"))));
        }
        if !r.readiness.is_empty() {
            l.push(Line::heading(tr("Readiness monitors")));
            l.extend(r.readiness.iter().map(|(n, ready)| Line::body(format!("{n}: {}", if *ready { tr("ready") } else { tr("NOT ready") }))));
        }
    }
    let images: Vec<crate::pdf::Image> = r.extra.photos.iter().filter_map(|p| crate::pdf::Image::from_file(p)).collect();
    if !images.is_empty() {
        l.push(Line::heading(tr("Photos")));
        l.extend((0..images.len()).map(|n| Line::picture(n, 170.0)));
    }
    l.push(Line::footer(if customer {
        format!("{} {}", tr("Generated by canforge. Read-only snapshot taken when the report was saved."), tr("The customer version leaves out the VIN, module addresses, adapter and freeze frame."))
    } else {
        tr("Generated by canforge. Read-only snapshot taken when the report was saved.").to_string()
    }));
    crate::pdf::render_with(&l, &images)
}

/// Plain text for pasting into a work order or message.
pub fn text(r: &Report) -> String {
    let mut o = format!("{}\n{}", r.vehicle, stamp(r.unix_time));
    if let Some(vin) = &r.vin {
        o.push_str(&format!("\nVIN {vin}"));
    }
    o.push_str(&format!("\nCheck-engine light: {}", if r.mil_on { "ON" } else { "off" }));
    if r.faults.is_empty() {
        o.push_str("\nNo trouble codes found.");
    }
    for f in &r.faults {
        o.push_str(&format!("\n{}  {}  [{}, {}]", f.code, f.description, f.module, f.status));
    }
    o
}

/// Writes the report into `<dir>/reports/` and returns its path.
pub fn write(dir: &Path, r: &Report) -> Result<PathBuf, String> {
    let reports = dir.join("reports");
    std::fs::create_dir_all(&reports).map_err(|e| format!("{}: {e}", reports.display()))?;
    let name: String = r.vehicle.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let path = reports.join(format!("{}-{}.html", name.trim_matches('-'), r.unix_time));
    std::fs::write(&path, html(r)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Writes the PDF into `<dir>/reports/` and returns its path.
pub fn write_pdf(dir: &Path, r: &Report, customer: bool) -> Result<PathBuf, String> {
    let reports = dir.join("reports");
    std::fs::create_dir_all(&reports).map_err(|e| format!("{}: {e}", reports.display()))?;
    let name: String = r.vehicle.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let path = reports.join(format!("{}-{}{}.pdf", name.trim_matches('-'), r.unix_time, if customer { "-customer" } else { "" }));
    std::fs::write(&path, pdf(r, customer)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_a_known_date() {
        assert_eq!(stamp(0), "1970-01-01 00:00 UTC");
        assert_eq!(stamp(1_781_000_000), "2026-06-09 10:13 UTC");
    }

    #[test]
    fn escapes_and_lists_faults() {
        let r = Report {
            unix_time: 0,
            vehicle: "A <b>".into(),
            vin: None,
            adapter: String::new(),
            mil_on: true,
            battery: None,
            faults: vec![Fault { code: "P0300".into(), description: "Misfire".into(), module: "Engine · 7E0".into(), status: "stored".into() }],
            modules_scanned: true,
            readiness: vec![("Misfire".into(), false)],
            freeze: vec![],
            freeze_code: None,
            actions: vec!["Cleared 1 code".into()],
            since_last: Some(SinceLast { when: "2026-10-01 09:00 UTC".into(), new: vec!["P0300".into()], resolved: vec![] }),
            extra: Extra::default(),
        };
        let h = html(&r);
        assert!(h.contains("A &lt;b&gt;") && h.contains("P0300") && h.contains("Not ready"));
        assert!(text(&r).contains("P0300  Misfire"));
        assert!(h.contains("Work done") && h.contains("Cleared 1 code") && h.contains("P0300") && h.contains("What to check"));
    }

    #[test]
    fn customer_pdf_leaves_out_technical_detail() {
        let r = Report {
            unix_time: 0,
            vehicle: "Fiesta".into(),
            vin: Some("WF0JXXWPJJAT64933".into()),
            adapter: "OpenPort".into(),
            mil_on: true,
            battery: None,
            faults: vec![Fault { code: "P0113".into(), description: "Intake air temperature".into(), module: "Engine · 7E0".into(), status: "stored".into() }],
            modules_scanned: true,
            readiness: vec![("Misfire".into(), false)],
            freeze: vec![("Engine speed".into(), "2250 rpm".into())],
            freeze_code: None,
            actions: vec![],
            since_last: None,
            extra: Extra::default(),
        };
        let text = |b: Vec<u8>| b.iter().map(|&c| c as char).collect::<String>();
        let customer = text(pdf(&r, true));
        let technical = text(pdf(&r, false));
        assert!(customer.contains("P0113") && customer.contains("Not ready for inspection"));
        assert!(!customer.contains("WF0JXXWPJJAT64933") && !customer.contains("7E0") && !customer.contains("OpenPort"));
        assert!(technical.contains("WF0JXXWPJJAT64933") && technical.contains("7E0") && technical.contains("Freeze frame"));
        assert!(customer.starts_with("%PDF"));
    }

    /// The customer sheet from the design (Paper): the same entries in the same order, the causes
    /// indented under their code, and the small print at the foot that says what was left out.
    #[test]
    fn customer_pdf_follows_the_paper_design() {
        let f = |c: &str, d: &str| Fault { code: c.into(), description: d.into(), module: "x".into(), status: "stored".into() };
        let r = Report {
            unix_time: 1_791_591_600,
            vehicle: "Fiesta 1.6 TDCi".into(),
            vin: None,
            adapter: String::new(),
            mil_on: true,
            battery: Some("12.6 V, Good (engine off)".into()),
            faults: vec![
                f("P0113", "Intake air temperature sensor circuit high"),
                f("P0101", "Mass air flow circuit range/performance"),
                f("C0035", "Left front wheel speed sensor circuit"),
            ],
            modules_scanned: true,
            readiness: vec![("Catalyst".into(), false)],
            freeze: vec![],
            freeze_code: None,
            actions: vec!["Engine control module: Particulate filter regeneration (accepted by the module)".into(), "Cleared 2 of 2 code(s) in the engine ECU. None came back yet.".into()],
            since_last: None,
            extra: Extra::default(),
        };
        let bytes = pdf(&r, true);
        let s: String = bytes.iter().map(|&c| c as char).collect();
        let at = |needle: &str| s.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
        assert!(at("Fiesta 1.6 TDCi") < at("Check-engine light: ON"));
        assert!(at("P0113 - Intake") < at("Likely causes: Intake air temperature sensor open circuit or failed"));
        assert!(at("Fault codes found") < at("Work done") && at("Work done") < at("Generated by canforge"));
        assert!(s.contains("customer version") && s.contains("freeze frame"));
        if let Some(out) = std::env::var_os("CANFORGE_PDF_OUT") {
            std::fs::write(out, bytes).unwrap();
        }
    }

    #[test]
    fn photos_and_the_repair_summary_reach_the_pdf() {
        let dir = std::env::temp_dir().join(format!("canforge-photo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("connector.png");
        image::RgbImage::from_fn(64, 48, |x, y| image::Rgb([(x * 4) as u8, (y * 5) as u8, 120])).save(&png).unwrap();
        let r = Report {
            unix_time: 0,
            vehicle: "Fiesta".into(),
            vin: None,
            adapter: String::new(),
            mil_on: false,
            battery: None,
            faults: vec![],
            modules_scanned: true,
            readiness: vec![],
            freeze: vec![],
            freeze_code: None,
            actions: vec![],
            since_last: None,
            extra: Extra {
                shop: "Garage Silva".into(),
                recommendation: "Replace the IAT connector.".into(),
                estimate: "45 EUR".into(),
                photos: vec![png],
                repair: Some(Repair { codes_before: 3, codes_now: 0, ready_before: (1, 8), ready_now: (8, 8), came_back: vec![] }),
                ..Extra::default()
            },
        };
        let s: String = pdf(&r, true).iter().map(|&c| c as char).collect();
        assert!(s.contains("/DCTDecode") && s.contains("/Im0 Do"));
        assert!(s.contains("Garage Silva") && s.contains("Replace the IAT connector.") && s.contains("Estimate: 45 EUR"));
        assert!(s.contains("before the repair") && s.contains("verified"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
