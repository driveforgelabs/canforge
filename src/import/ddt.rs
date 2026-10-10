//! DDT4ALL ECU definitions (the JSON files DDT4ALL produces from its ECU database).
//!
//! An ECU lists `requests` (what to send, what the reply starts with, which data items sit at
//! which byte of the reply) and `data` (how to turn those bytes into a value). Only the
//! parts needed to **decode replies** are read; nothing here sends anything.
//!
//! The database content is Renault's. Load files you obtained yourself; none are bundled.
//!
//! Decoding rules, as implemented:
//! - `firstbyte` is 1-based over the **whole reply** (byte 1 is the service ID).
//! - `bitoffset` counts from the most significant bit; `bitscount` is the field width.
//! - Little-endian fields are supported when byte-aligned (or inside one byte). Little-endian
//!   fields that straddle bytes at a bit offset are not decoded.
//! - scaled values: `(raw × step + offset) ÷ divideby`; unscaled values with a `lists` entry
//!   give its text, otherwise the raw number.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};
use serde_json::Value as Json;

/// A decoded data item.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    /// Unscaled raw value (flags, enumerations without a list entry).
    Raw(u64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub request: String,
    pub item: String,
    pub unit: String,
    pub value: Value,
}

#[derive(Debug, Clone)]
pub struct Ecu {
    pub name: String,
    /// CAN ID the ECU answers on (`obd.recv_id`), if the file has one.
    pub recv_id: Option<u32>,
    little_endian: bool,
    requests: Vec<Request>,
    data: BTreeMap<String, DataDef>,
}

#[derive(Debug, Clone)]
struct Request {
    name: String,
    /// Expected reply prefix.
    reply: Vec<u8>,
    items: BTreeMap<String, ItemPos>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ItemPos {
    #[serde(default)]
    firstbyte: usize,
    #[serde(default)]
    bitoffset: usize,
    #[serde(default)]
    endian: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DataDef {
    #[serde(default = "eight", deserialize_with = "usize_lenient")]
    bitscount: usize,
    #[serde(default)]
    bytesascii: bool,
    #[serde(default)]
    scaled: bool,
    #[serde(default)]
    signed: bool,
    #[serde(default = "one", deserialize_with = "f64_lenient")]
    step: f64,
    #[serde(default, deserialize_with = "f64_lenient")]
    offset: f64,
    #[serde(default = "one", deserialize_with = "f64_lenient")]
    divideby: f64,
    #[serde(default)]
    unit: String,
    #[serde(default)]
    lists: BTreeMap<String, String>,
}

fn eight() -> usize {
    8
}
fn one() -> f64 {
    1.0
}
fn f64_lenient<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    Ok(match Json::deserialize(d)? {
        Json::Number(n) => n.as_f64().unwrap_or(0.0),
        Json::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    })
}
fn usize_lenient<'de, D: Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    f64_lenient(d).map(|v| v as usize)
}

fn hex(s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    (0..s.len()).step_by(2).filter_map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

impl Ecu {
    pub fn from_json(text: &str) -> Result<Self, String> {
        let root: Json = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
        let obj = root.as_object().ok_or("expected a JSON object")?;
        let name = obj.get("ecuname").and_then(Json::as_str).ok_or("not a DDT4ALL ECU file (no \"ecuname\")")?;
        let id = |k: &str| {
            obj.get("obd")?.get(k)?.as_str().and_then(|s| u32::from_str_radix(s.trim(), 16).ok())
        };

        let data: BTreeMap<String, DataDef> = obj
            .get("data")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()
            .map_err(|e| format!("bad \"data\" section: {e}"))?
            .unwrap_or_default();

        let mut requests = Vec::new();
        for r in obj.get("requests").and_then(Json::as_array).into_iter().flatten() {
            let items = r
                .get("receivebyte_dataitems")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| format!("bad request items: {e}"))?
                .unwrap_or_default();
            requests.push(Request {
                name: r.get("name").and_then(Json::as_str).unwrap_or("?").to_string(),
                reply: hex(r.get("replybytes").and_then(Json::as_str).unwrap_or("")),
                items,
            });
        }
        Ok(Self {
            name: name.to_string(),
            recv_id: id("recv_id"),
            little_endian: obj.get("endian").and_then(Json::as_str) == Some("Little"),
            requests,
            data,
        })
    }

    /// Decodes a complete reply (starting at the service ID) with every request it matches.
    pub fn decode_reply(&self, msg: &[u8]) -> Vec<Decoded> {
        let mut out = Vec::new();
        for req in self.requests.iter().filter(|r| !r.reply.is_empty() && msg.starts_with(&r.reply)) {
            for (item, pos) in &req.items {
                let Some(def) = self.data.get(item) else { continue };
                let little = match pos.endian.as_str() {
                    "Little" => true,
                    "Big" => false,
                    _ => self.little_endian,
                };
                if let Some(value) = def.decode(msg, pos, little) {
                    out.push(Decoded { request: req.name.clone(), item: item.clone(), unit: def.unit.clone(), value });
                }
            }
        }
        out
    }
}

