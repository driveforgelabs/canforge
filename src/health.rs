//! Plain-language verdicts on live readings, so the screen says what a mechanic would conclude
//! instead of just showing a number.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Good,
    Warn,
    Bad,
    Unknown,
}

/// Engine speed above which the engine counts as running.
pub const RUNNING_RPM: f64 = 400.0;

pub fn engine_running(rpm: Option<f64>) -> Option<bool> {
    rpm.map(|r| r > RUNNING_RPM)
}

/// Battery or charging verdict. The same voltage means different things with the engine off
/// (battery state) and running (alternator output), so engine speed decides which is meant.
pub fn battery(volts: f64, rpm: Option<f64>) -> (&'static str, Level) {
    match engine_running(rpm) {
        Some(true) => match volts {
            v if v < 13.2 => ("Not charging", Level::Bad),
            v if v <= 14.9 => ("Charging OK", Level::Good),
            _ => ("Overcharging", Level::Bad),
        },
        Some(false) => match volts {
            v if v < 11.8 => ("Flat", Level::Bad),
            v if v < 12.4 => ("Weak: charge or test it", Level::Warn),
            _ => ("Good (engine off)", Level::Good),
        },
        None => ("Engine speed unknown", Level::Unknown),
    }
}

/// Whether the supply is good enough to write to a module or start a routine. A procedure that
/// loses power half way can leave a module unusable, so this blocks rather than only warns when the
/// voltage is clearly too low.
#[derive(Clone, Debug, PartialEq)]
pub enum Gate {
    Ok(String),
    Warn(String),
    Block(String),
}

pub fn write_gate(volts: Option<f64>, rpm: Option<f64>) -> Gate {
    let Some(v) = volts else {
        return Gate::Warn("Battery voltage unknown: connect a charger and check it yourself.".into());
    };
    match engine_running(rpm) {
        Some(true) if v < 12.8 => Gate::Block(format!("Only {v:.1} V with the engine running: the charging system is not keeping up.")),
        Some(true) if v < 13.2 => Gate::Warn(format!("{v:.1} V with the engine running is low for charging.")),
        Some(true) => Gate::Ok(format!("{v:.1} V, charging.")),
        _ if v < 11.8 => Gate::Block(format!("Battery is only {v:.1} V. Connect a charger before anything is written to a module.")),
        _ if v < 12.4 => Gate::Warn(format!("Battery is {v:.1} V. A charger is recommended before writing to a module.")),
        _ => Gate::Ok(format!("{v:.1} V.")),
    }
}

/// Fewer decimals for big numbers, more for small ones; whole numbers stay whole.
pub fn compact(v: f64) -> String {
    if v.fract() == 0.0 || v.abs() >= 1000.0 {
        return format!("{v:.0}");
    }
    let decimals = if v.abs() >= 100.0 { 0 } else if v.abs() >= 10.0 { 1 } else { 2 };
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// Round axis limits and tick count that put ticks on friendly numbers (800, 1000, 1200…).
/// Returns `(low, high, ticks)`; `ticks` includes both ends.
pub fn nice_axis(min: f64, max: f64) -> (f64, f64, usize) {
    let (min, max) = if (max - min).abs() < 1e-9 { (min - max.abs().max(1.0) * 0.1, max + max.abs().max(1.0) * 0.1) } else { (min, max) };
    let raw = (max - min) / 5.0;
    let pow = 10f64.powf(raw.log10().floor());
    let frac = raw / pow;
    let step = pow * if frac <= 1.0 { 1.0 } else if frac <= 2.0 { 2.0 } else if frac <= 5.0 { 5.0 } else { 10.0 };
    let low = (min / step).floor() * step;
    let high = (max / step).ceil() * step;
    (low, high, (((high - low) / step).round() as usize + 1).clamp(2, 12))
}

/// `1HGCM82633A••••••`: enough to recognise a car on screen, not enough to copy from a screenshot.
pub fn mask_vin(vin: &str) -> String {
    let keep = vin.chars().count().min(11);
    format!("{}••••••", vin.chars().take(keep).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_voltage_reads_differently_running_and_stopped() {
        assert_eq!(battery(12.1, Some(0.0)).1, Level::Warn);
        assert_eq!(battery(12.1, Some(850.0)).1, Level::Bad);
        assert_eq!(battery(14.2, Some(850.0)).1, Level::Good);
        assert_eq!(battery(15.6, Some(2000.0)).0, "Overcharging");
        assert_eq!(battery(12.6, Some(0.0)).1, Level::Good);
        assert_eq!(battery(12.6, None).1, Level::Unknown);
    }

    #[test]
    fn write_gate_blocks_low_supply_and_warns_when_unknown() {
        assert!(matches!(write_gate(Some(11.5), Some(0.0)), Gate::Block(_)));
        assert!(matches!(write_gate(Some(12.1), Some(0.0)), Gate::Warn(_)));
        assert!(matches!(write_gate(Some(12.6), Some(0.0)), Gate::Ok(_)));
        assert!(matches!(write_gate(Some(12.3), Some(900.0)), Gate::Block(_)));
        assert!(matches!(write_gate(Some(13.0), Some(900.0)), Gate::Warn(_)));
        assert!(matches!(write_gate(Some(14.1), Some(900.0)), Gate::Ok(_)));
        assert!(matches!(write_gate(None, Some(0.0)), Gate::Warn(_)));
        // engine speed unknown: judged as the battery alone
        assert!(matches!(write_gate(Some(11.0), None), Gate::Block(_)));
    }

    #[test]
    fn axis_ticks_land_on_friendly_numbers() {
        assert_eq!(nice_axis(780.0, 1230.0), (700.0, 1300.0, 7));
        let (lo, hi, n) = nice_axis(0.0, 6800.0);
        assert_eq!((lo, hi, n), (0.0, 8000.0, 5));
        let (lo, hi, _) = nice_axis(12.1, 14.3);
        assert!(lo <= 12.1 && hi >= 14.3);
        let (lo, hi, _) = nice_axis(5.0, 5.0);
        assert!(lo < 5.0 && hi > 5.0);
    }

    #[test]
    fn compact_keeps_small_values_precise() {
        assert_eq!(compact(0.98), "0.98");
        assert_eq!(compact(13.84), "13.8");
        assert_eq!(compact(92.0), "92");
        assert_eq!(compact(-3.1), "-3.1");
        assert_eq!(compact(2500.4), "2500");
    }
}
