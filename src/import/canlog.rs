//! Raw CAN capture importer and decoder.
//!
//! Parsed line formats (any mix is accepted, unrecognised lines are skipped):
//! - `candump -L`:        `(1609459200.123456) can0 7E8#03410C1AF8`
//! - Vector ASC:          `   0.016000 1  7E8  Rx   d 8 03 41 0C 1A F8 00 00 00`
//! - `candriver monitor`: `  164.912 RX 7E8 [8] 03 41 0C 1A F8 00 00 00`

use std::collections::BTreeMap;

use candriver_core::CanFrame;
use candriver_obd::isotp::{IsoTpRx, RxEvent};
use candriver_obd::pid;

use super::ddt::{Ecu, Value};
use crate::session::{Channel, Session};

pub fn parse(text: &str) -> Vec<CanFrame> {
    text.lines().filter_map(|l| candump(l).or_else(|| asc(l)).or_else(|| monitor(l))).collect()
}

fn frame(t: f64, id: &str, extended: bool, data: Vec<u8>) -> Option<CanFrame> {
    let id = u32::from_str_radix(id, 16).ok()?;
    let extended = extended || id > 0x7ff;
    (data.len() <= 8 && id <= 0x1fff_ffff).then_some(CanFrame { id, extended, data, timestamp_us: (t * 1e6) as u64, echo: false })
}

fn hex_bytes<'a>(parts: impl Iterator<Item = &'a str>) -> Option<Vec<u8>> {
    parts.map(|p| u8::from_str_radix(p, 16).ok()).collect()
}

fn candump(line: &str) -> Option<CanFrame> {
    let rest = line.trim().strip_prefix('(')?;
    let (ts, rest) = rest.split_once(')')?;
    let (_iface, frame_part) = rest.trim().split_once(' ')?;
    let (id, data) = frame_part.trim().split_once('#')?;
    if data.starts_with('#') || data.starts_with('R') {
        return None; // CAN FD / remote frame
    }
    let data = data.replace('.', "");
    let bytes = (0..data.len()).step_by(2).map(|i| u8::from_str_radix(data.get(i..i + 2)?, 16).ok()).collect::<Option<Vec<_>>>()?;
    frame(ts.parse().ok()?, id, id.len() > 3, bytes)
}

fn asc(line: &str) -> Option<CanFrame> {
    let p: Vec<&str> = line.split_whitespace().collect();
    // t channel id dir d len bytes…
    let t: f64 = p.first()?.parse().ok()?;
    p.get(1)?.parse::<u32>().ok()?;
    let (id, ext) = match p.get(2)?.strip_suffix(['x', 'X']) {
        Some(id) => (id, true),
        None => (*p.get(2)?, false),
    };
    if !matches!(*p.get(3)?, "Rx" | "Tx") || *p.get(4)? != "d" {
        return None;
    }
    let len: usize = p.get(5)?.parse().ok()?;
    let data = hex_bytes(p.get(6..6 + len)?.iter().copied())?;
    frame(t, id, ext, data)
}

fn monitor(line: &str) -> Option<CanFrame> {
    let p: Vec<&str> = line.split_whitespace().collect();
    // t RX|TX id [n] bytes…
    let t: f64 = p.first()?.parse().ok()?;
    if !matches!(*p.get(1)?, "RX" | "TX") {
        return None;
    }
    let len: usize = p.get(3)?.strip_prefix('[')?.strip_suffix(']')?.parse().ok()?;
    let data = hex_bytes(p.get(4..4 + len)?.iter().copied())?;
    let mut f = frame(t, p.get(2)?, p.get(2)?.len() > 3, data)?;
    f.echo = p[1] == "TX";
    Some(f)
}

/// OBD-II response from an emission ECU (11-bit 7E8–7EF, 29-bit 18DAF1xx).
fn is_obd_response(f: &CanFrame) -> bool {
    (!f.extended && (0x7e8..=0x7ef).contains(&f.id)) || (f.extended && f.id & 0x1fff_ff00 == 0x18da_f100)
}

