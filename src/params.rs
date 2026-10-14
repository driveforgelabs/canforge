//! Readable parameters of a module, from the user's own FORScan import (`<data dir>/forscan/params.tsv`,
//! written by `scripts/forscan_import.py`): name, data identifier, length and how to scale the answer.
//!
//! The file is the user's own local copy of proprietary data. It is never bundled, uploaded or
//! exported, and vehicle-db must not receive it (see vehicle-db/LEGAL.md). Reading a parameter is a
//! plain UDS read-data request; a module that does not have the identifier simply refuses it.

use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub did: u16,
    pub name: String,
    pub length: usize,
    pub mult: f64,
    pub div: f64,
    pub offset: f64,
    pub unit: String,
    pub bit: u32,
    /// The identifier carries several single-bit items (`bit` says which one).
    pub is_bit: bool,
}

impl Param {
    /// The scaled number (or the bit as 0/1), for charts and cards.
    pub fn value(&self, data: &[u8]) -> Option<f64> {
        if data.is_empty() {
            return None;
        }
        let raw = data[..data.len().min(8)].iter().fold(0u64, |acc, b| (acc << 8) | *b as u64);
        Some(if self.is_bit { ((raw >> self.bit.min(63)) & 1) as f64 } else { raw as f64 * self.mult / self.div + self.offset })
    }

    /// The answer as text: the scaled value, or the bit, with the raw bytes always shown.
    #[cfg(test)]
    pub fn show(&self, data: &[u8]) -> String {
        let raw_hex: String = data.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ");
        if data.is_empty() {
            return "empty answer".into();
        }
        let n = data.len().min(8);
        let raw = data[..n].iter().fold(0u64, |acc, b| (acc << 8) | *b as u64);
        if self.is_bit {
            let bit = (raw >> self.bit.min(63)) & 1;
            return format!("bit {} = {bit}  (raw {raw_hex}; bit order not verified)", self.bit);
        }
        let value = raw as f64 * self.mult / self.div + self.offset;
        let unit = if self.unit.is_empty() { String::new() } else { format!(" {}", self.unit) };
        format!("{}{unit}  (raw {raw_hex})", trim(value))
    }
}

#[cfg(test)]
fn trim(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[derive(Default)]
pub struct Params {
    by_module: BTreeMap<u32, Vec<Param>>,
}

impl Params {
    /// Empty when the file is missing or unreadable.
    pub fn load(dir: Option<&Path>) -> Self {
        let text = dir.and_then(|d| std::fs::read_to_string(d.join("forscan/params.tsv")).ok()).unwrap_or_default();
        Self::parse(&text)
    }

    fn parse(text: &str) -> Self {
        let mut by_module: BTreeMap<u32, Vec<Param>> = BTreeMap::new();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 11 {
                continue;
            }
            let (Ok(id), Ok(did)) = (u32::from_str_radix(f[0], 16), u16::from_str_radix(f[1], 16)) else { continue };
            let (Ok(length), Ok(mult), Ok(div), Ok(offset), Ok(bit)) =
                (f[3].parse(), f[4].parse(), f[5].parse::<f64>(), f[6].parse(), f[8].parse())
            else {
                continue;
            };
            if div == 0.0 {
                continue;
            }
            by_module.entry(id).or_default().push(Param {
                did,
                name: f[2].to_string(),
                length,
                mult,
                div,
                offset,
                unit: f[7].to_string(),
                bit,
                is_bit: f[10] == "bits",
            });
        }
        Self { by_module }
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.by_module.is_empty()
    }

    pub fn for_module(&self, request_id: u32) -> &[Param] {
        self.by_module.get(&request_id).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// The user's import, loaded once.
pub fn get() -> &'static Params {
    static PARAMS: std::sync::OnceLock<Params> = std::sync::OnceLock::new();
    PARAMS.get_or_init(|| Params::load(crate::dtcdb::data_dir().as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# comment\n# REQUEST_ID\n\
        7E0\tF405\tEngine coolant temperature\t1\t1\t1\t-40\t°C\t0\t414\tvalue\n\
        7E0\tF40C\tEngine speed\t2\t1\t4\t0\t1/min\t0\t414\tvalue\n\
        7E0\t0394\tCylinder 2 injector fault\t4\t1\t1\t0\t\t1\t506\tbits\n\
        7E0\tBAD\tnot hex did\n";

    #[test]
    fn parses_and_skips_bad_lines() {
        let p = Params::parse(FILE);
        assert_eq!(p.for_module(0x7e0).len(), 3);
        assert!(p.for_module(0x760).is_empty());
        assert_eq!(p.for_module(0x7e0)[1].did, 0xf40c);
    }

    #[test]
    fn scales_values_and_shows_raw_bytes() {
        let p = Params::parse(FILE);
        let m = p.for_module(0x7e0);
        assert_eq!(m[0].show(&[0x7d]), "85 °C  (raw 7D)"); // 125 - 40
        assert_eq!(m[1].show(&[0x1f, 0x40]), "2000 1/min  (raw 1F 40)"); // 8000 / 4
        assert_eq!(m[2].show(&[0, 0, 0, 0b10]), "bit 1 = 1  (raw 00 00 00 02; bit order not verified)");
        assert_eq!(m[0].show(&[]), "empty answer");
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(Params::load(Some(Path::new("/nonexistent/canforge"))).is_empty());
        assert!(Params::load(None).is_empty());
    }
}
