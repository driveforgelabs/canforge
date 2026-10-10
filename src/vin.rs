//! VIN → make, using the World Manufacturer Identifier (first 3 characters).
//!
//! The returned name is used for display and as the make for trouble code lookup. A make can be
//! recognised here without having manufacturer-specific codes in the database (Renault, for
//! instance): the lookup then says so.

/// (WMI prefix, make). Longest matching prefix wins.
const WMI: &[(&str, &str)] = &[
    ("WF0", "FORD"), ("WF1", "FORD"), ("VS6", "FORD"), ("1FA", "FORD"), ("1FB", "FORD"),
    ("1FC", "FORD"), ("1FD", "FORD"), ("1FM", "FORD"), ("1FT", "FORD"), ("2FM", "FORD"),
    ("3FA", "FORD"), ("MAJ", "FORD"), ("NM0", "FORD"), ("6FP", "FORD"), ("SFA", "FORD"),
    ("1G1", "CHEVY"), ("1GC", "CHEVY"), ("1G6", "CADILLAC"), ("1GM", "PONTIAC"),
    ("WVW", "VOLKSWAGEN"), ("WV1", "VOLKSWAGEN"), ("WV2", "VOLKSWAGEN"), ("3VW", "VOLKSWAGEN"),
    ("WAU", "AUDI"), ("WUA", "AUDI"),
    ("WBA", "BMW"), ("WBS", "BMW"), ("WBY", "BMW"), ("WMW", "MINI"),
    ("WDB", "MERCEDES"), ("WDD", "MERCEDES"), ("WDC", "MERCEDES"), ("W1K", "MERCEDES"),
    ("W1N", "MERCEDES"), ("WDF", "MERCEDES"), ("WME", "SMART"),
    ("JT", "TOYOTA"), ("4T", "TOYOTA"), ("JH", "HONDA"), ("1HG", "HONDA"), ("JN", "NISSAN"),
    ("JM", "MAZDA"), ("JF", "SUBARU"), ("KMH", "HYUNDAI"), ("KNA", "KIA"), ("KND", "KIA"),
    ("1C3", "CHRYSLER"), ("1J4", "JEEP"), ("SAJ", "JAGUAR"), ("SAL", "LAND ROVER"),
    ("VF1", "RENAULT"), ("93Y", "RENAULT"), ("UU1", "DACIA"),
    ("VF3", "PEUGEOT"), ("VR3", "PEUGEOT"), ("VF7", "CITROEN"), ("VR7", "CITROEN"),
    ("W0L", "OPEL"), ("W0V", "OPEL"),
    ("ZFA", "FIAT"), ("ZAR", "ALFA ROMEO"), ("ZLA", "LANCIA"),
    ("YV1", "VOLVO"), ("YV4", "VOLVO"), ("TMB", "SKODA"), ("VSS", "SEAT"),
    ("WP0", "PORSCHE"), ("WP1", "PORSCHE"), ("5YJ", "TESLA"), ("7SA", "TESLA"),
];

/// Every make this table can return, sorted.
pub fn makes() -> Vec<&'static str> {
    let mut v: Vec<&str> = WMI.iter().map(|(_, m)| *m).collect();
    v.sort_unstable();
    v.dedup();
    v
}

pub fn make(vin: &str) -> Option<&'static str> {
    let vin = vin.to_ascii_uppercase();
    WMI.iter()
        .filter(|(prefix, _)| vin.starts_with(prefix))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, make)| *make)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_known_prefixes() {
        assert_eq!(make("WF0AXXWPMA8Y12345"), Some("FORD"));
        assert_eq!(make("wf0axxwpma8y12345"), Some("FORD"));
        assert_eq!(make("JTDKB20U123456789"), Some("TOYOTA"));
        assert_eq!(make("WVWZZZ1JZ3W386752"), Some("VOLKSWAGEN"));
        assert_eq!(make("VF1BB05C123456789"), Some("RENAULT"));
        assert_eq!(make("VF3XXXXXXXX000000"), Some("PEUGEOT"));
        assert_eq!(make("YV1XXXXXXXX000000"), Some("VOLVO"));
    }

    #[test]
    fn unknown_or_short() {
        assert_eq!(make("W"), None);
        assert_eq!(make("XXXBB05C123456789"), None);
    }
}
