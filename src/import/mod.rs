//! Importers: turn files from other diagnostic tools into a [`Session`].
//!
//! - [`csv`]: data-logger exports (FORScan, DDT4ALL, spreadsheets…)
//! - [`canlog`]: raw CAN captures (`candump -L`, Vector ASC, `candriver monitor`)
//! - [`ddt`]: DDT4ALL ECU definitions, used to decode CAN captures into named values

pub mod canlog;
pub mod csv;
pub mod ddt;

use std::path::Path;

use crate::session::Session;

/// Loads `path` as a session. CAN captures are decoded as OBD-II and with any `ecus`.
pub fn load(path: &Path, ecus: &[ddt::Ecu]) -> Result<Session, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let frames = canlog::parse(&text);
    if frames.len() >= 2 {
        let session = canlog::decode(&frames, ecus, &name);
        if session.readings.is_empty() {
            return Err(format!(
                "{name}: {} CAN frames, but none decode to values (OBD-II replies on 7E8–7EF, or pass --ddt ECU definitions)",
                frames.len()
            ));
        }
        return Ok(session);
    }
    csv::parse(&text, &name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, text: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("canforge-test-{}-{name}", std::process::id()));
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn dispatches_on_content_not_extension() {
        let csv = write("a.log", "Time (s),RPM\n0,800\n1,900\n");
        let s = load(&csv, &[]).unwrap();
        assert_eq!(s.channels[0].name, "RPM");

        let can = write("b.csv", "(1.0) can0 7E8#04410C1AF8\n(2.0) can0 7E8#04410C2000\n");
        let s = load(&can, &[]).unwrap();
        assert_eq!(s.channels[0].name, "Engine speed");
        std::fs::remove_file(csv).ok();
        std::fs::remove_file(can).ok();
    }

    #[test]
    fn can_capture_without_decodable_traffic_explains_why() {
        let p = write("c.log", "(1.0) can0 240#0080\n(2.0) can0 240#0081\n");
        let err = load(&p, &[]).unwrap_err();
        assert!(err.contains("--ddt"), "{err}");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn decodes_a_capture_with_ecu_definitions() {
        let ecu = ddt::Ecu::from_json(
            r#"{"ecuname":"EDC","obd":{"recv_id":"7E8"},
                "requests":[{"name":"Data","replybytes":"6101","receivebyte_dataitems":{"Rail":{"firstbyte":3}}}],
                "data":{"Rail":{"bitscount":16,"scaled":true,"divideby":10,"unit":"bar"}}}"#,
        )
        .unwrap();
        // multi-frame ISO-TP reply (9 bytes): first frame carries 61 01 04 B0 + 2, one consecutive frame the rest
        let p = write(
            "d.log",
            "(1.0) can0 7E8#1009610104B00000\n(1.1) can0 7E8#21000000\n(2.0) can0 7E8#10096101 05DC0000\n(2.1) can0 7E8#21000000\n".replace("1 05DC", "105DC").as_str(),
        );
        let s = load(&p, &[ecu]).unwrap();
        assert_eq!((s.channels[0].name.as_str(), s.channels[0].unit.as_str()), ("EDC · Rail", "bar"));
        assert_eq!(s.readings.iter().map(|r| r.value).collect::<Vec<_>>(), [120.0, 150.0]);
        std::fs::remove_file(p).ok();
    }
}
