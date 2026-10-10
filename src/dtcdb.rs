//! Trouble code descriptions, from [vehicle-db](https://github.com/driveforgelabs/vehicle-db).
//!
//! Layers, later ones winning:
//! 1. the snapshot embedded in the binary (`data/dtc.tsv`, refreshed with `scripts/sync_vehicle_db.sh`)
//! 2. a downloaded release in the data directory (`canforge db update`), if present and valid
//! 3. your own files, `dtc.d/*.tsv` in the data directory, same format. Never shared or uploaded.
//!
//! Generic codes (SAE J2012) mean the same on every car. Manufacturer-specific codes
//! (mostly P1xxx, B1xxx, C1xxx, U1xxx) are defined by each carmaker, so they can only be
//! described when the make is known.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static EMBEDDED: &str = include_str!("../data/dtc.tsv");
const GENERIC: &str = "GENERIC";

/// Directory for downloaded data and user files: `$CANFORGE_DATA_DIR`, else the platform's
/// per-user data directory plus `canforge`.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CANFORGE_DATA_DIR") {
        return Some(dir.into());
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    base.map(|b| b.join("canforge"))
}

pub struct DtcDb {
    /// code → (make, description) rows
    by_code: HashMap<&'static str, Vec<(&'static str, &'static str)>>,
    /// e.g. `v0.1.0`, from the data file's header.
    pub version: String,
    pub origin: Origin,
    /// Number of rows loaded from the user's own files.
    pub user_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Embedded,
    Downloaded(PathBuf),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Scope {
    Generic,
    Manufacturer,
    Unknown,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Info {
    pub description: String,
    pub scope: Scope,
}

/// The database, loaded on first use. Borrows from loaded text, so lookups copy nothing.
pub fn get() -> &'static DtcDb {
    static DB: OnceLock<DtcDb> = OnceLock::new();
    DB.get_or_init(|| DtcDb::load(data_dir().as_deref()))
}

/// The version named in a data file's `# vehicle-db <version>: …` header.
pub fn header_version(text: &str) -> Option<&str> {
    text.lines().next()?.strip_prefix("# vehicle-db ")?.split(':').next()
}

/// Rows parsed without allocating: `(code, make, description)`. Comments and malformed
/// lines are skipped.
pub fn rows(text: &str) -> impl Iterator<Item = (&str, &str, &str)> {
    text.lines().filter(|l| !l.starts_with('#')).filter_map(|line| {
        let mut cols = line.splitn(4, '\t');
        Some((cols.next()?, cols.next()?, cols.next()?))
    })
}

impl DtcDb {
    pub fn embedded() -> Self {
        Self::build(EMBEDDED, Origin::Embedded)
    }

    fn build(text: &'static str, origin: Origin) -> Self {
        let mut by_code: HashMap<_, Vec<_>> = HashMap::new();
        for (code, make, desc) in rows(text) {
            by_code.entry(code).or_default().push((make, desc));
        }
        let version = header_version(text).unwrap_or("unknown").to_string();
        Self { by_code, version, origin, user_rows: 0 }
    }

    /// Embedded snapshot, then the downloaded release if it parses, then `dtc.d/*.tsv`.
    /// Files are kept for the life of the process, which is why their text is leaked.
    pub fn load(dir: Option<&Path>) -> Self {
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let mut db = Self::embedded();
        let Some(dir) = dir else { return db };

        if let Ok(text) = std::fs::read_to_string(dir.join("dtc.tsv")) {
            let text = leak(text);
            if rows(text).next().is_some() {
                db = Self::build(text, Origin::Downloaded(dir.join("dtc.tsv")));
            }
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("dtc.d"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "tsv"))
            .collect();
        files.sort();
        for file in files {
            if let Ok(text) = std::fs::read_to_string(&file) {
                for (code, make, desc) in rows(leak(text)) {
                    // user rows go first so they override the shared data
                    db.by_code.entry(code).or_default().insert(0, (make, desc));
                    db.user_rows += 1;
                }
            }
        }
        db
    }

    /// Makes that have manufacturer-specific entries, sorted.
    pub fn makes(&self) -> Vec<&'static str> {
        let mut v: Vec<&str> = self.by_code.values().flatten().map(|(m, _)| *m).filter(|m| *m != GENERIC).collect();
        v.sort_unstable();
        v.dedup();
        v
    }

    pub fn len(&self) -> usize {
        self.by_code.values().map(Vec::len).sum()
    }

