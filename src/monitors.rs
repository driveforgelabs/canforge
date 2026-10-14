//! OBD-II mode 06: the results of the car's own self-tests (catalyst efficiency, misfire counts,
//! oxygen sensor response, EVAP leak checks), each with its limits. This is what shows a part that is
//! getting worse before it sets a code.
//!
//! The values are shown raw. Mode 06 scales each value by a unit-and-scaling identifier; the table of
//! those is long and the pass/fail verdict does not need it, because value and limits share a scale.

#[derive(Clone, Debug, PartialEq)]
pub struct Test {
    /// Monitor identifier: which system was tested.
    pub mid: u8,
    /// Test identifier within the monitor.
    pub tid: u8,
    pub uasid: u8,
    pub value: u16,
    pub min: u16,
    pub max: u16,
}

impl Test {
    /// Within its limits. Unit identifiers from 0x80 up are signed.
    pub fn passed(&self) -> bool {
        if self.uasid >= 0x80 {
            let (v, lo, hi) = (self.value as i16, self.min as i16, self.max as i16);
            v >= lo && v <= hi
        } else {
            self.value >= self.min && self.value <= self.max
        }
    }

    /// How close the value is to a limit, from 0 (at the limit) to 1 (in the middle of the range),
    /// so a test that passes but is nearly failing can be flagged. `None` for an open-ended range.
    pub fn margin(&self) -> Option<f64> {
        let (v, lo, hi) = if self.uasid >= 0x80 {
            (f64::from(self.value as i16), f64::from(self.min as i16), f64::from(self.max as i16))
        } else {
            (f64::from(self.value), f64::from(self.min), f64::from(self.max))
        };
        let span = hi - lo;
        if span <= 0.0 || (self.uasid < 0x80 && self.max == 0xffff) {
            return None;
        }
        Some((1.0 - ((v - (lo + hi) / 2.0).abs() / (span / 2.0))).clamp(0.0, 1.0))
    }
}

/// Reads a mode 06 answer for one monitor: `46 MID (TID UASID VAL MIN MAX)*`.
pub fn parse(response: &[u8]) -> Vec<Test> {
    let [0x46, mid, rest @ ..] = response else { return Vec::new() };
    rest.chunks_exact(8).map(|c| Test { mid: *mid, tid: c[0], uasid: c[1], value: u16::from_be_bytes([c[2], c[3]]), min: u16::from_be_bytes([c[4], c[5]]), max: u16::from_be_bytes([c[6], c[7]]) }).collect()
}

/// The monitor identifiers a "supported" answer (`46 base mask×4`) lists, and whether the next
/// block of 0x20 identifiers is supported too.
pub fn supported(response: &[u8]) -> (Vec<u8>, bool) {
    let [0x46, base, a, b, c, d, ..] = response else { return (Vec::new(), false) };
    let mask = u32::from_be_bytes([*a, *b, *c, *d]);
    let ids = (0..32u32).filter(|i| mask & (0x8000_0000 >> i) != 0).map(|i| base.wrapping_add(i as u8 + 1)).collect::<Vec<u8>>();
    let more = mask & 1 != 0;
    (ids.into_iter().filter(|id| id % 0x20 != 0).collect(), more)
}

/// What a monitor identifier tests, in workshop words.
pub fn mid_name(mid: u8) -> String {
    let bank = |n: u8| if n == 0 { "bank 1" } else { "bank 2" };
    match mid {
        0x01..=0x08 => format!("Oxygen sensor, {} sensor {}", bank((mid - 1) / 4), (mid - 1) % 4 + 1),
        0x21 => "Catalyst, bank 1".into(),
        0x22 => "Catalyst, bank 2".into(),
        0x31 => "EGR system, bank 1".into(),
        0x32 => "EGR system, bank 2".into(),
        0x35 => "Variable valve timing, bank 1".into(),
        0x36 => "Variable valve timing, bank 2".into(),
        0x39 => "EVAP leak check (0.150 in)".into(),
        0x3A => "EVAP leak check (0.090 in)".into(),
        0x3B => "EVAP leak check (0.040 in)".into(),
        0x3C => "EVAP leak check (0.020 in)".into(),
        0x3D => "EVAP purge flow".into(),
        0x41..=0x48 => format!("Oxygen sensor heater, {} sensor {}", bank((mid - 0x41) / 4), (mid - 0x41) % 4 + 1),
        0x61 => "Heated catalyst, bank 1".into(),
        0x62 => "Heated catalyst, bank 2".into(),
        0x71 => "Secondary air, bank 1".into(),
        0x72 => "Secondary air, bank 2".into(),
        0x81 => "Fuel system, bank 1".into(),
        0x82 => "Fuel system, bank 2".into(),
        0xa1 => "Misfire, all cylinders".into(),
        0xa2..=0xad => format!("Misfire, cylinder {}", mid - 0xa1),
        other => format!("Monitor {other:02X}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tests_in_eight_byte_groups() {
        // 46 A1 | 0B 0C 0001 0000 0010 | 0C 0C 0002 0000 0010
        let r = [0x46, 0xa1, 0x0b, 0x0c, 0, 1, 0, 0, 0, 0x10, 0x0c, 0x0c, 0, 2, 0, 0, 0, 0x10];
        let t = parse(&r);
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].mid, t[0].tid, t[0].value, t[0].max), (0xa1, 0x0b, 1, 0x10));
        assert!(parse(&[0x7f, 6, 0x11]).is_empty());
        assert!(parse(&[0x46, 0xa1, 1, 2, 3]).is_empty());
    }

    #[test]
    fn pass_fail_and_margin() {
        let t = |uasid, value, min, max| Test { mid: 1, tid: 1, uasid, value, min, max };
        assert!(t(0x01, 5, 0, 10).passed());
        assert!(!t(0x01, 11, 0, 10).passed());
        assert!(t(0x01, 0, 0, 10).passed());
        // signed: -3 within -5..5
        assert!(t(0x84, 0xfffd, 0xfffb, 5).passed());
        assert!(!t(0x84, 0xfff0, 0xfffb, 5).passed());
        assert_eq!(t(0x01, 5, 0, 10).margin(), Some(1.0));
        assert_eq!(t(0x01, 10, 0, 10).margin(), Some(0.0));
        assert_eq!(t(0x01, 5, 0, 0xffff).margin(), None);
    }

    #[test]
    fn lists_supported_monitors() {
        // MIDs 01, 02 and 0x21 (bit 0x21 - 0x00 - 1 = 32 is not in this block), next block flagged
        let (ids, more) = supported(&[0x46, 0x00, 0xc0, 0, 0, 0x01]);
        assert_eq!(ids, vec![0x01, 0x02]);
        assert!(more);
        let (ids, more) = supported(&[0x46, 0x20, 0x80, 0, 0, 0]);
        assert_eq!(ids, vec![0x21]);
        assert!(!more);
    }

    #[test]
    fn names() {
        assert_eq!(mid_name(0x02), "Oxygen sensor, bank 1 sensor 2");
        assert_eq!(mid_name(0xa3), "Misfire, cylinder 2");
        assert_eq!(mid_name(0x99), "Monitor 99");
    }
}