impl DataDef {
    fn decode(&self, msg: &[u8], pos: &ItemPos, little: bool) -> Option<Value> {
        let start = pos.firstbyte.checked_sub(1)?;
        let bits = self.bitscount;
        let nbytes = (pos.bitoffset + bits).div_ceil(8);
        let field = msg.get(start..start + nbytes)?;

        if self.bytesascii {
            let text: String = field.iter().filter(|b| **b != 0).map(|&b| b as char).collect();
            return Some(Value::Text(text.trim().to_string()));
        }
        if bits == 0 || bits > 64 {
            return None;
        }

        let raw = if nbytes == 1 || !little {
            // big-endian bit string, MSB first: skip `bitoffset` bits, take `bits`
            let all = field.iter().fold(0u128, |a, &b| a << 8 | u128::from(b));
            let drop = nbytes * 8 - pos.bitoffset - bits;
            ((all >> drop) & ((1u128 << bits) - 1)) as u64
        } else if pos.bitoffset == 0 && bits % 8 == 0 {
            field.iter().rev().fold(0u64, |a, &b| a << 8 | u64::from(b))
        } else {
            return None; // little-endian field straddling bytes at a bit offset
        };

        let signed = || {
            if self.signed && bits < 64 && raw >> (bits - 1) & 1 == 1 { raw as i64 - (1i64 << bits) } else { raw as i64 }
        };
        if self.scaled {
            if self.divideby == 0.0 {
                return None;
            }
            return Some(Value::Number((signed() as f64 * self.step + self.offset) / self.divideby));
        }
        match self.lists.get(&signed().to_string()) {
            Some(text) => Some(Value::Text(text.clone())),
            None => Some(Value::Raw(raw)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECU: &str = r#"{
        "ecuname": "EDC16", "endian": "Big",
        "obd": {"protocol": "CAN", "send_id": "7E0", "recv_id": "7E8"},
        "requests": [{
            "name": "Read engine data", "sentbytes": "2101", "replybytes": "6101",
            "receivebyte_dataitems": {
                "Rail pressure": {"firstbyte": 3},
                "Coolant":       {"firstbyte": 5},
                "Torque":        {"firstbyte": 6, "endian": "Little"},
                "Glow plugs":    {"firstbyte": 8, "bitoffset": 2},
                "Mode":          {"firstbyte": 9},
                "Unknown item":  {"firstbyte": 1}
            }
        }],
        "data": {
            "Rail pressure": {"bitscount": 16, "scaled": true, "step": 1, "offset": 0, "divideby": 10, "unit": "bar"},
            "Coolant":       {"bitscount": 8, "scaled": true, "step": 1, "offset": -40, "divideby": "1", "unit": "°C"},
            "Torque":        {"bitscount": 16, "scaled": true, "signed": true, "step": 0.5, "unit": "Nm"},
            "Glow plugs":    {"bitscount": 3},
            "Mode":          {"bitscount": 8, "lists": {"0": "Off", "1": "Idle", "2": "Drive"}}
        }
    }"#;

    fn decoded(msg: &[u8]) -> BTreeMap<String, Value> {
        Ecu::from_json(ECU).unwrap().decode_reply(msg).into_iter().map(|d| (d.item, d.value)).collect()
    }

    #[test]
    fn reads_metadata() {
        let e = Ecu::from_json(ECU).unwrap();
        assert_eq!((e.name.as_str(), e.recv_id), ("EDC16", Some(0x7e8)));
    }

    #[test]
    fn decodes_scaled_signed_bitfield_and_list_items() {
        // 61 01 | 04 B0 (1200/10 = 120 bar) | 5A (90-40 = 50 °C) | E8 FF (LE: -24 × 0.5) | 0b0011_1000 | 02
        let v = decoded(&[0x61, 0x01, 0x04, 0xb0, 0x5a, 0xe8, 0xff, 0b0011_1000, 0x02]);
        assert_eq!(v["Rail pressure"], Value::Number(120.0));
        assert_eq!(v["Coolant"], Value::Number(50.0));
        assert_eq!(v["Torque"], Value::Number(-12.0));
        assert_eq!(v["Glow plugs"], Value::Raw(0b111)); // bits 2..5 of 0b0011_1000
        assert_eq!(v["Mode"], Value::Text("Drive".into()));
        assert!(!v.contains_key("Unknown item"));
    }

    #[test]
    fn ignores_other_replies_and_short_messages() {
        assert!(decoded(&[0x61, 0x02, 0x04, 0xb0]).is_empty());
        let v = decoded(&[0x61, 0x01, 0x04]); // too short for anything past byte 3
        assert!(v.is_empty());
    }

    #[test]
    fn rejects_files_that_are_not_ecu_definitions() {
        assert!(Ecu::from_json("[1,2]").is_err());
        assert!(Ecu::from_json("{\"foo\": 1}").is_err());
        assert!(Ecu::from_json("nope").is_err());
    }

    #[test]
    fn works_on_a_ddt4all_test_fixture_shape() {
        let e = Ecu::from_json(
            r#"{"ecuname":"JSON_ECU","obd":{"send_id":"7E0","recv_id":"7E8"},"requests":[{"name":"ReadVIN","sentbytes":"22F190","replybytes":"62F190","receivebyte_dataitems":{"VIN":{"firstbyte":4}}}],"data":{"VIN":{"bitscount":136,"bytesascii":true}}}"#,
        )
        .unwrap();
        let mut msg = vec![0x62, 0xf1, 0x90];
        msg.extend_from_slice(b"WF0AXXWPMA8Y12345");
        assert_eq!(e.decode_reply(&msg)[0].value, Value::Text("WF0AXXWPMA8Y12345".into()));
    }
}
