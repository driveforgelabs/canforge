//! Likely causes and what to check next for a trouble code, as general workshop guidance. These
//! are starting points, not a diagnosis: the same code has different causes on different cars.
//! Specific codes come first; every other code falls back to its system (P00xx, P03xx, U1xxx…).

pub struct Hint {
    pub causes: &'static [&'static str],
    pub checks: &'static [&'static str],
}

/// (code, hint) for codes that are common enough to deserve their own advice.
const SPECIFIC: &[(&str, Hint)] = &[
    ("P0100", Hint { causes: &["Air flow sensor (MAF) dirty or failed", "Wiring or connector fault", "Intake leak after the sensor"], checks: &["Inspect the connector and wiring", "Compare MAF reading at idle with the specification", "Look for split intake hoses"] }),
    ("P0101", Hint { causes: &["Dirty or failing MAF sensor", "Air leak between sensor and engine", "Blocked air filter"], checks: &["Watch the MAF reading while revving: it should rise smoothly", "Check the air filter and intake hoses", "Clean the sensor only with MAF-safe cleaner"] }),
    ("P0113", Hint { causes: &["Intake air temperature sensor open circuit or failed", "Wiring or connector fault"], checks: &["Compare the sensor reading with ambient on a cold engine", "Check the connector for corrosion", "Measure the sensor's resistance against its table"] }),
    ("P0171", Hint { causes: &["Vacuum or intake leak", "Dirty MAF sensor", "Weak fuel pump or clogged filter", "Leaking injector"], checks: &["Look at long-term fuel trim at idle and at 2500 rpm: a leak shows at idle", "Smoke-test the intake", "Check fuel pressure"] }),
    ("P0172", Hint { causes: &["Leaking or stuck-open injector", "Fuel pressure too high", "Faulty oxygen sensor", "Dirty air filter"], checks: &["Check fuel trims and oxygen sensor behaviour", "Check fuel pressure and the pressure regulator", "Inspect the air filter"] }),
    ("P0217", Hint { causes: &["Overheating engine", "Low coolant", "Stuck thermostat or failed fan"], checks: &["Stop the engine and let it cool before opening anything", "Check coolant level and fan operation", "Compare the temperature reading with an infrared thermometer"] }),
    ("P0299", Hint { causes: &["Boost leak (hose, intercooler, clamp)", "Turbo actuator or wastegate fault", "Clogged DPF or air filter"], checks: &["Pressure-test the charge-air system", "Check the actuator moves freely", "Compare requested and actual boost"] }),
    ("P0300", Hint { causes: &["Worn spark plugs or coils", "Fuel delivery problem", "Vacuum leak", "Low compression"], checks: &["Read which cylinders misfire (P0301–P0306)", "Swap coils between cylinders and see if the misfire moves", "Check fuel pressure and trims"] }),
    ("P0335", Hint { causes: &["Crankshaft position sensor failed", "Damaged reluctor ring or wiring"], checks: &["Check the connector and sensor gap", "Measure the sensor signal with a scope", "Check engine speed reads while cranking"] }),
    ("P0401", Hint { causes: &["Blocked EGR passage", "EGR valve stuck", "Faulty differential pressure sensor"], checks: &["Command the EGR and watch the intake pressure change", "Inspect the EGR valve and pipes for carbon", "Check vacuum or electrical supply to the valve"] }),
    ("P0420", Hint { causes: &["Worn catalytic converter", "Exhaust leak before or around the sensors", "Failing downstream oxygen sensor"], checks: &["Compare upstream and downstream oxygen sensor traces", "Check for exhaust leaks", "Make sure there are no misfire codes first"] }),
    ("P0442", Hint { causes: &["Loose or damaged fuel cap", "Cracked EVAP hose", "Purge or vent valve leak"], checks: &["Check the fuel cap seal first", "Smoke-test the EVAP system", "Command the purge valve and listen for it"] }),
    ("P0455", Hint { causes: &["Fuel cap missing or loose", "Large EVAP leak or disconnected hose"], checks: &["Check the fuel cap", "Inspect EVAP hoses near the tank and canister"] }),
    ("P0500", Hint { causes: &["Vehicle speed sensor or wheel speed sensor fault", "Wiring fault", "ABS module problem"], checks: &["Compare wheel speeds with the live vehicle speed", "Check the ABS module for codes too"] }),
    ("P0562", Hint { causes: &["Weak battery", "Charging fault", "Poor battery or ground connections"], checks: &["Test the battery at rest and running", "Check alternator output (13.2–14.9 V running)", "Clean and tighten the terminals"] }),
    ("P0606", Hint { causes: &["Engine control unit internal fault", "Supply or ground problem to the ECU"], checks: &["Check the ECU supply voltages and grounds first", "Update software if a newer version exists"] }),
    ("P0700", Hint { causes: &["Transmission control module has stored its own code"], checks: &["Read the transmission module for its specific codes"] }),
    ("P2002", Hint { causes: &["DPF clogged or past its capacity", "Failed regeneration", "Differential pressure sensor or pipes blocked"], checks: &["Read soot loading and differential pressure", "Run a forced regeneration if conditions allow", "Check the pressure sensor pipes for blockage"] }),
    ("U0100", Hint { causes: &["Lost communication with the engine module", "Module without power or ground", "CAN bus wiring fault"], checks: &["Check fuses and grounds of the silent module", "Measure CAN-H/CAN-L resistance (about 60 Ω across the bus)", "Check battery voltage, since low voltage causes ghost U-codes"] }),
];