/// Decodes frames into channels: OBD-II mode 01 replies, plus every request of every
/// `ecus` definition whose reply appears on that ECU's response ID.
pub fn decode(frames: &[CanFrame], ecus: &[Ecu], source: &str) -> Session {
    let mut session = Session::new(source);
    let mut rx: BTreeMap<u32, IsoTpRx> = BTreeMap::new();
    // first ECU to answer a PID owns it, so two ECUs do not interleave into one channel
    let mut owner: BTreeMap<u8, u32> = BTreeMap::new();

    for f in frames.iter().filter(|f| !f.echo) {
        let wanted = is_obd_response(f) || ecus.iter().any(|e| e.recv_id == Some(f.id));
        if !wanted {
            continue;
        }
        let t = f.timestamp_us as f64 / 1e6;
        let RxEvent::Complete(msg) = rx.entry(f.id).or_default().on_frame(&f.data) else { continue };

        if is_obd_response(f) {
            if let [0x41, p, data @ ..] = msg.as_slice() {
                let (p, info) = (*p, pid::info(*p));
                if let (Some(info), Some(v)) = (info, pid::decode(p, data)) {
                    if *owner.entry(p).or_insert(f.id) == f.id {
                        let key = format!("pid:{p:02x}");
                        let ch = session.channel(&key, || {
                            let mut c = Channel::new(key.clone(), info.name, info.unit);
                            (c.min, c.max) = (info.min, info.max);
                            c
                        });
                        session.push(t, ch, v);
                    }
                }
            }
        }
        for ecu in ecus.iter().filter(|e| e.recv_id == Some(f.id)) {
            for d in ecu.decode_reply(&msg) {
                let n = match d.value {
                    Value::Number(n) => n,
                    Value::Raw(r) => r as f64,
                    Value::Text(_) => continue,
                };
                let key = format!("ddt:{}:{}:{}", ecu.name, d.request, d.item);
                let ch = session.channel(&key, || Channel::new(key.clone(), format!("{} · {}", ecu.name, d.item), d.unit.clone()));
                session.push(t, ch, n);
            }
        }
    }
    session.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_three_formats() {
        let text = "\
(1609459200.500000) can0 7E8#04410C1AF8
(1609459200.600000) can0 18DAF110#0341051B
   0.016000 1  7E8  Rx   d 8 03 41 0D 40 00 00 00 00
   0.020000 1  18DAF110x  Rx   d 3 03 41 05
  164.912 RX 240 [8] 00 80 80 00 00 00 00 00
  164.913 TX 7DF [8] 02 01 0C 00 00 00 00 00
garbage line
(1.0) can0 123##1AABB
";
        let f = parse(text);
        assert_eq!(f.len(), 6, "{f:?}");
        assert_eq!((f[0].id, f[0].extended, f[0].data.as_slice()), (0x7e8, false, &[4, 0x41, 0x0c, 0x1a, 0xf8][..]));
        assert!(f[1].extended && f[1].id == 0x18da_f110);
        assert_eq!(f[2].timestamp_us, 16_000);
        assert!(f[3].extended);
        assert!(f[5].echo && !f[4].echo);
    }

    #[test]
    fn decodes_obd_replies_including_multi_frame_and_ignores_other_traffic() {
        let text = "\
(10.000) can0 7E8#04410C1AF8
(10.100) can0 240#0080800000000000
(10.200) can0 7E8#03410D3C
(11.000) can0 7E8#04410C2000
(11.100) can0 7E9#04410C0000
";
        let s = decode(&parse(text), &[], "x.log");
        let names: Vec<_> = s.channels.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Engine speed", "Vehicle speed"]);
        let rpm: Vec<f64> = s.readings.iter().filter(|r| r.channel == 0).map(|r| r.value).collect();
        assert_eq!(rpm, [1726.0, 2048.0]); // 7E9's reply is ignored: 7E8 owns the PID
        assert!(s.readings.iter().any(|r| r.channel == 1 && r.value == 60.0));
        assert_eq!(s.readings[0].t, 0.0); // time starts at the first reading
    }
}
