use std::time::Instant;

pub struct AdaptiveGain {
    last_gain_db: f32,
    last_update: Instant,
    tau_attack: f32,
    tau_release: f32,
    l_desired_db: f32,
    user_offset_db: f32,
}

impl AdaptiveGain {
    pub fn new(l_desired_db: f32, tau_attack: f32, tau_release: f32, user_offset_db: f32) -> Self {
        Self {
            last_gain_db: 0.0,
            last_update: Instant::now(),
            tau_attack,
            tau_release,
            l_desired_db,
            user_offset_db,
        }
    }

    fn speed_to_noise(speed_kmh: f32) -> f32 {
        let a = 6.0;
        let b = 40.0;
        a * (speed_kmh + 1.0).ln() + b
    }

    pub fn compute_gain(&mut self, cabin_db: f32, speed_kmh: f32) -> (f32, f32) {
        let noise_db = cabin_db.max(Self::speed_to_noise(speed_kmh));
        let mut raw_gain_db = self.l_desired_db - noise_db + self.user_offset_db;
        raw_gain_db = raw_gain_db.clamp(-12.0, 12.0);

        let now = Instant::now();
        let dt = (now - self.last_update).as_secs_f32();
        self.last_update = now;

        let tau = if raw_gain_db > self.last_gain_db {
            self.tau_attack
        } else {
            self.tau_release
        };
        let alpha = 1.0 - (-dt / tau).exp();
        self.last_gain_db += alpha * (raw_gain_db - self.last_gain_db);

        let gain_lin = 10f32.powf(self.last_gain_db / 20.0);
        (self.last_gain_db, gain_lin)
    }
}