/// What a code of this system usually involves, by the first digits.
const BY_SYSTEM: &[(&str, Hint)] = &[
    ("P00", Hint { causes: &["Air, fuel or turbo control fault", "Sensor or wiring problem"], checks: &["Check connectors and wiring of the named sensor", "Compare its live reading with a known good value"] }),
    ("P01", Hint { causes: &["Fuel or air metering fault", "Sensor out of range"], checks: &["Check fuel trims, then the air and fuel sensors", "Look for vacuum leaks"] }),
    ("P02", Hint { causes: &["Injector circuit or fuel system fault"], checks: &["Test the injector circuit and wiring", "Check fuel pressure"] }),
    ("P03", Hint { causes: &["Ignition or misfire problem", "Crankshaft or camshaft sensor fault"], checks: &["Check plugs and coils", "Read the per-cylinder misfire codes", "Check compression if the misfire stays"] }),
    ("P04", Hint { causes: &["Emission control fault (EGR, catalyst, EVAP, secondary air)"], checks: &["Check for leaks in the named system", "Compare sensor readings before and after the component"] }),
    ("P05", Hint { causes: &["Speed, idle control or auxiliary input fault"], checks: &["Check the related sensor and its wiring", "Check battery and charging"] }),
    ("P06", Hint { causes: &["Control module or its supply fault"], checks: &["Check supply voltage and grounds to the module", "Check for software updates"] }),
    ("P07", Hint { causes: &["Transmission fault"], checks: &["Check fluid level and condition", "Read the transmission module's own codes"] }),
    ("P2", Hint { causes: &["Sensor, actuator or after-treatment fault (DPF, SCR, EGR, turbo)"], checks: &["Check the named component's wiring and live values", "Check for blockages and leaks"] }),
    ("U", Hint { causes: &["Lost communication between modules", "Low battery voltage", "CAN bus wiring fault"], checks: &["Check battery voltage first", "Check fuses and grounds of the silent module", "Inspect the CAN wiring"] }),
    ("C", Hint { causes: &["Chassis fault: wheel speed sensor, brake or steering sensor"], checks: &["Check wheel speed sensor wiring and air gap", "Compare wheel speeds with each other"] }),
    ("B", Hint { causes: &["Body or comfort fault: lamp, lock, seat, airbag circuit"], checks: &["Check the named circuit's connector and fuse", "Never probe airbag circuits with a test lamp"] }),
];

/// Advice for `code` (`P0113`, optionally with a `-1C` failure-type suffix).
pub fn lookup(code: &str) -> Option<&'static Hint> {
    let code = code.get(..5).unwrap_or(code).to_ascii_uppercase();
    if let Some((_, h)) = SPECIFIC.iter().find(|(c, _)| *c == code) {
        return Some(h);
    }
    BY_SYSTEM.iter().filter(|(p, _)| code.starts_with(p)).max_by_key(|(p, _)| p.len()).map(|(_, h)| h)
}

/// Whether the advice is for this exact code rather than its system.
pub fn is_specific(code: &str) -> bool {
    let code = code.get(..5).unwrap_or(code).to_ascii_uppercase();
    SPECIFIC.iter().any(|(c, _)| *c == code)
}


/// How urgent a fault is for the car's owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Driving on can cause serious damage or is unsafe.
    Stop,
    /// Drivability, safety systems or damage over time: fix soon.
    Soon,
    /// Mostly emissions: can wait for a convenient visit.
    Monitor,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Stop => "Stop driving",
            Severity::Soon => "Fix soon",
            Severity::Monitor => "Can wait",
        }
    }
}

fn number(code: &str) -> Option<(char, u32)> {
    let code = code.get(..5).unwrap_or(code).to_ascii_uppercase();
    let letter = code.chars().next()?;
    // the digits are decimal for the codes this ranks (P0xxx, P2xxx, C0xxx, B0xxx, U0xxx)
    Some((letter, code.get(1..)?.parse().ok()?))
}

