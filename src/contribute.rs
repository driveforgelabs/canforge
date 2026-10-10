//! Helping the database: names for modules and descriptions for trouble codes that vehicle-db
//! does not have yet, entered by the person who has the car in front of them.
//!
//! Entries are kept in the user's own data directory (`modules.d/contributed.tsv`,
//! `dtc.d/contributed.tsv`, the same files canforge already loads as private overrides), so they
//! work immediately and survive restarts. [`export`] turns them into files that can be merged
//! into vehicle-db with its `tools/merge.py`.
//!
//! The rules mirror vehicle-db's CONTRIBUTING.md and are enforced here, at the moment of saving.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Module,
    Dtc,
}

impl Kind {
    fn dir(self) -> &'static str {
        match self {
            Kind::Module => "modules.d",
            Kind::Dtc => "dtc.d",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Module => "modules",
            Kind::Dtc => "dtc",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    /// `720` for a module (request ID, hex), `P1572` for a trouble code.
    pub id: String,
    pub make: String,
    pub text: String,
    /// `own-capture:<note>`
    pub source: String,
}

/// Diagnostic software whose data must not be copied. Same list as vehicle-db's validator.
const BANNED: &[&str] = &[
    "ddt2000", "ddt4all", "renolink", "clip", "odis", "xentry", "ista", "forscan", "vcds", "techstream", "tis2web",
    "ids", "vida", "snap-on", "autel", "launch",
];

fn file(dir: &Path, kind: Kind) -> PathBuf {
    dir.join(kind.dir()).join("contributed.tsv")
}

/// Turns a free-text vehicle description into the `source` note: `Ford Fiesta 1.6 TDCi` →
/// `own-capture:ford-fiesta-1.6-tdci`. Refuses VINs and names of OEM tools.
pub fn source_for(note: &str) -> Result<String, String> {
    let words: Vec<&str> = note.split_whitespace().collect();
    if words.is_empty() {
        return Err("Describe the vehicle first (make, model, engine, year; not the VIN).".into());
    }
    let slug = words.join("-").to_lowercase();
    if slug.len() > 60 {
        return Err("Keep the vehicle description under 60 characters.".into());
    }
    if words.iter().any(|w| {
        let w = w.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        w.len() == 17 && w.chars().all(|c| c.is_ascii_alphanumeric() && !"IOQioq".contains(c)) && w.chars().any(|c| c.is_ascii_digit())
    }) {
        return Err("That looks like a VIN. Describe the model instead (a VIN identifies one person's car).".into());
    }
    if let Some(bad) = BANNED.iter().find(|b| names_word(&slug, b)) {
        return Err(format!(
            "Data from {bad} is not accepted. Only enter what you worked out from your own vehicle."
        ));
    }
    Ok(format!("own-capture:{slug}"))
}

/// Whether `word` appears in `text` as a whole word (neighbours are not letters or digits), like
/// vehicle-db's validator, so `from-forscan` matches but `clipboard` does not match `clip`.
fn names_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric()) && !after.is_some_and(|c| c.is_ascii_alphanumeric())
    })
}

fn check_text(text: &str, max: usize, what: &str) -> Result<String, String> {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return Err(format!("Enter {what}."));
    }
    if t.chars().count() > max {
        return Err(format!("Keep {what} under {max} characters."));
    }
    Ok(t)
}

/// Normalises and validates an id for `kind`: `720` / `P1572-1C` → `P1572`.
fn check_id(kind: Kind, id: &str) -> Result<String, String> {
    let id = id.trim().to_ascii_uppercase();
    match kind {
        Kind::Module => {
            let n = u32::from_str_radix(&id, 16).map_err(|_| format!("{id} is not a request ID"))?;
            if id.len() != 3 || !(0x700..=0x7f7).contains(&n) || n == 0x7df || (0x7e8..=0x7ef).contains(&n) {
                return Err(format!("{id} is not a physical request ID this scan can name."));
            }
            Ok(id)
        }
        Kind::Dtc => {
            let code = id.get(..5).unwrap_or(&id);
            let b = code.as_bytes();
            let ok = b.len() == 5
                && b"PCBU".contains(&b[0])
                && (b'0'..=b'3').contains(&b[1])
                && b[2..].iter().all(u8::is_ascii_hexdigit);
            ok.then(|| code.to_string()).ok_or_else(|| format!("{id} is not a trouble code"))
        }
    }
}

