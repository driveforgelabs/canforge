//! What each visit found, kept per vehicle so the next visit can say what changed: codes that are
//! new, codes that went away, and what the last clear did. Stored as one JSON file per visit in
//! `<data dir>/history/<vehicle key>/`. Local only.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub unix_time: u64,
    pub vehicle: String,
    pub vin: Option<String>,
    pub mil_on: bool,
    /// `P0113` style codes, one per (code, module) pair.
    pub codes: Vec<String>,
    pub battery_volts: Option<f64>,
    /// Monitors that were supported but not ready.
    pub not_ready: Vec<String>,
    /// What was done to the car during the visit, in order ("Cleared 3 codes", "Ran DPF regeneration").
    #[serde(default)]
    pub actions: Vec<String>,
    /// Suggested checks the mechanic ticked off, as "P0113: Check the connector for corrosion".
    #[serde(default)]
    pub checks_done: Vec<String>,
    /// The mechanic's note on the visit, what they recommend to the customer, and the estimate.
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub recommendation: String,
    #[serde(default)]
    pub estimate: String,
    /// Photos kept for this visit: file names in `<data dir>/attachments/<vehicle key>/<visit>/`.
    #[serde(default)]
    pub photos: Vec<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Diff {
    pub new: Vec<String>,
    pub resolved: Vec<String>,
    pub unchanged: Vec<String>,
}

/// What changed from `before` to `now`.
pub fn diff(before: &Visit, now: &Visit) -> Diff {
    let a: BTreeSet<&String> = before.codes.iter().collect();
    let b: BTreeSet<&String> = now.codes.iter().collect();
    Diff {
        new: b.difference(&a).map(|s| s.to_string()).collect(),
        resolved: a.difference(&b).map(|s| s.to_string()).collect(),
        unchanged: a.intersection(&b).map(|s| s.to_string()).collect(),
    }
}

/// A folder name for a vehicle: its VIN when known, else its name.
pub fn key(vin: Option<&str>, name: &str) -> String {
    let raw = vin.filter(|v| !v.is_empty()).unwrap_or(name);
    let k: String = raw.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '-' }).collect();
    let k = k.trim_matches('-').to_string();
    if k.is_empty() { "UNKNOWN".into() } else { k }
}

fn folder(dir: &Path, key: &str) -> PathBuf {
    dir.join("history").join(key)
}

pub fn save(dir: &Path, key: &str, visit: &Visit) -> Result<PathBuf, String> {
    let folder = folder(dir, key);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = folder.join(format!("{}.json", visit.unix_time));
    let json = serde_json::to_string_pretty(visit).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Every saved visit of the vehicle, oldest first. Unreadable files are skipped.
pub fn load(dir: &Path, key: &str) -> Vec<Visit> {
    let mut visits: Vec<Visit> = std::fs::read_dir(folder(dir, key))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    visits.sort_by_key(|v| v.unix_time);
    visits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit(t: u64, codes: &[&str]) -> Visit {
        Visit { unix_time: t, vehicle: "Fiesta".into(), codes: codes.iter().map(|c| c.to_string()).collect(), ..Visit::default() }
    }

    #[test]
    fn diff_splits_new_resolved_unchanged() {
        let d = diff(&visit(1, &["P0113", "P0101"]), &visit(2, &["P0101", "P0300"]));
        assert_eq!((d.new, d.resolved, d.unchanged), (vec!["P0300".to_string()], vec!["P0113".to_string()], vec!["P0101".to_string()]));
    }

    #[test]
    fn key_prefers_vin_and_is_filename_safe() {
        assert_eq!(key(Some("wf0jxxwpjjat64933"), "x"), "WF0JXXWPJJAT64933");
        assert_eq!(key(None, "Golf VII 2.0/TDI"), "GOLF-VII-2-0-TDI");
        assert_eq!(key(Some(""), "///"), "UNKNOWN");
    }

    #[test]
    fn saves_and_loads_oldest_first() {
        let d = std::env::temp_dir().join(format!("canforge-history-{}", std::process::id()));
        save(&d, "K", &visit(20, &["B"])).unwrap();
        save(&d, "K", &visit(10, &["A"])).unwrap();
        let v = load(&d, "K");
        assert_eq!(v.iter().map(|v| v.unix_time).collect::<Vec<_>>(), vec![10, 20]);
        assert!(load(&d, "other").is_empty());
        std::fs::remove_dir_all(d).ok();
    }
}