/// How urgent `code` is, and why, in a sentence the customer can read.
pub fn severity(code: &str) -> (Severity, &'static str) {
    let Some((letter, n)) = number(code) else { return (Severity::Soon, "Have it looked at soon.") };
    match (letter, n) {
        ('P', 217 | 218) => (Severity::Stop, "The engine is overheating: driving on can wreck it."),
        ('P', 520..=524) => (Severity::Stop, "Low oil pressure: driving on can wreck the engine."),
        ('P', 300..=312) => (Severity::Soon, "A misfire wastes fuel and can damage the catalytic converter."),
        ('P', 100..=114 | 120..=124 | 170..=175) => (Severity::Soon, "The engine is running on wrong readings: poor running and higher fuel use."),
        ('P', 335..=339) => (Severity::Soon, "The engine may stall or not restart."),
        ('P', 562 | 563) => (Severity::Soon, "The battery or charging system is not keeping up."),
        ('P', 299 | 2002 | 2003) => (Severity::Soon, "Loss of power and a clogged filter can follow."),
        ('P', 606 | 700) => (Severity::Soon, "A control unit reports a fault of its own."),
        ('P', 420 | 430 | 440..=457 | 400..=409) => (Severity::Monitor, "An emissions fault: the car drives normally but may fail inspection."),
        ('C', _) => (Severity::Soon, "A brake or chassis system is affected: have it checked before long trips."),
        ('B', 1..=99) => (Severity::Soon, "An airbag or restraint circuit is affected: it may not work in a crash."),
        ('B', _) => (Severity::Monitor, "A comfort or body function is affected."),
        ('U', _) => (Severity::Soon, "Modules are not talking to each other: often a weak battery or a wiring fault."),
        _ => (Severity::Soon, "Have it looked at soon."),
    }
}

/// Codes that tend to share one cause, and where each sits in the chain of cause and effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    pub id: &'static str,
    pub title: &'static str,
    /// 0 is the usual root cause; a larger number is more often a consequence of the lower ones.
    pub rank: u8,
}

pub fn group(code: &str) -> Option<Group> {
    let (letter, n) = number(code)?;
    let g = |id, title, rank| Some(Group { id, title, rank });
    match (letter, n) {
        ('P', 100..=114 | 120..=124) => g("air-fuel", "Air and fuel metering", 0),
        ('P', 170..=175) => g("air-fuel", "Air and fuel metering", 1),
        ('P', 300..=312) => g("air-fuel", "Air and fuel metering", 2),
        ('P', 130..=167 | 420 | 430) => g("exhaust", "Exhaust and emissions", 3),
        ('P', 400..=409 | 440..=457) => g("exhaust", "Exhaust and emissions", 3),
        ('P', 562 | 563 | 620..=630) => g("charging", "Battery and charging", 0),
        ('U', _) => g("comms", "Module communication", 1),
        ('C', _) => g("chassis", "Brakes and chassis", 1),
        ('B', _) => g("body", "Body and airbags", 1),
        _ => None,
    }
}

/// Sort key for a fault list: what is urgent first, then the likely root cause before what it
/// probably set off, stored before pending. With a weak battery, communication codes come first:
/// low voltage makes modules drop out and set codes that are not real faults.
pub fn order_key(code: &str, pending: bool, battery_bad: bool) -> (u8, u8, bool) {
    let (sev, _) = severity(code);
    let rank = match group(code) {
        Some(g) if battery_bad && g.id == "comms" => 0,
        Some(g) => g.rank,
        None => 5,
    };
    let urgent = if battery_bad && group(code).is_some_and(|g| g.id == "comms") { 0 } else { sev as u8 };
    (urgent, rank, pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specific_code_beats_system() {
        let h = lookup("P0113-1C").expect("known");
        assert!(h.causes[0].contains("temperature"));
        assert!(is_specific("p0113"));
    }

    #[test]
    fn unknown_code_falls_back_to_its_system_and_longest_prefix_wins() {
        assert!(lookup("P0399").unwrap().causes[0].contains("Ignition"));
        assert!(lookup("P2999").unwrap().causes[0].contains("after-treatment"));
        assert!(lookup("U1234").unwrap().causes.iter().any(|c| c.contains("CAN")));
        assert!(!is_specific("P0399"));
    }

    #[test]
    fn nothing_for_garbage() {
        assert!(lookup("").is_none());
        assert!(lookup("X9999").is_none());
    }

    #[test]
    fn every_specific_entry_has_checks() {
        for (c, h) in SPECIFIC {
            assert!(!h.causes.is_empty() && !h.checks.is_empty(), "{c}");
        }
    }

    #[test]
    fn root_causes_sort_before_their_consequences() {
        let mut codes = vec!["P0420", "P0300", "P0171", "P0113"];
        codes.sort_by_key(|c| order_key(c, false, false));
        assert_eq!(codes, vec!["P0113", "P0171", "P0300", "P0420"]);
    }

    #[test]
    fn overheating_is_urgent_and_emissions_can_wait() {
        assert_eq!(severity("P0217").0, Severity::Stop);
        assert_eq!(severity("P0420").0, Severity::Monitor);
        assert_eq!(severity("P0300").0, Severity::Soon);
        assert!(order_key("P0217", false, false) < order_key("P0113", false, false));
    }

    #[test]
    fn a_weak_battery_puts_communication_codes_first() {
        assert!(order_key("U0100", false, true) < order_key("P0217", false, true));
        assert!(order_key("U0100", false, false) > order_key("P0113", false, false));
    }

    #[test]
    fn related_codes_share_a_group() {
        assert_eq!(group("P0101-00").map(|g| g.id), group("P0171").map(|g| g.id));
        assert_eq!(group("P0300").map(|g| g.id), Some("air-fuel"));
        assert!(group("P0606").is_none());
    }
}
