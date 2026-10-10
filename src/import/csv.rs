//! Tolerant importer for data-logger CSV exports.
//!
//! The first non-empty line is the header. The delimiter (`,` `;` or tab) is detected.
//! A column whose name contains "time" is the time axis (seconds, milliseconds or
//! `HH:MM:SS.mmm`); without one, rows are spaced one second apart. Every other column with
//! numeric cells becomes a channel; a unit in the header (`Engine speed [rpm]`,
//! `Coolant (°C)`) is split off the name.

use crate::session::{Channel, Session};

pub fn parse(text: &str, source: &str) -> Result<Session, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines.next().ok_or_else(|| format!("{source}: empty file"))?;
    let delim = [',', ';', '\t']
        .into_iter()
        .max_by_key(|d| header_line.matches(*d).count())
        .filter(|d| header_line.contains(*d))
        .ok_or_else(|| format!("{source}: not a CSV file (no delimiter in header)"))?;
    let header = split(header_line, delim);
    let rows: Vec<Vec<String>> = lines.map(|l| split(l, delim)).collect();
    if rows.is_empty() {
        return Err(format!("{source}: header only, no data rows"));
    }

    let time_col = header.iter().position(|h| {
        let h = h.to_lowercase();
        h.contains("time") || h == "t"
    });
    let decimal_comma = delim == ';';
    let num = |s: &str| -> Option<f64> {
        let s = s.trim();
        if decimal_comma { s.replace(',', ".").parse().ok() } else { s.parse().ok() }
    };

    // time axis in seconds
    let mut times: Vec<f64> = match time_col {
        Some(c) => rows.iter().enumerate().map(|(i, r)| r.get(c).and_then(|s| clock(s).or_else(|| num(s))).unwrap_or(i as f64)).collect(),
        None => (0..rows.len()).map(|i| i as f64).collect(),
    };
    if let Some(c) = time_col {
        let h = header[c].to_lowercase();
        let is_ms = h.contains("ms") || h.contains("milli") || {
            let mut d: Vec<f64> = times.windows(2).map(|w| (w[1] - w[0]).abs()).filter(|d| *d > 0.0).collect();
            d.sort_by(f64::total_cmp);
            !h.contains("[s]") && !h.contains("(s)") && d.get(d.len() / 2).is_some_and(|m| *m >= 20.0) && !rows[0][c].contains(':')
        };
        if is_ms {
            times.iter_mut().for_each(|t| *t /= 1000.0);
        }
    }

    let mut session = Session::new(source);
    for (col, title) in header.iter().enumerate().filter(|(c, _)| Some(*c) != time_col) {
        let (name, unit) = split_unit(title);
        if name.is_empty() {
            continue;
        }
        let mut channel = None;
        for (row, t) in rows.iter().zip(&times) {
            if let Some(v) = row.get(col).and_then(|s| num(s)) {
                let ch = *channel.get_or_insert_with(|| {
                    session.channel(&format!("csv:{col}"), || Channel::new(format!("csv:{col}"), name, unit))
                });
                session.push(*t, ch, v);
            }
        }
    }
    if session.readings.is_empty() {
        return Err(format!("{source}: no numeric columns found"));
    }
    Ok(session.finish())
}

fn split(line: &str, delim: char) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    for c in line.trim_end_matches(['\r', '\n']).chars() {
        match c {
            '"' => quoted = !quoted,
            c if c == delim && !quoted => out.push(std::mem::take(&mut cur).trim().to_string()),
            c => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// `Name [unit]` / `Name (unit)` → (`Name`, `unit`).
fn split_unit(title: &str) -> (&str, &str) {
    let title = title.trim();
    for (open, close) in [('[', ']'), ('(', ')')] {
        if let (Some(o), true) = (title.rfind(open), title.ends_with(close)) {
            return (title[..o].trim(), title[o + 1..title.len() - 1].trim());
        }
    }
    (title, "")
}

/// `HH:MM:SS(.fff)` → seconds.
fn clock(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    parts.iter().try_fold(0.0, |acc, p| p.replace(',', ".").parse::<f64>().ok().map(|v| acc * 60.0 + v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(s: &Session) -> Vec<(&str, &str)> {
        s.channels.iter().map(|c| (c.name.as_str(), c.unit.as_str())).collect()
    }

    #[test]
    fn comma_csv_with_units_in_headers() {
        let s = parse("Time (s),Engine speed [rpm],Coolant (°C)\n0.0,800,20\n0.5,1500,21\n1.0,2200,\n", "a.csv").unwrap();
        assert_eq!(keys(&s), [("Engine speed", "rpm"), ("Coolant", "°C")]);
        assert_eq!(s.readings.len(), 5); // the empty cell is skipped
        assert_eq!(s.duration(), 1.0);
    }

    #[test]
    fn milliseconds_detected_from_step_size() {
        let s = parse("Time,RPM\n1000,800\n1100,900\n1200,1000\n", "a.csv").unwrap();
        assert!((s.duration() - 0.2).abs() < 1e-9, "{}", s.duration());
    }

    #[test]
    fn semicolon_with_decimal_comma_and_clock_time() {
        let s = parse("\u{feff}Time;Boost [bar]\n12:00:00,5;1,25\n12:00:01,0;1,75\n", "a.csv").unwrap();
        assert_eq!(s.duration(), 0.5);
        assert!((s.readings[1].value - 1.75).abs() < 1e-9);
    }

    #[test]
    fn no_time_column_uses_one_second_rows() {
        let s = parse("a,b\n1,2\n3,4\n5,6\n", "a.csv").unwrap();
        assert_eq!(s.duration(), 2.0);
        assert_eq!(s.channels.len(), 2);
    }

    #[test]
    fn rejects_non_csv() {
        assert!(parse("hello world\n", "a.txt").is_err());
        assert!(parse("a,b\n", "a.csv").is_err());
        assert!(parse("a,b\nx,y\n", "a.csv").is_err());
    }
}
