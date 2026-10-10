//! A complete, local record of what the connected vehicle revealed: for the user's own analysis,
//! not a contribution (those are written by `contribute`). It includes the VIN, so keep it private.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::backend::{DidLine, DtcEntry, FreezeFrame, ModuleView, RoutineScan};
use crate::session::Channel;
use candriver_obd::pid;
use candriver_obd::readiness::Readiness;

pub struct Snapshot<'a> {
    pub name: Option<&'a str>,
    pub make: Option<&'a str>,
    pub vin: Option<&'a str>,
    pub adapter: &'a str,
    pub ecu: Option<u32>,
    pub mil_on: bool,
    pub channels: &'a [Channel],
    pub readings: &'a BTreeMap<String, f64>,
    pub codes: Vec<&'a DtcEntry>,
    pub readiness: Option<&'a Readiness>,
    pub freeze: Option<&'a FreezeFrame>,
    pub modules: &'a [ModuleView],
    pub dids: &'a BTreeMap<u32, Vec<DidLine>>,
    /// `None` when the routine scan was not requested.
    pub routines: Option<&'a BTreeMap<u32, RoutineScan>>,
}

pub fn build(s: &Snapshot, unix_time: u64) -> Value {
    let modules: Vec<Value> = s
        .modules
        .iter()
        .map(|m| {
            json!({
                "request_id": format!("{:03X}", m.tx_id),
                "response_id": format!("{:03X}", m.rx_id),
                "identity": m.identity.iter().map(|(k, v)| json!({"name": k, "value": v})).collect::<Vec<_>>(),
                "trouble_codes": m.dtcs.iter().map(|d| json!({"code": d.code, "status": d.status})).collect::<Vec<_>>(),
                "trouble_codes_note": m.dtc_note,
                // diagnostic services the module accepted, refused or ignored when probed (read-only probes)
                "services": m.services.iter().map(|(n, s)| json!({"service": n, "support": s})).collect::<Vec<_>>(),
                "data_identifiers": s.dids.get(&m.tx_id).map(|l| {
                    l.iter().map(|d| json!({"did": format!("{:04X}", d.did), "name": d.name, "value": d.value})).collect::<Vec<_>>()
                }),
                // routine identifiers that exist (none were started), with how the module answered
                "routines": s.routines.map(|r| r.get(&m.tx_id).map(|scan| json!({
                    "scan": scan.end,
                    "complete": scan.complete,
                    "resume_from": scan.next.map(|n| format!("{n:04X}")),
                    "found": scan.lines.iter().map(|l| json!({"routine": format!("{:04X}", l.id), "state": l.state})).collect::<Vec<_>>(),
                }))),
            })
        })
        .collect();
    json!({
        "format": "canforge-vehicle-dump/1",
        "created_unix": unix_time,
        "vehicle": {"name": s.name, "make": s.make, "vin": s.vin},
        "adapter": s.adapter,
        "engine_ecu": s.ecu.map(|e| format!("{e:03X}")),
        "mil_on": s.mil_on,
        "obd_parameters": s.channels.iter().map(|c| json!({
            "key": c.key, "name": c.name, "unit": c.unit, "min": c.min, "max": c.max,
            "last_value": s.readings.get(&c.key),
        })).collect::<Vec<_>>(),
        "obd_trouble_codes": s.codes.iter().map(|c| json!({"code": c.code.to_string(), "kind": c.kind, "ecu": format!("{:03X}", c.ecu)})).collect::<Vec<_>>(),
        "readiness": s.readiness.map(|r| r.monitors.iter().map(|m| json!({"monitor": m.name, "supported": m.supported, "complete": m.complete})).collect::<Vec<_>>()),
        "freeze_frame": s.freeze.map(|f| json!({
            "code": f.dtc.to_string(),
            "values": f.values.iter().map(|(p, v)| json!({
                "pid": format!("{p:02X}"),
                "name": pid::info(*p).map(|i| i.name),
                "unit": pid::info(*p).map(|i| i.unit),
                "value": v,
            })).collect::<Vec<_>>(),
        })),
        "modules": modules,
        "not_included": "What each routine does, its input parameters, and input/output control and write functions cannot be learned without commands that change the vehicle; canforge only reads and never starts a routine.",
    })
}

