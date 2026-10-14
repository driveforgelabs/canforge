//! The shop's own notes on a fault for one vehicle ("on this engine P0113 is nearly always the
//! connector behind the airbox"). Kept in `<data dir>/shop-notes.tsv`, on this computer only; a
//! note is found again the next time that vehicle shows that code.

use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    /// The vehicle's name, compared without regard to case.
    pub vehicle: String,
    pub code: String,
    pub text: String,
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n").replace('\t', " ")
}

fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(o) => out.push(o),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn load(dir: &Path) -> Vec<Note> {
    std::fs::read_to_string(dir.join("shop-notes.tsv"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            Some(Note { vehicle: f.next()?.to_string(), code: f.next()?.to_string(), text: unescape(f.next()?) })
        })
        .collect()
}

pub fn find<'a>(notes: &'a [Note], vehicle: &str, code: &str) -> Option<&'a Note> {
    let code = code.get(..5).unwrap_or(code);
    notes.iter().find(|n| n.vehicle.eq_ignore_ascii_case(vehicle) && n.code.eq_ignore_ascii_case(code))
}

/// Sets, or with empty text removes, the note and writes the file.
pub fn save(dir: &Path, notes: &mut Vec<Note>, vehicle: &str, code: &str, text: &str) -> Result<(), String> {
    let code = code.get(..5).unwrap_or(code).to_ascii_uppercase();
    notes.retain(|n| !(n.vehicle.eq_ignore_ascii_case(vehicle) && n.code == code));
    if !text.trim().is_empty() {
        notes.push(Note { vehicle: vehicle.to_string(), code, text: text.trim().to_string() });
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let body: String = notes.iter().map(|n| format!("{}\t{}\t{}\n", escape(&n.vehicle), n.code, escape(&n.text))).collect();
    std::fs::write(dir.join("shop-notes.tsv"), body).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_found_again_for_that_vehicle_and_code() {
        let dir = std::env::temp_dir().join(format!("canforge-notes-{}", std::process::id()));
        let mut notes = Vec::new();
        save(&dir, &mut notes, "Fiesta 1.6 TDCi", "P0113-1C", "Connector behind the airbox.\nCheck the clip.").unwrap();
        let loaded = load(&dir);
        assert_eq!(find(&loaded, "fiesta 1.6 tdci", "P0113").map(|n| n.text.as_str()), Some("Connector behind the airbox.\nCheck the clip."));
        assert!(find(&loaded, "Golf", "P0113").is_none());
        let mut notes = loaded;
        save(&dir, &mut notes, "Fiesta 1.6 TDCi", "P0113", "").unwrap();
        assert!(load(&dir).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