pub fn load(dir: &Path, kind: Kind) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(file(dir, kind)) else { return Vec::new() };
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            (c.len() == 4).then(|| Entry { kind, id: c[0].into(), make: c[1].into(), text: c[2].into(), source: c[3].into() })
        })
        .collect()
}

/// Validates and stores one entry, replacing an earlier entry for the same id and make.
pub fn save(dir: &Path, kind: Kind, id: &str, make: &str, text: &str, note: &str) -> Result<Entry, String> {
    let make = make.trim().to_ascii_uppercase();
    if make.is_empty() || !make.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ') {
        return Err("Choose the vehicle's make first.".into());
    }
    let entry = Entry {
        kind,
        id: check_id(kind, id)?,
        make,
        text: match kind {
            Kind::Module => check_text(text, 60, "a name")?,
            Kind::Dtc => check_text(text, 300, "a description")?,
        },
        source: source_for(note)?,
    };
    let mut all = load(dir, kind);
    all.retain(|e| !(e.id == entry.id && e.make == entry.make));
    all.push(entry.clone());
    all.sort_by(|a, b| (&a.make, &a.id).cmp(&(&b.make, &b.id)));
    let mut out = String::from("# id\tmake\ttext\tsource  (your own contributions; see `canforge` Help improve)\n");
    for e in &all {
        out += &format!("{}\t{}\t{}\t{}\n", e.id, e.make, e.text, e.source);
    }
    let path = file(dir, kind);
    std::fs::create_dir_all(path.parent().expect("has a parent")).map_err(|e| e.to_string())?;
    std::fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(entry)
}

