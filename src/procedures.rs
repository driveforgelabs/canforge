//! Guided service procedures: a fixed order of instructions, live checks and module functions, so a
//! common job is done the same way every time. A step that runs a function uses the user's own
//! function list (the FORScan import) and is found by words in its title; when the car has no such
//! function the step says so and can be skipped, because the job may be done with another tool.
//! Nothing runs without the same confirmation as on the Functions page.

/// A condition on the live data that must hold before the procedure goes on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Need {
    EngineOff,
    EngineRunning,
    VoltageAtLeast(f64),
}

pub enum Step {
    /// Something the mechanic does; they press Done.
    Do(&'static str),
    /// Checked against live data.
    Require(Need),
    /// A module function. `module` words: any must be in the module's name (empty = any module).
    /// `function` words: all must be in the function's title.
    Run { module: &'static [&'static str], function: &'static [&'static str], label: &'static str },
}

pub struct Procedure {
    pub title: &'static str,
    pub summary: &'static str,
    pub steps: &'static [Step],
}

pub const ALL: &[Procedure] = &[
    Procedure {
        title: "Battery registration",
        summary: "After fitting a new battery: tell the car so charging is managed for the new one.",
        steps: &[
            Step::Do("Fit the new battery. Keep a maintainer connected while the old one is out so modules do not lose power."),
            Step::Require(Need::EngineOff),
            Step::Require(Need::VoltageAtLeast(12.0)),
            Step::Run { module: &["battery", "power", "bcm", "body", "gateway"], function: &["battery"], label: "Register the new battery" },
            Step::Do("Switch the ignition off for ten seconds, then on, and check the Fault codes page for new codes."),
        ],
    },
    Procedure {
        title: "DPF regeneration",
        summary: "Force a particulate filter regeneration when the soot load is high.",
        steps: &[
            Step::Do("Park outside on level ground. Engine at working temperature and more than a quarter tank of fuel. Nothing flammable under the car."),
            Step::Require(Need::VoltageAtLeast(12.4)),
            Step::Require(Need::EngineRunning),
            Step::Run { module: &[], function: &["regeneration"], label: "Start the regeneration" },
            Step::Do("Leave the engine idling until the procedure ends. Do not switch it off. Then re-check the codes."),
        ],
    },
    Procedure {
        title: "Oil service reset",
        summary: "Reset the service interval after an oil change.",
        steps: &[
            Step::Do("The oil and filter are changed and the level is correct."),
            Step::Require(Need::EngineOff),
            Step::Run { module: &[], function: &["oil"], label: "Reset the oil service interval" },
            Step::Do("Check the dashboard shows the new interval."),
        ],
    },
    Procedure {
        title: "Brake pad service (parking brake)",
        summary: "Retract an electric parking brake to change pads, then set it up again.",
        steps: &[
            Step::Do("Car on level ground, wheels chocked, ignition on, engine off, foot off the brake."),
            Step::Require(Need::EngineOff),
            Step::Require(Need::VoltageAtLeast(12.2)),
            Step::Run { module: &["brake", "parking", "abs", "esp"], function: &["parking", "release"], label: "Retract the parking brake motors" },
            Step::Do("Change the pads and pistons. Do not apply the parking brake while the pads are out."),
            Step::Run { module: &["brake", "parking", "abs", "esp"], function: &["parking", "apply"], label: "Close the parking brake and re-learn" },
            Step::Do("Pump the brake pedal until firm, then check the brake fluid level."),
        ],
    },
];

/// Whether `need` holds. `None` means the live data to decide is missing.
pub fn holds(need: Need, volts: Option<f64>, rpm: Option<f64>) -> Option<bool> {
    match need {
        Need::EngineOff => crate::health::engine_running(rpm).map(|r| !r),
        Need::EngineRunning => crate::health::engine_running(rpm),
        Need::VoltageAtLeast(min) => volts.map(|v| v >= min),
    }
}

/// The module and function (index in its list) that a [`Step::Run`] means on this car.
/// `titles(tx)` gives each function's `(title, runnable)` of that module.
pub fn find(
    module_words: &[&str],
    function_words: &[&str],
    modules: &[(u32, String)],
    titles: &dyn Fn(u32) -> Vec<(String, bool)>,
) -> Option<(u32, usize)> {
    modules
        .iter()
        .filter(|(_, name)| module_words.is_empty() || module_words.iter().any(|w| name.to_lowercase().contains(w)))
        .find_map(|(tx, _)| {
            titles(*tx).iter().position(|(t, runnable)| *runnable && function_words.iter().all(|w| t.to_lowercase().contains(w))).map(|i| (*tx, i))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_check_live_data() {
        assert_eq!(holds(Need::EngineOff, None, Some(0.0)), Some(true));
        assert_eq!(holds(Need::EngineOff, None, Some(900.0)), Some(false));
        assert_eq!(holds(Need::EngineRunning, None, None), None);
        assert_eq!(holds(Need::VoltageAtLeast(12.0), Some(11.9), None), Some(false));
        assert_eq!(holds(Need::VoltageAtLeast(12.0), None, None), None);
    }

    #[test]
    fn finds_function_in_matching_module() {
        let modules = vec![(0x7e0, "Engine control".to_string()), (0x760, "ABS and parking brake".to_string())];
        let titles = |tx: u32| match tx {
            0x7e0 => vec![("DPF regeneration".to_string(), true)],
            _ => vec![("Parking brake release".to_string(), false), ("Electric parking brake release".to_string(), true)],
        };
        assert_eq!(find(&[], &["regeneration"], &modules, &titles), Some((0x7e0, 0)));
        assert_eq!(find(&["brake"], &["parking", "release"], &modules, &titles), Some((0x760, 1)));
        assert_eq!(find(&["engine"], &["parking"], &modules, &titles), None);
    }

    #[test]
    fn every_procedure_ends_with_a_check_and_has_steps() {
        for p in ALL {
            assert!(p.steps.len() >= 3, "{}", p.title);
            assert!(matches!(p.steps.last(), Some(Step::Do(_))), "{}", p.title);
        }
    }
}
