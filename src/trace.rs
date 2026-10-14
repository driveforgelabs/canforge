//! A record of the diagnostic frames the app sent and received, for finding out why a module did not
//! answer. candriver reports every OBD and UDS message through the `log` crate at trace level;
//! this keeps the most recent ones in memory. The simulator adds its own lines so the viewer works
//! without a car.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

const KEEP: usize = 6000;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub seq: u64,
    /// Milliseconds since the app started.
    pub ms: u64,
    pub text: String,
}

impl Line {
    pub fn is_tx(&self) -> bool {
        self.text.contains(" TX ")
    }
    pub fn is_obd(&self) -> bool {
        self.text.starts_with("OBD")
    }
}

static BUF: Mutex<VecDeque<Line>> = Mutex::new(VecDeque::new());
static SEQ: AtomicU64 = AtomicU64::new(0);
static START: OnceLock<Instant> = OnceLock::new();

pub fn push(text: String) {
    let ms = START.get_or_init(Instant::now).elapsed().as_millis() as u64;
    let seq = SEQ.fetch_add(1, Ordering::Relaxed) + 1;
    if let Ok(mut buf) = BUF.lock() {
        if buf.len() >= KEEP {
            buf.pop_front();
        }
        buf.push_back(Line { seq, ms, text });
    }
}

/// The sequence number of the newest line (0 when there is none).
pub fn latest() -> u64 {
    SEQ.load(Ordering::Relaxed)
}

/// Lines newer than `seq`, oldest first.
pub fn since(seq: u64) -> Vec<Line> {
    BUF.lock().map(|b| b.iter().filter(|l| l.seq > seq).cloned().collect()).unwrap_or_default()
}

/// One line per frame as plain text, for saving next to a recording.
pub fn to_text(lines: &[Line]) -> String {
    lines.iter().map(|l| format!("{:>9.3}  {}\n", l.ms as f64 / 1000.0, l.text)).collect()
}

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() == log::Level::Trace && (m.target().starts_with("candriver_uds") || m.target().starts_with("candriver_obd"))
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            push(r.args().to_string());
        }
    }
    fn flush(&self) {}
}

/// Starts keeping candriver's frame log. Harmless if called twice.
pub fn install() {
    START.get_or_init(Instant::now);
    if log::set_logger(&Logger).is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_order_and_reads_since() {
        let before = latest();
        push("UDS TX 7E0 [03, 22, F1, 90]".into());
        push("UDS RX 7E8 [10, 14]".into());
        let new = since(before);
        assert!(new.len() >= 2);
        let tail = &new[new.len() - 2..];
        assert!(tail[0].is_tx() && !tail[1].is_tx());
        assert!(tail[0].seq < tail[1].seq);
        assert!(to_text(tail).contains("UDS RX 7E8"));
    }
}