/// Writes `<out>/data/{modules,dtc}/<make>.tsv` containing only the contributions, in vehicle-db's
/// file format, ready for `tools/merge.py`. Returns the files written.
pub fn export(dir: &Path, out: &Path) -> Result<Vec<PathBuf>, String> {
    let mut written = Vec::new();
    for kind in [Kind::Module, Kind::Dtc] {
        let mut by_make: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
        for e in load(dir, kind) {
            by_make.entry(e.make.clone()).or_default().push(e);
        }
        for (make, mut entries) in by_make {
            entries.sort_by(|a, b| a.id.cmp(&b.id));
            let path = out.join("data").join(kind.label().replace("dtc", "dtc")).join(format!("{}.tsv", make.to_lowercase().replace(' ', "_")));
            std::fs::create_dir_all(path.parent().expect("has a parent")).map_err(|e| e.to_string())?;
            let header = match kind {
                Kind::Module => "# request_id\tmake\tname\tsource\n",
                Kind::Dtc => "# code\tmake\tdescription\tsource\n",
            };
            let body: String = entries.iter().map(|e| format!("{}\t{}\t{}\t{}\n", e.id, e.make, e.text, e.source)).collect();
            std::fs::write(&path, header.to_string() + &body).map_err(|e| format!("{}: {e}", path.display()))?;
            written.push(path);
        }
    }
    if written.is_empty() {
        return Err("Nothing to export yet: name a module or describe a code first.".into());
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("canforge-contrib-{}-{tag}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        d
    }

    #[test]
    fn source_note_is_slugged_and_checked() {
        assert_eq!(source_for("Ford Fiesta 1.6 TDCi 2018").as_deref(), Ok("own-capture:ford-fiesta-1.6-tdci-2018"));
        assert!(source_for("   ").is_err());
        assert!(source_for("WF0JXXWPJJAT64933").unwrap_err().contains("VIN"));
        assert!(source_for("my car (WF0JXXWPJJAT64933)").unwrap_err().contains("VIN"));
        assert!(source_for("from FORScan").unwrap_err().contains("forscan"));
        assert!(source_for("ddt4all export").is_err());
        assert!(source_for(&"x".repeat(80)).is_err());
        assert!(source_for("fiesta snap-on scan").is_err());
        // words that merely contain a banned name are fine
        assert!(source_for("fiesta clipboard test").is_ok());
    }

    #[test]
    fn modules_validate_ids_and_names() {
        let d = dir("m");
        assert!(save(&d, Kind::Module, "720", "ford", "Instrument cluster", "fiesta mk7").is_ok());
        assert!(save(&d, Kind::Module, "7DF", "ford", "x", "fiesta").unwrap_err().contains("not a physical"));
        assert!(save(&d, Kind::Module, "7E8", "ford", "x", "fiesta").is_err());
        assert!(save(&d, Kind::Module, "ZZZ", "ford", "x", "fiesta").is_err());
        assert!(save(&d, Kind::Module, "730", "ford", "", "fiesta").unwrap_err().contains("name"));
        assert!(save(&d, Kind::Module, "730", "", "Name", "fiesta").unwrap_err().contains("make"));
        assert!(save(&d, Kind::Module, "730", "ford", "Name", "").unwrap_err().contains("Describe the vehicle"));
        assert_eq!(load(&d, Kind::Module).len(), 1);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn dtc_ids_accept_suffixes_and_reject_junk() {
        let d = dir("d");
        let e = save(&d, Kind::Dtc, "p1572-01", "ford", "Brake switch circuit", "fiesta mk7").unwrap();
        assert_eq!((e.id.as_str(), e.make.as_str()), ("P1572", "FORD"));
        assert!(save(&d, Kind::Dtc, "X1572", "ford", "x", "fiesta").is_err());
        assert!(save(&d, Kind::Dtc, "P9999", "ford", "x", "fiesta").is_err());
        assert!(save(&d, Kind::Dtc, "P157", "ford", "x", "fiesta").is_err());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn saving_again_replaces_and_keeps_files_sorted() {
        let d = dir("r");
        save(&d, Kind::Module, "730", "ford", "Second", "fiesta").unwrap();
        save(&d, Kind::Module, "720", "ford", "First", "fiesta").unwrap();
        save(&d, Kind::Module, "720", "ford", "First, corrected", "fiesta").unwrap();
        let all = load(&d, Kind::Module);
        assert_eq!(all.iter().map(|e| (e.id.as_str(), e.text.as_str())).collect::<Vec<_>>(), [("720", "First, corrected"), ("730", "Second")]);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn saved_entries_are_picked_up_by_the_normal_loaders_after_restart() {
        let d = dir("l");
        save(&d, Kind::Module, "720", "ford", "Instrument cluster", "fiesta").unwrap();
        save(&d, Kind::Dtc, "P1572", "ford", "Brake switch circuit", "fiesta").unwrap();
        let names = crate::modnames::ModuleNames::load(Some(&d));
        assert_eq!(names.name(0x720, Some("FORD")), Some("Instrument cluster"));
        let db = crate::dtcdb::DtcDb::load(Some(&d));
        assert_eq!(db.lookup("P1572", Some("FORD")).description, "Brake switch circuit");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn export_writes_vehicle_db_shaped_files_per_make() {
        let d = dir("e");
        save(&d, Kind::Module, "730", "ford", "Parking aid module", "fiesta mk7").unwrap();
        save(&d, Kind::Module, "720", "ford", "Instrument cluster", "fiesta mk7").unwrap();
        save(&d, Kind::Dtc, "P1572", "ford", "Brake switch circuit", "fiesta mk7").unwrap();
        let out = d.join("out");
        let files = export(&d, &out).unwrap();
        assert_eq!(files.len(), 2);
        let m = std::fs::read_to_string(out.join("data/modules/ford.tsv")).unwrap();
        assert_eq!(
            m,
            "# request_id\tmake\tname\tsource\n720\tFORD\tInstrument cluster\town-capture:fiesta-mk7\n730\tFORD\tParking aid module\town-capture:fiesta-mk7\n"
        );
        assert!(out.join("data/dtc/ford.tsv").exists());
        assert!(export(&dir("empty"), &dir("empty-out")).is_err());
        std::fs::remove_dir_all(d).ok();
    }
}
