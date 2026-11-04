pub(crate) fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub(crate) fn power_to_db(power: f32) -> f32 {
    10.0 * power.max(f32::MIN_POSITIVE).log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_db_doubles_amplitude() {
        assert!((db_to_gain(6.0206) - 2.0).abs() < 1e-4);
        assert!((power_to_db(100.0) - 20.0).abs() < 1e-5);
    }
}
