pub const TARGET_HZ: f32 = 90.0;

const MATCH_TOLERANCE_HZ: f32 = 0.01;

pub fn rate_matches(left: f32, right: f32) -> bool {
    (left - right).abs() <= MATCH_TOLERANCE_HZ
}

pub fn select_supported_rate(available_rates: &[f32], target_hz: f32) -> Option<f32> {
    available_rates
        .iter()
        .copied()
        .find(|available_hz| rate_matches(*available_hz, target_hz))
}

#[cfg(test)]
mod tests {
    use super::{rate_matches, select_supported_rate};

    #[test]
    fn selects_the_advertised_target_rate() {
        assert_eq!(
            select_supported_rate(&[72.0, 80.0, 90.0, 120.0], 90.0),
            Some(90.0)
        );
    }

    #[test]
    fn accepts_small_runtime_rounding_differences() {
        assert_eq!(
            select_supported_rate(&[72.0, 89.999, 120.0], 90.0),
            Some(89.999)
        );
        assert!(rate_matches(89.999, 90.0));
    }

    #[test]
    fn does_not_substitute_a_different_refresh_rate() {
        assert_eq!(select_supported_rate(&[72.0, 80.0, 120.0], 90.0), None);
        assert_eq!(select_supported_rate(&[], 90.0), None);
        assert!(!rate_matches(80.0, 90.0));
    }
}
