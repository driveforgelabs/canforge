//! A fake car for `--simulate`: a repeating drive cycle and a few stored faults.

pub struct Simulator {
    t: f64,
}

/// Raw DTC values: P0301, P0420, P0171.
pub const CODES: &[u16] = &[0x0301, 0x0420, 0x0171];

impl Simulator {
    pub fn new() -> Self {
        Self { t: 0.0 }
    }

    pub fn tick(&mut self, dt: f64) {
        self.t += dt;
    }

    pub fn value(&self, pid: u8) -> Option<f64> {
        let t = self.t;
        // 40 s cycle: accelerate, cruise, brake, idle.
        let phase = (t % 40.0) / 40.0;
        let speed = match phase {
            p if p < 0.3 => p / 0.3 * 90.0,
            p if p < 0.6 => 90.0 + 6.0 * (t * 0.7).sin(),
            p if p < 0.8 => (0.8 - p) / 0.2 * 90.0,
            _ => 0.0,
        };
        Some(match pid {
            0x0c => 800.0 + speed * 28.0 + 40.0 * (t * 3.0).sin(),
            0x0d => speed,
            0x05 => (20.0 + t * 1.2).min(92.0) + (t * 0.2).sin(),
            0x04 => 15.0 + speed * 0.5 + 3.0 * (t * 2.0).sin(),
            0x11 => 12.0 + speed * 0.4,
            0x0f => 31.0 + (t * 0.05).sin() * 2.0,
            0x2f => (68.0 - t * 0.01).max(0.0),
            0x42 => 13.9 + 0.2 * (t * 0.5).sin(),
            _ => return None,
        })
    }
}
