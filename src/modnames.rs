//! Names for diagnostic modules by request ID, from vehicle-db's `modules.tsv`.
//!
//! Layers like the trouble code data: the embedded snapshot, a downloaded release, then
//! your own `modules.d/*.tsv` in the data directory (`REQUEST_ID<TAB>MAKE<TAB>NAME`), which
//! override the shared names and are never uploaded.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::dtcdb::{data_dir, rows};

static EMBEDDED: &str = include_str!("../data/modules.tsv");
const GENERIC: &str = "GENERIC";

pub struct ModuleNames {
    /// `7E0` → (make, name) rows, user rows first
    by_id: HashMap<&'static str, Vec<(&'static str, &'static str)>>,
}

pub fn get() -> &'static ModuleNames {
    static NAMES: OnceLock<ModuleNames> = OnceLock::new();
    NAMES.get_or_init(|| ModuleNames::load(data_dir().as_deref()))
}

impl ModuleNames {
    fn add(&mut self, text: &'static str, front: bool) {
        for (id, make, name) in rows(text) {
            let list = self.by_id.entry(id).or_default();
            if front {
                list.insert(0, (make, name));
            } else {
                list.push((make, name));
            }
        }
    }

    pub fn embedded() -> Self {
        let mut n = Self { by_id: HashMap::new() };
        n.add(EMBEDDED, false);
        n
    }

    pub fn load(dir: Option<&Path>) -> Self {
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let mut n = Self::embedded();
        let Some(dir) = dir else { return n };
        if let Ok(text) = std::fs::read_to_string(dir.join("modules.tsv")) {
            let text = leak(text);
            if rows(text).next().is_some() {
                n = Self { by_id: HashMap::new() };
                n.add(text, false);
            }
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("modules.d"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "tsv"))
            .collect();
        files.sort();
        for file in files {
            if let Ok(text) = std::fs::read_to_string(file) {
                n.add(leak(text), true);
            }
        }
        n
    }

    /// The module's name for `make` if known, else the generic assignment.
    pub fn name(&self, request_id: u32, make: Option<&str>) -> Option<&'static str> {
        let rows = self.by_id.get(format!("{request_id:03X}").as_str())?;
        make.and_then(|m| rows.iter().find(|(rm, _)| rm.eq_ignore_ascii_case(m) && *rm != GENERIC))
            .or_else(|| rows.iter().find(|(rm, _)| *rm == GENERIC))
            .map(|(_, name)| *name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_has_the_standard_obd_modules() {
        let n = ModuleNames::embedded();
        assert_eq!(n.name(0x7e0, None), Some("Engine control module"));
        assert_eq!(n.name(0x7e1, Some("FORD")), Some("Transmission control module"));
        assert_eq!(n.name(0x720, Some("FORD")), None);
    }

    #[test]
    fn user_names_override_and_make_beats_generic() {
        let dir = std::env::temp_dir().join(format!("canforge-modnames-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("modules.d")).unwrap();
        std::fs::write(dir.join("modules.d/mine.tsv"), "720\tFORD\tInstrument cluster\n7E0\tFORD\tPCM\n").unwrap();
        let n = ModuleNames::load(Some(&dir));
        assert_eq!(n.name(0x720, Some("ford")), Some("Instrument cluster"));
        assert_eq!(n.name(0x720, Some("GM")), None); // names are per make
        assert_eq!(n.name(0x7e0, Some("FORD")), Some("PCM")); // make-specific beats generic
        assert_eq!(n.name(0x7e0, Some("GM")), Some("Engine control module"));
        std::fs::remove_dir_all(dir).ok();
    }
}
