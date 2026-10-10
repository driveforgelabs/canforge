//! `canforge db status|update`: manage the vehicle-db data used for trouble code descriptions.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::dtcdb;

const DEFAULT_SOURCE: &str = "https://github.com/driveforgelabs/vehicle-db/releases/latest/download";

#[derive(Deserialize)]
struct Manifest {
    schema: u32,
    version: String,
    files: std::collections::BTreeMap<String, FileInfo>,
}

#[derive(Deserialize)]
struct FileInfo {
    sha256: String,
    rows: usize,
}

pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("status") => {
            status();
            Ok(())
        }
        Some("update") => {
            let from = args.windows(2).find(|w| w[0] == "--from").map(|w| w[1].as_str());
            let dir = dtcdb::data_dir().ok_or("cannot find a data directory (set CANFORGE_DATA_DIR)")?;
            let (version, rows) = update(from.unwrap_or(DEFAULT_SOURCE), &dir)?;
            println!("updated to {version} ({rows} rows) in {}", dir.display());
            Ok(())
        }
        _ => Err("usage: canforge db status | canforge db update [--from URL_OR_DIRECTORY]".into()),
    }
}

fn status() {
    let db = dtcdb::get();
    println!("trouble code data: {} ({} rows)", db.version, db.len());
    match &db.origin {
        dtcdb::Origin::Embedded => println!("  from: snapshot built into canforge (run `canforge db update` for the latest)"),
        dtcdb::Origin::Downloaded(p) => println!("  from: {}", p.display()),
    }
    match dtcdb::data_dir() {
        Some(d) => println!("  your own files: {}/dtc.d/*.tsv ({} rows loaded)", d.display(), db.user_rows),
        None => println!("  no data directory available"),
    }
}

/// Fetches `manifest.json` and `dtc.tsv` from `source`, checks size and SHA-256 against the
/// manifest, and replaces `<dir>/dtc.tsv` atomically. Nothing is written if any check fails.
pub fn update(source: &str, dir: &Path) -> Result<(String, usize), String> {
    let base = normalise(source)?;
    let manifest_bytes = fetch(&format!("{base}/manifest.json"))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|e| format!("bad manifest.json: {e}"))?;
    if manifest.schema != 1 {
        return Err(format!("unsupported data schema {} (this canforge understands 1): update canforge", manifest.schema));
    }
    if !manifest.files.contains_key("dtc.tsv") {
        return Err("manifest.json does not list dtc.tsv".into());
    }

    // fetch and verify everything first, so a bad file leaves the installed data untouched
    let mut verified: Vec<(&str, String)> = Vec::new();
    let mut dtc_rows = 0;
    for name in ["dtc.tsv", "modules.tsv"] {
        let Some(info) = manifest.files.get(name) else { continue };
        let data = fetch(&format!("{base}/{name}"))?;
        let digest: String = Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
        if !digest.eq_ignore_ascii_case(&info.sha256) {
            return Err(format!("checksum mismatch for {name}: expected {}, got {digest}", info.sha256));
        }
        let text = String::from_utf8(data).map_err(|_| format!("{name} is not valid UTF-8"))?;
        let rows = dtcdb::rows(&text).count();
        if rows != info.rows || (rows == 0 && name == "dtc.tsv") {
            return Err(format!("{name} has {rows} rows but the manifest says {}", info.rows));
        }
        if name == "dtc.tsv" {
            dtc_rows = rows;
        }
        verified.push((name, text));
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (name, text) in &verified {
        let tmp = dir.join(format!("{name}.part"));
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, dir.join(name)).map_err(|e| format!("replacing {name}: {e}"))?;
    }
    Ok((manifest.version, dtc_rows))
}

