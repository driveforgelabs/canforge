//! Units and number formatting: Celsius or Fahrenheit, kPa or psi, km/h or mph, chosen in Settings
//! and kept in `<data dir>/units`; and a decimal comma when the language is Portuguese. The car
//! always reports metric: values are converted only for display.

use std::sync::atomic::{AtomicBool, Ordering};

static FAHRENHEIT: AtomicBool = AtomicBool::new(false);
static PSI: AtomicBool = AtomicBool::new(false);
static MPH: AtomicBool = AtomicBool::new(false);

pub fn fahrenheit() -> bool {
    FAHRENHEIT.load(Ordering::Relaxed)
}

pub fn psi() -> bool {
    PSI.load(Ordering::Relaxed)
}

pub fn mph() -> bool {
    MPH.load(Ordering::Relaxed)
}

pub fn load() {
    let text = crate::dtcdb::data_dir().and_then(|d| std::fs::read_to_string(d.join("units")).ok()).unwrap_or_default();
    FAHRENHEIT.store(text.split_whitespace().any(|w| w == "f"), Ordering::Relaxed);
    PSI.store(text.split_whitespace().any(|w| w == "psi"), Ordering::Relaxed);
    MPH.store(text.split_whitespace().any(|w| w == "mph"), Ordering::Relaxed);
}

fn save() {
    if let Some(dir) = crate::dtcdb::data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let mut words = Vec::new();
        if fahrenheit() {
            words.push("f");
        }
        if psi() {
            words.push("psi");
        }
        if mph() {
            words.push("mph");
        }
        let _ = std::fs::write(dir.join("units"), words.join(" "));
    }
}

pub fn set_fahrenheit(on: bool) {
    FAHRENHEIT.store(on, Ordering::Relaxed);
    save();
}

pub fn set_psi(on: bool) {
    PSI.store(on, Ordering::Relaxed);
    save();
}

pub fn set_mph(on: bool) {
    MPH.store(on, Ordering::Relaxed);
    save();
}

/// `value` in `unit` as it is shown: (scale, offset, label) so that shown = value * scale + offset.
fn rule(unit: &str) -> (f64, f64, String) {
    match unit {
        "°C" if fahrenheit() => (1.8, 32.0, "°F".into()),
        "kPa" if psi() => (0.145_037_7, 0.0, "psi".into()),
        "km/h" if mph() => (0.621_371, 0.0, "mph".into()),
        _ => (1.0, 0.0, unit.to_string()),
    }
}

/// A decimal comma instead of a point in Portuguese, and a real minus sign.
pub fn localise(number: String) -> String {
    localise_for(number, crate::i18n::is_pt())
}

fn localise_for(number: String, portuguese: bool) -> String {
    let n = number.replace('-', "−");
    if portuguese { n.replace('.', ",") } else { n }
}

/// Decimals by how wide the range is: few for rpm and km/h, more for volts and bar.
fn decimals(span: f64) -> usize {
    if span >= 50.0 {
        0
    } else if span >= 5.0 {
        1
    } else {
        2
    }
}

/// The number and unit to show for a value whose gauge range is `span` wide, in the car's unit.
pub fn show(value: f64, span: f64, unit: &str) -> (String, String) {
    let (scale, offset, label) = rule(unit);
    let d = decimals(span * scale);
    (localise(format!("{:.*}", d, value * scale + offset)), label)
}

/// Like [`show`] for a value with no range: as many digits as it needs, up to two.
pub fn show_free(value: f64, unit: &str) -> (String, String) {
    let (scale, offset, label) = rule(unit);
    (localise(crate::health::compact(value * scale + offset)), label)
}

/// A unit's label as it is shown (°C or °F).
pub fn label(unit: &str) -> String {
    rule(unit).2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_only_when_asked() {
        FAHRENHEIT.store(false, Ordering::Relaxed);
        assert_eq!(show(88.0, 120.0, "°C"), ("88".into(), "°C".into()));
        FAHRENHEIT.store(true, Ordering::Relaxed);
        assert_eq!(show(88.0, 120.0, "°C"), ("190".into(), "°F".into()));
        assert_eq!(show(-40.0, 160.0, "°C"), ("−40".into(), "°F".into()));
        FAHRENHEIT.store(false, Ordering::Relaxed);
    }

    #[test]
    fn pressure_and_speed_follow_their_own_switch() {
        PSI.store(true, Ordering::Relaxed);
        assert_eq!(show(101.0, 120.0, "kPa"), ("14.6".into(), "psi".into()));
        PSI.store(false, Ordering::Relaxed);
        MPH.store(true, Ordering::Relaxed);
        assert_eq!(show(100.0, 250.0, "km/h"), ("62".into(), "mph".into()));
        MPH.store(false, Ordering::Relaxed);
        assert_eq!(show(14.2, 5.0, "V"), ("14.2".into(), "V".into()));
    }

    #[test]
    fn portuguese_uses_a_decimal_comma() {
        assert_eq!(localise_for("12.6".into(), true), "12,6");
        assert_eq!(localise_for("-3.5".into(), false), "−3.5");
    }
}
