//! A copy of a module's readable data identifiers taken just before something is written to it,
//! so the values can be put back (UDS 0x2E) if the procedure goes wrong. Stored as JSON in
//! `<data dir>/backups/<vehicle key>/`. Local only.
//!
//! A restore only offers the identifiers whose value changed, and a module may still refuse to take
//! a value back (it often needs security access): the result says which were written.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub unix_time: u64,
    pub tx_id: u32,
    pub module: String,
    /// What was about to run, e.g. `Injector coding (routine 0203)`.
    pub before: String,
    /// (data identifier, value as hex).
    pub dids: Vec<(u16, String)>,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

impl Backup {
    pub fn new(unix_time: u64, tx_id: u32, module: String, before: String, values: &[(u16, Vec<u8>)]) -> Self {
        Self { unix_time, tx_id, module, before, dids: values.iter().map(|(d, v)| (*d, hex(v))).collect() }
    }

    /// The saved values as bytes.
    pub fn values(&self) -> Vec<(u16, Vec<u8>)> {
        self.dids.iter().filter_map(|(d, h)| Some((*d, from_hex(h)?))).collect()
    }
}

/// The saved identifiers whose current value differs: what a restore has to write. `now` is the
/// module's present data; an identifier missing from it counts as changed.
pub fn changed(saved: &[(u16, Vec<u8>)], now: &[(u16, Vec<u8>)]) -> Vec<(u16, Vec<u8>)> {
    saved.iter().filter(|(did, v)| now.iter().find(|(d, _)| d == did).map(|(_, c)| c) != Some(v)).cloned().collect()
}

fn folder(dir: &Path, key: &str) -> PathBuf {
    dir.join("backups").join(key)
}

pub fn save(dir: &Path, key: &str, b: &Backup) -> Result<PathBuf, String> {
    let folder = folder(dir, key);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let path = folder.join(format!("{}-{:03X}.json", b.unix_time, b.tx_id));
    std::fs::write(&path, serde_json::to_string_pretty(b).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// The vehicle's backups, newest first.
pub fn load(dir: &Path, key: &str) -> Vec<Backup> {
    let mut all: Vec<Backup> = std::fs::read_dir(folder(dir, key))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    all.sort_by_key(|b| std::cmp::Reverse(b.unix_time));
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        assert_eq!(hex(&[0, 0xab, 0x10]), "00AB10");
        assert_eq!(from_hex("00ab10"), Some(vec![0, 0xab, 0x10]));
        assert_eq!(from_hex("0"), None);
        assert_eq!(from_hex("zz"), None);
    }

    #[test]
    fn restore_lists_only_what_changed() {
        let b = Backup::new(1, 0x7e0, "Engine".into(), "x".into(), &[(0xf100, vec![1, 2]), (0xf101, vec![3]), (0xf102, vec![4])]);
        let now = vec![(0xf100, vec![1, 2]), (0xf101, vec![9])];
        assert_eq!(changed(&b.values(), &now), vec![(0xf101, vec![3]), (0xf102, vec![4])]);
    }

    #[test]
    fn saves_newest_first() {
        let d = std::env::temp_dir().join(format!("canforge-backup-{}", std::process::id()));
        for t in [5, 9] {
            save(&d, "K", &Backup::new(t, 0x7e0, "E".into(), "r".into(), &[(0xf100, vec![1])])).unwrap();
        }
        assert_eq!(load(&d, "K").iter().map(|b| b.unix_time).collect::<Vec<_>>(), vec![9, 5]);
        std::fs::remove_dir_all(d).ok();
    }
}
