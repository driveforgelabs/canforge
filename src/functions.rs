//! What a module can do, from the user's own FORScan import (`<data dir>/forscan/functions.tsv`,
//! written by `scripts/forscan_import.py`). Only functions the importer marked as runnable (a UDS
//! routine that needs no input of ours) can be started, and only after the user confirms.
//!
//! The file is the user's own local copy of proprietary data. It is never bundled, uploaded or
//! exported, and vehicle-db must not receive it (see vehicle-db/LEGAL.md).

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub title: String,
    /// UDS routine identifier (hex) the function runs, if it has one.
    pub routine: String,
    pub reads: String,
    pub writes: String,
    /// Diagnostic session to open first: 3 = extended. `None` stays in the default session.
    pub session: Option<u8>,
    /// Security access level the function asks for, if any (not supported: the module will refuse).
    pub security: String,
    /// 1 = ignition on, engine off. 2 = engine running.
    pub engine_mode: Option<u8>,
    /// A UDS routine that can be started: its only input is `option`.
    pub runnable: bool,
    /// The routine's option record, sent after the routine ID (empty if it has none).
    pub option: Vec<u8>,
}

impl Function {
    /// The UDS routine identifier, if it has one.
    pub fn routine_id(&self) -> Option<u16> {
        u16::from_str_radix(&self.routine, 16).ok()
    }

    pub fn needs_security(&self) -> bool {
        !self.security.is_empty() && self.security != "0"
    }

    /// The system this function is about, from its title, so a module's long list can be browsed.
    pub fn category(&self) -> &'static str {
        const TABLE: &[(&str, &[&str])] = &[
            ("Fuel and injectors", &["fuel", "injector", "rail", "low pressure"]),
            ("Exhaust and emissions", &["dpf", "particulate", "regeneration", "scr", "nox", "nitrogen", "exhaust", "catalyst", "lambda", "oxygen", "adblue", "urea", "differential pressure", "emission"]),
            ("Air, EGR and turbo", &["egr", "maf", "throttle", "etb", "turbo", "boost", "intake", "air ", "vcv", "wastegate", "swirl"]),
            ("Engine position and timing", &["crankshaft", "camshaft", "misfire", "cylinder", "timing", "idle", "compression", "oil"]),
            ("Brakes and steering", &["abs", "brake", "esp", "hydraulic", "steering", "yaw", "stability", "bleed", "parking"]),
            ("Transmission", &["transmission", "shift", "gear", "clutch", "converter", "tcm", "differential", "transfer"]),
            ("Security, keys and odometer", &["pats", "key", "immobil", "odometer", "vin", "security"]),
            ("Body, lighting and safety", &["lock", "remote", "rf ", "headlamp", "lamp", "led", "door", "window", "seat", "tire", "tpms", "crash", "airbag", "restraint", "ocs", "wiper", "mirror", "light"]),
            ("Module and setup", &["module", "initialization", "initialisation", "configuration", "scan for", "enter", "exit", "power", "relearn", "enable", "release", "resume", "stop", "apply", "local"]),
        ];
        let title = self.title.to_lowercase();
        TABLE.iter().find(|(_, words)| words.iter().any(|w| title.contains(w))).map_or("Other", |(name, _)| name)
    }
}

#[derive(Default)]
pub struct Functions {
    by_module: BTreeMap<u32, Vec<Function>>,
}

impl Functions {
    /// Empty when the file is missing or unreadable.
    pub fn load(dir: Option<&Path>) -> Self {
        let text = dir.and_then(|d| std::fs::read_to_string(d.join("forscan/functions.tsv")).ok()).unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        let mut by_module: BTreeMap<u32, Vec<Function>> = BTreeMap::new();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let mut f = line.split('\t');
            let (Some(id), Some(title)) = (f.next(), f.next()) else { continue };
            let Ok(id) = u32::from_str_radix(id, 16) else { continue };
            let mut field = || f.next().unwrap_or("").to_string();
            let (routine, reads, writes) = (field(), field(), field());
            let (session, security, mode, runnable, option) = (field(), field(), field(), field(), field());
            by_module.entry(id).or_default().push(Function {
                title: title.to_string(),
                routine,
                reads,
                writes,
                session: u8::from_str_radix(&session, 16).ok().filter(|s| *s != 0),
                security,
                engine_mode: mode.parse().ok(),
                runnable: runnable == "1",
                option: (0..option.len() / 2).filter_map(|i| u8::from_str_radix(option.get(i * 2..i * 2 + 2)?, 16).ok()).collect(),
            });
        }
        Self { by_module }
    }

    pub fn is_empty(&self) -> bool {
        self.by_module.is_empty()
    }

    pub fn for_module(&self, request_id: u32) -> &[Function] {
        self.by_module.get(&request_id).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// The user's import, loaded once.
pub fn get() -> &'static Functions {
    static FUNCTIONS: std::sync::OnceLock<Functions> = std::sync::OnceLock::new();
    FUNCTIONS.get_or_init(|| Functions::load(crate::dtcdb::data_dir().as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rows_and_skips_the_header_and_bad_lines() {
        let f = Functions::parse(
            "# REQUEST_ID\tTITLE\n760\tCalibrate Steering Angle Sensor\t3027\tMAINECUV\t\t3\t0\t2\t1\t\nnot hex\tx\n760\tReset adaptation\t4026\t\t\t\t8\t1\t1\t010C\nnot hex\tx\n760\tABS Service Bleed\n",
        );
        let rows = f.for_module(0x760);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].routine, "3027");
        assert_eq!(rows[0].routine_id(), Some(0x3027));
        assert_eq!(rows[0].reads, "MAINECUV");
        assert_eq!((rows[0].session, rows[0].engine_mode, rows[0].runnable, rows[0].needs_security()), (Some(3), Some(2), true, false));
        assert_eq!((rows[1].runnable, rows[1].option.as_slice()), (true, &[0x01, 0x0c][..]));
        assert!(rows[0].option.is_empty());
        assert_eq!(rows[2].routine, "");
        assert!(!rows[2].runnable && rows[2].routine_id().is_none());
        assert!(f.for_module(0x720).is_empty());
    }

    #[test]
    fn sorts_titles_into_systems() {
        let f = |t: &str| Function { title: t.into(), routine: String::new(), reads: String::new(), writes: String::new(), session: None, security: String::new(), engine_mode: None, runnable: false, option: vec![] };
        assert_eq!(f("Reset Fuel Injector Adaptation - Cylinder 2").category(), "Fuel and injectors");
        assert_eq!(f("Reset EGR Valve Adaptations").category(), "Air, EGR and turbo");
        assert_eq!(f("Reset Differential Pressure Sensor Learned Values").category(), "Exhaust and emissions");
        assert_eq!(f("Calibrate Steering Angle Sensor").category(), "Brakes and steering");
        assert_eq!(f("Something unusual").category(), "Other");
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(Functions::load(Some(Path::new("/nonexistent/canforge"))).is_empty());
        assert!(Functions::load(None).is_empty());
    }
}
