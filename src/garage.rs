//! The user's vehicles: a name, a make (which selects manufacturer-specific trouble codes) and
//! optionally a VIN, so a car is recognised again when it is plugged in.
//! Stored in `<data dir>/garage.tsv` as `NAME<TAB>MAKE<TAB>VIN` lines (VIN may be empty).

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Vehicle {
    pub name: String,
    pub make: String,
    pub vin: Option<String>,
}

/// Who the car on the bench is, given what has just been read from it.
#[derive(Debug, PartialEq)]
pub enum Identity {
    /// A saved vehicle has this VIN.
    Known(usize),
    /// The selected vehicle has no VIN yet and fits: record the VIN on it.
    Attach(usize),
    /// Not in the garage, but the VIN names a make: add it.
    New(&'static str),
    /// Nothing to go on: the user has to say.
    Unknown,
}

pub fn identify(vehicles: &[Vehicle], current: Option<usize>, vin: &str) -> Identity {
    if let Some(ix) = vehicles.iter().position(|v| v.vin.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(vin))) {
        return Identity::Known(ix);
    }
    let detected = crate::vin::make(vin);
    if let Some(ix) = current.filter(|&ix| vehicles.get(ix).is_some_and(|v| v.vin.is_none())) {
        if detected.is_none_or(|m| m.eq_ignore_ascii_case(&vehicles[ix].make)) {
            return Identity::Attach(ix);
        }
    }
    detected.map_or(Identity::Unknown, Identity::New)
}

fn clean(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

pub fn load(dir: &Path) -> Vec<Vehicle> {
    let text = std::fs::read_to_string(dir.join("garage.tsv")).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (name, make) = (f.next()?.trim(), f.next()?.trim());
            let vin = f.next().map(str::trim).filter(|v| !v.is_empty()).map(str::to_ascii_uppercase);
            (!name.is_empty() && !make.is_empty()).then(|| Vehicle { name: name.into(), make: make.to_ascii_uppercase(), vin })
        })
        .collect()
}

pub fn save(dir: &Path, vehicles: &[Vehicle]) -> std::io::Result<()> {
    let lines: Vec<String> = vehicles
        .iter()
        .map(|v| format!("{}\t{}\t{}", clean(&v.name), clean(&v.make), v.vin.as_deref().map(clean).unwrap_or_default()))
        .collect();
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("garage.tsv"), lines.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(name: &str, make: &str, vin: Option<&str>) -> Vehicle {
        Vehicle { name: name.into(), make: make.into(), vin: vin.map(String::from) }
    }

    #[test]
    fn round_trips() {
        let d = std::env::temp_dir().join(format!("canforge-garage-{}", std::process::id()));
        let cars = vec![car("Fiesta 1.6 TDCi", "FORD", Some("WF0JXXWPJJAT64933")), car("Old Golf", "VOLKSWAGEN", None)];
        save(&d, &cars).unwrap();
        assert_eq!(load(&d), cars);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn identifies_by_vin_then_make() {
        let cars = vec![car("Fiesta", "FORD", Some("WF0JXXWPJJAT64933")), car("Golf", "VOLKSWAGEN", None)];
        assert_eq!(identify(&cars, None, "wf0jxxwpjjat64933"), Identity::Known(0));
        assert_eq!(identify(&cars, Some(1), "WVWZZZ1JZ3W386752"), Identity::Attach(1)); // same make, no VIN yet
        assert_eq!(identify(&cars, Some(1), "VF1BB05C123456789"), Identity::New("RENAULT")); // a different car
        assert_eq!(identify(&cars, Some(0), "VF1BB05C123456789"), Identity::New("RENAULT"));
        assert_eq!(identify(&cars, None, "XXXBB05C123456789"), Identity::Unknown);
        assert_eq!(identify(&cars, Some(1), "XXXBB05C123456789"), Identity::Attach(1)); // can't tell: assume it
    }
}
