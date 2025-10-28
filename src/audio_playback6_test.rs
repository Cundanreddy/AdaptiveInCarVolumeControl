#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[test]
    fn test_fetch_remote_state_invalid_url() {
        let result = fetch_remote_state("http://invalid-url-that-does-not-exist.com/state");
        assert!(result.is_none(), "Should return None for invalid URL");
    }

    #[test]
    fn test_mock_speed_and_noise() {
        // Test at different time points
        let t_values = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        
        for t in t_values {
            let speed = mock_get_speed_kmh(t);
            let noise = mock_get_cabin_noise_db(t);
            
            // Speed should be within reasonable bounds (0-120 km/h as per mock function)
            assert!(speed >= 20.0 && speed <= 100.0, 
                "Speed {} at time {} should be within reasonable bounds", speed, t);
            
            // Cabin noise should be within reasonable bounds (base is 60dB with modulation)
            assert!(noise >= 47.0 && noise <= 73.0,
                "Noise {} at time {} should be within reasonable bounds", noise, t);
        }
    }

    #[test]
    fn test_speed_to_noise_conversion() {
        let test_speeds = vec![0.0, 30.0, 60.0, 90.0, 120.0];
        
        for speed in test_speeds {
            let noise = speed_to_noise(speed);
            
            // Noise should increase with speed
            assert!(noise > 40.0, "Noise level should be above 40dB");
            assert!(noise < 90.0, "Noise level should be below 90dB");
            
            if speed > 0.0 {
                let lower_speed_noise = speed_to_noise(speed - 10.0);
                assert!(noise > lower_speed_noise, 
                    "Noise should increase with speed: {} dB at {} km/h vs {} dB at {} km/h",
                    noise, speed, lower_speed_noise, speed - 10.0);
            }
        }
    }

    #[test]
    fn test_gain_smoother() {
        let mut smoother = Smoother::new(0.0, 0.1, 1.0);
        
        // Test attack (getting louder)
        let target_db = 6.0;
        let mut current = smoother.step_dt(target_db, 0.05);
        assert!(current > 0.0 && current < target_db, 
            "First attack step should be between 0 and target: {}", current);
        
        // Test that it eventually reaches close to target
        for _ in 0..20 {
            current = smoother.step_dt(target_db, 0.05);
        }
        assert!((current - target_db).abs() < 0.1, 
            "Should converge to target: {} vs {}", current, target_db);
        
        // Test release (getting quieter)
        let target_db = -6.0;
        current = smoother.step_dt(target_db, 0.05);
        assert!(current < 6.0 && current > target_db,
            "First release step should be between previous and target: {}", current);
    }

    #[test]
    fn test_db_to_linear_conversion() {
        let test_cases = vec![
            (0.0, 1.0),
            (6.0, 1.995262),
            (-6.0, 0.501187),
            (20.0, 10.0),
            (-20.0, 0.1),
        ];
        
        for (db, expected) in test_cases {
            let result = db_to_lin(db);
            assert!((result - expected).abs() < 0.0001,
                "Converting {} dB to linear: got {}, expected {}", db, result, expected);
        }
    }

    #[test]
    fn test_soft_limiter() {
        let threshold = 0.8;
        
        // Test values below threshold
        let input = 0.5;
        assert_eq!(soft_limit(input, threshold), input,
            "Should not affect values below threshold");
        
        // Test values above threshold
        let input = 1.0;
        let limited = soft_limit(input, threshold);
        assert!(limited < input && limited > threshold,
            "Should softly limit values above threshold: {} -> {}", input, limited);
        
        // Test negative values
        let input = -1.0;
        let limited = soft_limit(input, threshold);
        assert!(limited > input && limited < -threshold,
            "Should softly limit negative values: {} -> {}", input, limited);
    }

    #[test]
    fn test_apply_gain_and_limit() {
        // Create a test signal: sine wave
        let frequency = 440.0; // Hz
        let duration = 0.01; // seconds
        let sample_rate = 44100;
        let num_samples = (duration * sample_rate as f32) as usize;
        
        let mut input = Vec::with_capacity(num_samples);
        for i in 0..num_samples {
            let t = i as f32 / sample_rate as f32;
            let value = (2.0 * PI * frequency * t).sin();
            input.push((value * i16::MAX as f32) as i16);
        }
        
        // Test with different gain values
        let gains = vec![0.5, 1.0, 2.0];
        for gain in gains {
            let output = apply_gain_and_limit(&input, gain);
            
            assert_eq!(output.len(), input.len(), "Output length should match input");
            
            // Check that output is properly scaled and limited
            for &sample in &output {
                assert!(sample <= i16::MAX && sample >= i16::MIN,
                    "Output should be within i16 bounds: {}", sample);
                
                if gain <= 1.0 {
                    // For gains <= 1.0, output should be strictly scaled
                    let expected_max = (i16::MAX as f32 * gain) as i16;
                    assert!(sample.abs() <= expected_max,
                        "Output {} should not exceed expected max {}", sample, expected_max);
                }
            }
        }
    }
}