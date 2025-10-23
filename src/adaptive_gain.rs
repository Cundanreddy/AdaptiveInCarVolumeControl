use std::time::Instant;
use std::time::Duration;

pub const SAMPLE_RATE: usize = 48000;
pub const CHUNK_SAMPLES: usize = 480; // 10 ms frames
pub const L_DESIRED_DB: f32 = 75.0; // target perceived playback level
pub const USER_OFFSET_DB: f32 = 0.0;
// Baseline cabin noise (used as reference zero for adaptive boost)
pub const BASE_NOISE_DB: f32 = 60.0;
// How strongly playback gain responds to increases in noise (1.0 => 1 dB gain per 1 dB noise)
pub const GAIN_SENSITIVITY: f32 = 0.6;

pub fn speed_to_noise(speed_kmh: f32) -> f32 {
    // simple model: noise increases with log(speed)
    let a = 6.0;
    let b = 40.0;
    a * (speed_kmh + 1.0).ln() + b
}

pub struct Smoother {
    pub value_db: f32,
    pub tau_attack: f32,
    pub tau_release: f32,
    pub last_update: Instant,
}

impl Smoother {
    pub fn new(init_db: f32, tau_attack: f32, tau_release: f32) -> Self {
        Smoother {
            value_db: init_db,
            tau_attack,
            tau_release,
            last_update: Instant::now(),
        }
    }

    /// Step the smoother using wall-clock time. Returns the new smoothed value.
    pub fn step(&mut self, target_db: f32) -> f32 {
        let now = Instant::now();
        let dt = (now - self.last_update).as_secs_f32();
        self.last_update = now;
        if dt <= 0.0 { return self.value_db; }
        let tau = if target_db < self.value_db {
            // getting quieter -> release (slower)
            self.tau_release
        } else {
            // getting louder -> attack (faster)
            self.tau_attack
        };
        let alpha = 1.0 - (-dt / tau).exp();
        self.value_db += alpha * (target_db - self.value_db);
        self.value_db
    }

    /// Alternative step function driven by a simulated dt (seconds).
    /// Use this when you want smoothing tied to simulated time instead of wall clock.
    pub fn step_dt(&mut self, target_db: f32, dt: f32) -> f32 {
        if dt <= 0.0 { return self.value_db; }
        let tau = if target_db < self.value_db { self.tau_release } else { self.tau_attack };
        let alpha = 1.0 - (-dt / tau).exp();
        self.value_db += alpha * (target_db - self.value_db);
        self.value_db
    }
}

pub fn db_to_lin(db: f32) -> f32 {
    (10.0f32).powf(db / 20.0)
}

// Simple soft limiter: if |sample| > threshold => compress to avoid clip
pub fn soft_limit(sample: f32, threshold: f32) -> f32 {
    let abs = sample.abs();
    if abs <= threshold { sample }
    else {
        let sign = sample.signum();
        // gentle compression beyond threshold (e.g., sqrt curve)
        let exceeded = (abs - threshold) / (1.0 + abs - threshold);
        sign * (threshold + exceeded)
    }
}

pub fn apply_gain_and_limit(input: &[i16], gain_lin: f32) -> Vec<i16> {
    let mut out = Vec::with_capacity(input.len());
    let max_i16 = i16::MAX as f32;
    let threshold = 0.98 * max_i16;
    for &s in input {
        let s_f = s as f32;
        let mut o = s_f * gain_lin;
        o = soft_limit(o, threshold);
        // clamp
        let o_clamped = o.max(-max_i16).min(max_i16);
        out.push(o_clamped as i16);
    }
    out
}

pub fn mock_get_cabin_noise_db(t: f32) -> f32 {
    // simulate a varying cabin noise in dB SPL
    // base 60 dB, plus slow sine modulation + transient bumps
    let base = 60.0;
    base + 5.0 * (0.2 * t).sin() + 8.0 * (0.5 * t).sin()
}

pub fn mock_get_speed_kmh(t: f32) -> f32 {
    // simulate speed between 0 and 120
    60.0 + 40.0 * (0.05 * t).sin()
}

/// Utility that simulates time progression (advances t by dt and sleeps wall-clock dt).
/// Returns the next t value.
pub fn advance_time_and_sleep(t: f32) -> f32 {
    let dt = CHUNK_SAMPLES as f32 / SAMPLE_RATE as f32;
    std::thread::sleep(Duration::from_secs_f32(dt));
    t + dt
}