/// Writes the dump into `<dir>/export/` and returns its path.
pub fn write(dir: &Path, label: &str, value: &Value, unix_time: u64) -> Result<PathBuf, String> {
    let out = dir.join("export");
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let label: String = label.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let path = out.join(format!("{}-{unix_time}.json", label.trim_matches('-')));
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_named_json_file() {
        let d = std::env::temp_dir().join(format!("canforge-dump-{}", std::process::id()));
        let path = write(&d, "Golf VII 2.0 TDI", &json!({"a": 1}), 42).unwrap();
        assert!(path.ends_with("export/golf-vii-2-0-tdi-42.json"));
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"a\": 1"));
        std::fs::remove_dir_all(d).ok();
    }
}

/// Routine scan progress is kept per vehicle so a stopped scan can resume later.
pub mod progress {
    use super::*;

    fn path(dir: &Path, key: &str) -> PathBuf {
        let key: String = key.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
        dir.join("routine-scans").join(format!("{key}.json"))
    }

    pub fn clear(dir: &Path, key: &str) {
        let _ = std::fs::remove_file(path(dir, key));
    }

    pub fn save(dir: &Path, key: &str, scans: &BTreeMap<u32, RoutineScan>) -> Result<(), String> {
        let value: BTreeMap<String, Value> = scans
            .iter()
            .map(|(tx, s)| {
                (
                    format!("{tx:03X}"),
                    json!({
                        "end": s.end, "complete": s.complete, "next": s.next,
                        "found": s.lines.iter().map(|l| json!([l.id, l.state])).collect::<Vec<_>>(),
                    }),
                )
            })
            .collect();
        let p = path(dir, key);
        std::fs::create_dir_all(p.parent().expect("has a parent")).map_err(|e| e.to_string())?;
        std::fs::write(&p, serde_json::to_string(&value).map_err(|e| e.to_string())?).map_err(|e| format!("{}: {e}", p.display()))
    }

    pub fn load(dir: &Path, key: &str) -> BTreeMap<u32, RoutineScan> {
        let Ok(text) = std::fs::read_to_string(path(dir, key)) else { return BTreeMap::new() };
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) else { return BTreeMap::new() };
        map.into_iter()
            .filter_map(|(tx, v)| {
                let lines = v["found"]
                    .as_array()?
                    .iter()
                    .filter_map(|l| Some(crate::backend::RoutineLine { id: l[0].as_u64()? as u16, state: l[1].as_str()?.to_string() }))
                    .collect();
                Some((
                    u32::from_str_radix(&tx, 16).ok()?,
                    RoutineScan {
                        lines,
                        end: v["end"].as_str().unwrap_or_default().to_string(),
                        complete: v["complete"].as_bool().unwrap_or(false),
                        next: v["next"].as_u64().map(|n| n as u16),
                    },
                ))
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::backend::RoutineLine;

        #[test]
        fn saved_progress_round_trips() {
            let d = std::env::temp_dir().join(format!("canforge-scan-{}", std::process::id()));
            let mut scans = BTreeMap::new();
            scans.insert(
                0x7e0,
                RoutineScan { lines: vec![RoutineLine { id: 0x0203, state: "exists, never started".into() }], end: "stopped at 0400".into(), complete: false, next: Some(0x400) },
            );
            assert!(load(&d, "WF0 123").is_empty());
            save(&d, "WF0 123", &scans).unwrap();
            let back = load(&d, "WF0 123");
            assert_eq!(back[&0x7e0].next, Some(0x400));
            assert_eq!(back[&0x7e0].lines[0].id, 0x203);
            clear(&d, "WF0 123");
            assert!(load(&d, "WF0 123").is_empty());
            std::fs::remove_dir_all(d).ok();
        }
    }
}