/// `https://…` as is, `file://…` as is, a plain directory path → `file://`. Plain `http://` is refused.
fn normalise(source: &str) -> Result<String, String> {
    let s = source.trim_end_matches('/');
    if s.starts_with("https://") || s.starts_with("file://") {
        Ok(s.to_string())
    } else if s.starts_with("http://") {
        Err("refusing plain http://: use https://".into())
    } else if Path::new(s).is_dir() {
        let abs = std::fs::canonicalize(s).map_err(|e| format!("{s}: {e}"))?;
        Ok(format!("file://{}", abs.display()))
    } else {
        Err(format!("{source}: not an https:// URL or an existing directory"))
    }
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    if let Some(path) = url.strip_prefix("file://") {
        return std::fs::read(PathBuf::from(path)).map_err(|e| format!("{path}: {e}"));
    }
    let out = std::process::Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location", "--proto", "=https", "--max-time", "60", url])
        .output()
        .map_err(|e| format!("could not run curl ({e}); install curl or use --from <directory>"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!("download failed: {}: {}", url, String::from_utf8_lossy(&out.stderr).trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist(tag: &str, text: &str, sha: Option<&str>, rows: usize) -> PathBuf {
        let d = std::env::temp_dir().join(format!("canforge-db-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("dtc.tsv"), text).unwrap();
        let digest: String = Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        let manifest = format!(
            r#"{{"schema":1,"version":"v1.2.3","files":{{"dtc.tsv":{{"sha256":"{}","rows":{rows},"bytes":{}}}}}}}"#,
            sha.unwrap_or(&digest),
            text.len()
        );
        std::fs::write(d.join("manifest.json"), manifest).unwrap();
        d
    }

    #[test]
    fn installs_module_names_too_when_the_release_has_them() {
        let src = dist("mods", GOOD, None, 2);
        let mods = "# vehicle-db v1.2.3\n720\tFORD\tInstrument cluster\n";
        std::fs::write(src.join("modules.tsv"), mods).unwrap();
        let digest: String = Sha256::digest(mods.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        let manifest = std::fs::read_to_string(src.join("manifest.json")).unwrap();
        let manifest = manifest.replace("}}}", &format!(r#"}},"modules.tsv":{{"sha256":"{digest}","rows":1,"bytes":{}}}}}}}"#, mods.len()));
        std::fs::write(src.join("manifest.json"), manifest).unwrap();
        let dest = std::env::temp_dir().join(format!("canforge-db-{}-dest5", std::process::id()));
        update(src.to_str().unwrap(), &dest).unwrap();
        let names = crate::modnames::ModuleNames::load(Some(&dest));
        assert_eq!(names.name(0x720, Some("FORD")), Some("Instrument cluster"));
        // a corrupt modules.tsv blocks the whole update, dtc.tsv included
        std::fs::write(src.join("modules.tsv"), "tampered\n").unwrap();
        let dest2 = std::env::temp_dir().join(format!("canforge-db-{}-dest6", std::process::id()));
        assert!(update(src.to_str().unwrap(), &dest2).unwrap_err().contains("modules.tsv"));
        assert!(!dest2.join("dtc.tsv").exists());
        std::fs::remove_dir_all(src).ok();
        std::fs::remove_dir_all(dest).ok();
    }

    const GOOD: &str = "# vehicle-db v1.2.3: CODE\tMAKE\tDESCRIPTION\nP0113\tGENERIC\tIntake air temperature circuit high\nP1690\tFORD\tWastegate\n";

    #[test]
    fn installs_a_verified_release() {
        let src = dist("ok", GOOD, None, 2);
        let dest = std::env::temp_dir().join(format!("canforge-db-{}-dest1", std::process::id()));
        let (version, rows) = update(src.to_str().unwrap(), &dest).unwrap();
        assert_eq!((version.as_str(), rows), ("v1.2.3", 2));
        let db = dtcdb::DtcDb::load(Some(&dest));
        assert_eq!(db.version, "v1.2.3");
        assert_eq!(db.len(), 2);
        std::fs::remove_dir_all(src).ok();
        std::fs::remove_dir_all(dest).ok();
    }

    #[test]
    fn rejects_a_checksum_mismatch_and_writes_nothing() {
        let src = dist("sum", GOOD, Some(&"0".repeat(64)), 2);
        let dest = std::env::temp_dir().join(format!("canforge-db-{}-dest2", std::process::id()));
        let err = update(src.to_str().unwrap(), &dest).unwrap_err();
        assert!(err.contains("checksum mismatch"), "{err}");
        assert!(!dest.join("dtc.tsv").exists());
        std::fs::remove_dir_all(src).ok();
    }

    #[test]
    fn rejects_a_row_count_mismatch() {
        let src = dist("rows", GOOD, None, 99);
        let dest = std::env::temp_dir().join(format!("canforge-db-{}-dest3", std::process::id()));
        assert!(update(src.to_str().unwrap(), &dest).unwrap_err().contains("rows"));
        std::fs::remove_dir_all(src).ok();
    }

    #[test]
    fn refuses_insecure_or_unknown_sources() {
        let dest = std::env::temp_dir().join("canforge-db-unused");
        assert!(update("http://example.com/x", &dest).unwrap_err().contains("https"));
        assert!(update("/definitely/not/a/dir", &dest).is_err());
    }

    #[test]
    fn a_failed_update_keeps_the_previous_data() {
        let src = dist("keep", GOOD, None, 2);
        let dest = std::env::temp_dir().join(format!("canforge-db-{}-dest4", std::process::id()));
        update(src.to_str().unwrap(), &dest).unwrap();
        std::fs::write(src.join("dtc.tsv"), "tampered\n").unwrap();
        assert!(update(src.to_str().unwrap(), &dest).is_err());
        assert_eq!(dtcdb::DtcDb::load(Some(&dest)).version, "v1.2.3");
        std::fs::remove_dir_all(src).ok();
        std::fs::remove_dir_all(dest).ok();
    }
}