    /// Describes `code` (e.g. `"P0113"`). `make` (e.g. `"FORD"`, case-insensitive) selects
    /// manufacturer-specific definitions.
    pub fn lookup(&self, code: &str, make: Option<&str>) -> Info {
        let rows = self.by_code.get(code).map(Vec::as_slice).unwrap_or_default();
        let found = |description: &str, scope| Info { description: description.to_string(), scope };

        // a user's own row for the requested make beats the shared generic one
        if let Some(make) = make {
            if let Some((_, desc)) = rows.iter().find(|(m, _)| m.eq_ignore_ascii_case(make) && *m != GENERIC) {
                if self.user_rows > 0 || !rows.iter().any(|(m, _)| *m == GENERIC) {
                    return found(desc, Scope::Manufacturer);
                }
            }
        }
        if let Some((_, desc)) = rows.iter().find(|(m, _)| *m == GENERIC) {
            return found(desc, Scope::Generic);
        }
        if let Some(make) = make {
            if let Some((_, desc)) = rows.iter().find(|(m, _)| m.eq_ignore_ascii_case(make)) {
                return found(desc, Scope::Manufacturer);
            }
        }
        let description = match (rows.is_empty(), make) {
            (true, _) if code.as_bytes().get(1) == Some(&b'0') => "Generic code (no description available)".into(),
            (true, _) => "Manufacturer-specific code (no description available)".into(),
            (false, None) => "Manufacturer-specific code (set --make to look it up)".into(),
            (false, Some(make)) => format!("Manufacturer-specific code (not listed for {})", make.to_uppercase()),
        };
        Info { description, scope: Scope::Unknown }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_codes_ignore_make() {
        let db = DtcDb::embedded();
        let a = db.lookup("P0113", None);
        assert_eq!(a.scope, Scope::Generic);
        assert!(a.description.starts_with("Intake Air Temperature Sensor 1 Circuit High"), "{}", a.description);
        assert_eq!(db.lookup("P0113", Some("ford")), a);
    }

    #[test]
    fn manufacturer_codes_depend_on_make() {
        let db = DtcDb::embedded();
        let ford = db.lookup("P1690", Some("ford"));
        assert_eq!((ford.scope, ford.description.as_str()), (Scope::Manufacturer, "Wastegate Solenoid Circuit Malfunction"));
        assert_eq!(db.lookup("P1690", Some("GM")).description, "ECM Loop Overrun");
        assert!(db.lookup("P1690", None).description.contains("--make"));
        assert!(db.lookup("P1690", Some("tesla")).description.contains("not listed for TESLA"));
    }

    #[test]
    fn unknown_codes_fall_back() {
        let db = DtcDb::embedded();
        assert_eq!(db.lookup("P0FFF", None).scope, Scope::Unknown);
        assert!(db.lookup("P0FFF", None).description.starts_with("Generic code"));
        assert!(db.lookup("P1FFF", None).description.starts_with("Manufacturer-specific"));
    }

    #[test]
    fn lists_makes_without_generic() {
        let makes = DtcDb::embedded().makes();
        assert!(makes.contains(&"FORD") && makes.contains(&"VOLKSWAGEN"));
        assert!(!makes.contains(&"GENERIC"));
        assert!(makes.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn embedded_snapshot_is_populated_and_versioned() {
        let db = DtcDb::embedded();
        assert!(db.len() > 10_000);
        assert!(db.version.starts_with('v'), "{}", db.version);
        assert_eq!(db.origin, Origin::Embedded);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("canforge-dtcdb-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(d.join("dtc.d")).unwrap();
        d
    }

    #[test]
    fn downloaded_release_replaces_the_snapshot() {
        let dir = temp_dir("dl");
        std::fs::write(dir.join("dtc.tsv"), "# vehicle-db v9.9.9: CODE\tMAKE\tDESCRIPTION\nP0113\tGENERIC\tFrom the release\n").unwrap();
        let db = DtcDb::load(Some(&dir));
        assert_eq!(db.version, "v9.9.9");
        assert_eq!(db.lookup("P0113", None).description, "From the release");
        assert!(matches!(db.origin, Origin::Downloaded(_)));
        assert_eq!(db.len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn broken_download_falls_back_to_the_snapshot() {
        let dir = temp_dir("bad");
        std::fs::write(dir.join("dtc.tsv"), "not a data file").unwrap();
        let db = DtcDb::load(Some(&dir));
        assert_eq!(db.origin, Origin::Embedded);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn user_files_override_shared_data_and_add_makes() {
        let dir = temp_dir("user");
        std::fs::write(
            dir.join("dtc.d/mine.tsv"),
            "P1690\tFORD\tMy own wastegate note\nP1ABC\tRENAULT\tMy private Renault code\tsomething ignored\n",
        )
        .unwrap();
        let db = DtcDb::load(Some(&dir));
        assert_eq!(db.user_rows, 2);
        assert_eq!(db.lookup("P1690", Some("FORD")).description, "My own wastegate note");
        assert_eq!(db.lookup("P1ABC", Some("renault")).description, "My private Renault code");
        assert_eq!(db.lookup("P1690", Some("GM")).description, "ECM Loop Overrun"); // other makes untouched
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn user_rows_can_override_a_generic_description_for_their_make() {
        let dir = temp_dir("gen");
        std::fs::write(dir.join("dtc.d/mine.tsv"), "P0113\tFORD\tFord-specific wording\n").unwrap();
        let db = DtcDb::load(Some(&dir));
        assert_eq!(db.lookup("P0113", Some("FORD")).description, "Ford-specific wording");
        assert!(db.lookup("P0113", None).description.starts_with("Intake Air"));
        std::fs::remove_dir_all(dir).ok();
    }
}
