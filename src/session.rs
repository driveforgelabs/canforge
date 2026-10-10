//! A recorded diagnostic session: named channels and timestamped readings, whatever the
//! source (live OBD, a CSV data log, a CAN capture decoded with ECU definitions).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// Stable identifier, unique within a session (`pid:0c`, `csv:3`, `ddt:…`).
    pub key: String,
    pub name: String,
    pub unit: String,
    /// Gauge range; `max <= min` means "unset", and [`Session::finish`] fits it to the data.
    pub min: f64,
    pub max: f64,
    /// Values above this are shown as a warning.
    pub warn_above: Option<f64>,
}

impl Channel {
    pub fn new(key: impl Into<String>, name: impl Into<String>, unit: impl Into<String>) -> Self {
        Self { key: key.into(), name: name.into(), unit: unit.into(), min: 0.0, max: 0.0, warn_above: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// Seconds since the start of the session.
    pub t: f64,
    /// Index into [`Session::channels`].
    pub channel: usize,
    pub value: f64,
}

#[derive(Debug, Clone, Default)]
pub struct Session {
    pub source: String,
    pub channels: Vec<Channel>,
    /// Sorted by time.
    pub readings: Vec<Reading>,
}

impl Session {
    pub fn new(source: impl Into<String>) -> Self {
        Self { source: source.into(), ..Self::default() }
    }

    /// Index of the channel with `key`, adding it with `make` if new.
    pub fn channel(&mut self, key: &str, make: impl FnOnce() -> Channel) -> usize {
        match self.channels.iter().position(|c| c.key == key) {
            Some(i) => i,
            None => {
                self.channels.push(make());
                self.channels.len() - 1
            }
        }
    }

    pub fn push(&mut self, t: f64, channel: usize, value: f64) {
        if value.is_finite() {
            self.readings.push(Reading { t, channel, value });
        }
    }

    pub fn duration(&self) -> f64 {
        self.readings.last().map_or(0.0, |r| r.t)
    }

    /// Sorts readings, shifts time to start at 0, drops empty channels and fits gauge
    /// ranges to the data (channels that already have a meaningful range keep it).
    pub fn finish(mut self) -> Self {
        self.readings.sort_by(|a, b| a.t.total_cmp(&b.t));
        let t0 = self.readings.first().map_or(0.0, |r| r.t);
        for r in &mut self.readings {
            r.t -= t0;
        }
        let mut span: BTreeMap<usize, (f64, f64)> = BTreeMap::new();
        for r in &self.readings {
            let e = span.entry(r.channel).or_insert((r.value, r.value));
            e.0 = e.0.min(r.value);
            e.1 = e.1.max(r.value);
        }
        // compact channel indices, keeping only channels that have data
        let remap: BTreeMap<usize, usize> = span.keys().enumerate().map(|(new, &old)| (old, new)).collect();
        let old = std::mem::take(&mut self.channels);
        for (i, mut ch) in old.into_iter().enumerate() {
            if let Some(&(lo, hi)) = span.get(&i) {
                if ch.max <= ch.min {
                    let pad = ((hi - lo) * 0.05).max(1e-6);
                    (ch.min, ch.max) = (lo - pad, hi + pad);
                }
                self.channels.push(ch);
            }
        }
        for r in &mut self.readings {
            r.channel = remap[&r.channel];
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_normalises() {
        let mut s = Session::new("t");
        let a = s.channel("a", || Channel::new("a", "A", "u"));
        let unused = s.channel("unused", || Channel::new("unused", "U", ""));
        let b = s.channel("b", || Channel::new("b", "B", "u"));
        assert_eq!((a, unused, b), (0, 1, 2));
        s.push(12.0, b, 5.0);
        s.push(10.0, a, 1.0);
        s.push(11.0, a, 3.0);
        s.push(11.5, a, f64::NAN); // ignored
        let s = s.finish();
        assert_eq!(s.channels.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(s.readings.iter().map(|r| (r.t, r.channel)).collect::<Vec<_>>(), [(0.0, 0), (1.0, 0), (2.0, 1)]);
        assert!(s.channels[0].min < 1.0 && s.channels[0].max > 3.0);
        assert_eq!(s.duration(), 2.0);
    }
}
