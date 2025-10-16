// src/main.rs (host simulation)
use std::thread::sleep;
use std::time::{Duration, Instant};

const SAMPLE_RATE: usize = 48000;
const CHUNK_SAMPLES: usize = 480; // 10 ms frames
const L_DESIRED_DB: f32 = 75.0; // target perceived playback level
const USER_OFFSET_DB: f32 = 0.0;

fn speed_to_noise(speed_kmh: f32) -> f32 {
    // simple model: noise increases with log(speed)
    let a = 6.0;
    let b = 40.0;
    a * (speed_kmh + 1.0).ln() + b
}

struct Smoother {
    value_db: f32,
    tau_attack: f32,
    tau_release: f32,
    last_update: Instant,
}

impl Smoother {
    fn new(init_db: f32, tau_attack: f32, tau_release: f32) -> Self {
        Smoother {
            value_db: init_db,
            tau_attack,
            tau_release,
            last_update: Instant::now(),
        }
    }
    fn step(&mut self, target_db: f32) -> f32 {
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
}

fn db_to_lin(db: f32) -> f32 {
    (10.0f32).powf(db / 20.0)
}

// Simple soft limiter: if |sample| > threshold => compress to avoid clip
fn soft_limit(sample: f32, threshold: f32) -> f32 {
    let abs = sample.abs();
    if abs <= threshold { sample }
    else {
        let sign = sample.signum();
        // gentle compression beyond threshold (e.g., sqrt curve)
        let exceeded = (abs - threshold) / (1.0 + abs - threshold);
        sign * (threshold + exceeded)
    }
}

fn apply_gain_and_limit(input: &[i16], gain_lin: f32) -> Vec<i16> {
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

fn mock_get_cabin_noise_db(t: f32) -> f32 {
    // simulate a varying cabin noise in dB SPL
    // base 60 dB, plus slow sine modulation + transient bumps
    let base = 60.0;
    base + 5.0 * (0.2 * t).sin() + 8.0 * (0.5 * t).sin()
}

fn mock_get_speed_kmh(t: f32) -> f32 {
    // simulate speed between 0 and 120
    60.0 + 40.0 * (0.05 * t).sin()
}

fn main() {
    let mut smoother = Smoother::new(0.0, 0.1, 1.0); // tau_attack=0.1s, tau_release=1s
    let mut t = 0.0f32;
    let dt = CHUNK_SAMPLES as f32 / SAMPLE_RATE as f32;
    for _iter in 0..1000 {
        // 1) read simulated sensors
        let cabin_db = mock_get_cabin_noise_db(t);
        let speed = mock_get_speed_kmh(t);
        let speed_noise = speed_to_noise(speed);
        let noise_db = cabin_db.max(speed_noise);

        // 2) compute raw gain dB
        let gain_db_raw = L_DESIRED_DB - noise_db + USER_OFFSET_DB;

        // clamp gain_db within reasonable bounds
        let gain_db_raw = gain_db_raw.max(-24.0).min(24.0);

        // 3) smooth
        let gain_db = smoother.step(gain_db_raw);

        // 4) convert to linear
        let gain_lin = db_to_lin(gain_db);

        // 5) simulate input audio chunk (sine)
        let mut chunk = vec![0i16; CHUNK_SAMPLES];
        for n in 0..CHUNK_SAMPLES {
            let sample = 0.4 * (2.0 * std::f32::consts::PI * 1000.0 * (t + n as f32 / SAMPLE_RATE as f32)).sin() ;
            chunk[n] = (sample * i16::MAX as f32) as i16;
        }

        // 6) apply
        let out_chunk: Vec<i16> = apply_gain_and_limit(&chunk, gain_lin);

        // here you'd send out_chunk to audio device / DMA

        // logging (print every 50 iter)
        if _iter % 50 == 0 {
            println!("t={:.2}s speed={:.1} km/h cabin_db={:.1} noise_db={:.1} gain_db={:.2} gain_lin={:.3}",
                t, speed, cabin_db, noise_db, gain_db, gain_lin);
        }

        t += dt;
        sleep(Duration::from_secs_f32(dt)); // simulate real time
    }
}
