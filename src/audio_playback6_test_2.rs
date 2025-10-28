#[cfg(test)]
mod tests {
    use super::*;
    use crate::adaptive_gain::{
        db_to_lin,
        speed_to_noise,
        mock_get_speed_kmh,
        mock_get_cabin_noise_db,
        Smoother,
    };
    use std::f32::consts::PI;

    #[test]
    fn test_mock_speed_and_noise() {
        // Test over different time samples
        let t_values = vec![0.0, 10.0, 20.0, 30.0, 40.0];
        
        for t in t_values {
            let speed = mock_get_speed_kmh(t);
            let noise = mock_get_cabin_noise_db(t);

            // Speed should oscillate roughly between 20–100 km/h
            assert!(
                (20.0..=100.0).contains(&speed),
                "Speed {} at time {} out of expected range", speed, t
            );

            // Noise should oscillate roughly between 47–73 dB
            assert!(
                (47.0..=73.0).contains(&noise),
                "Noise {} at time {} out of expected range", noise, t
            );
        }
    }

    #[test]
    fn test_speed_to_noise_relationship() {
        let speeds = vec![0.0, 30.0, 60.0, 90.0, 120.0];
        let mut prev_noise = None;

        for speed in speeds {
            let noise = speed_to_noise(speed);
            assert!(noise > 30.0 && noise < 90.0, "Noise {} out of range", noise);

            if let Some(prev) = prev_noise {
                assert!(
                    noise > prev,
                    "Noise should increase with speed: {} dB at {} km/h vs {} dB at lower speed",
                    noise, speed, prev
                );
            }
            prev_noise = Some(noise);
        }
    }

    #[test]
    fn test_db_to_lin_conversion() {
        let cases = vec![
            (0.0, 1.0),
            (6.0, 1.995262),
            (-6.0, 0.501187),
            (20.0, 10.0),
            (-20.0, 0.1),
        ];

        for (db, expected) in cases {
            let result = db_to_lin(db);
            assert!(
                (result - expected).abs() < 0.0001,
                "db_to_lin({}) -> {}, expected {}", db, result, expected
            );
        }
    }

    #[test]
    fn test_smoother_behavior() {
        let mut s = Smoother::new(0.0, 0.1, 0.5);

        // attack phase (increase)
        let mut val = 0.0;
        for _ in 0..10 {
            val = s.step(6.0);
        }
        assert!(val > 0.0, "Smoother should rise during attack phase");

        // release phase (decrease)
        for _ in 0..10 {
            val = s.step(-6.0);
        }
        assert!(val < 6.0, "Smoother should decay during release phase");
    }
}
