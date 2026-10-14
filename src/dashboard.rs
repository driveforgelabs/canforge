//! The live-data dashboard: which signals the user put on it and how each is shown.
//! Saved to `<data dir>/dashboard.json` so it survives restarts.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Something with a number to show: an engine PID, or a value a module reports (from the user's
/// own FORScan import).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Sig {
    /// A channel key such as `pid:0c`.
    Obd(String),
    Param { tx: u32, did: u16, bit: u32 },
}

impl Sig {
    pub fn id(&self) -> String {
        match self {
            Sig::Obd(key) => key.clone(),
            Sig::Param { tx, did, bit } => format!("{tx:03X}:{did:04X}:{bit}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    Card,
    Line,
    Area,
}

/// Chart time windows: label and number of samples (a sample every 250 ms).
pub const WINDOWS: [(&str, usize); 4] = [("10 s", 40), ("30 s", 120), ("1 min", 240), ("5 min", 1200)];
/// Chart widths: label and pixels.
pub const SIZES: [(&str, f32); 3] = [("Small", 340.), ("Medium", 520.), ("Large", 760.)];

fn default_window() -> u8 {
    1
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    pub sig: Sig,
    pub view: View,
    /// Index into [`SIZES`] (charts only).
    #[serde(default)]
    pub size: u8,
    /// Index into [`WINDOWS`] (charts only).
    #[serde(default = "default_window")]
    pub window: u8,
    /// Chart: fix the vertical axis to the signal's own range instead of fitting the data.
    #[serde(default)]
    pub lock_range: bool,
}

impl Widget {
    pub fn card(sig: Sig) -> Self {
        Self { sig, view: View::Card, size: 0, window: default_window(), lock_range: false }
    }

    pub fn chart(sig: Sig, view: View) -> Self {
        Self { sig, view, size: 1, window: default_window(), lock_range: false }
    }
}

/// Six cards and two charts for whatever the source provides.
pub fn defaults(keys: &[String]) -> Vec<Widget> {
    let mut out: Vec<Widget> = keys.iter().take(6).map(|k| Widget::card(Sig::Obd(k.clone()))).collect();
    for k in ["pid:0c", "pid:0d"] {
        if keys.iter().any(|c| c == k) {
            out.push(Widget::chart(Sig::Obd(k.into()), View::Area));
        }
    }
    out
}

pub fn load(dir: &Path) -> Option<Vec<Widget>> {
    serde_json::from_str(&std::fs::read_to_string(dir.join("dashboard.json")).ok()?).ok()
}

pub fn save(dir: &Path, widgets: &[Widget]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = serde_json::to_string_pretty(widgets).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("dashboard.json"), text).map_err(|e| format!("dashboard.json: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_dashboard_round_trips_and_old_files_still_load() {
        let dir = std::env::temp_dir().join(format!("canforge-dash-{}", std::process::id()));
        let widgets = vec![Widget::card(Sig::Obd("pid:0c".into())), Widget::chart(Sig::Param { tx: 0x7e0, did: 0xf40c, bit: 0 }, View::Line)];
        save(&dir, &widgets).unwrap();
        assert_eq!(load(&dir), Some(widgets));
        std::fs::write(dir.join("dashboard.json"), r#"[{"sig":{"Obd":"pid:05"},"view":"Card"}]"#).unwrap();
        let old = load(&dir).unwrap();
        assert_eq!((old[0].window, old[0].size, old[0].lock_range), (1, 0, false));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn defaults_chart_speed_and_rpm_when_present() {
        let keys: Vec<String> = ["pid:0c", "pid:0d", "pid:05"].map(String::from).into();
        let d = defaults(&keys);
        assert_eq!(d.len(), 5);
        assert_eq!(d[3].view, View::Area);
        assert!(defaults(&[]).is_empty());
    }
}
