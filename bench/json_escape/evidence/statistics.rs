//! Exact integer timing statistics shared by the protected JSON evidence harnesses.

use std::time::Duration;

pub fn elapsed_nanoseconds(elapsed: Duration) -> Result<u64, &'static str> {
    elapsed
        .as_nanos()
        .try_into()
        .map_err(|_| "elapsed nanoseconds exceed u64")
}

pub fn median_microseconds(samples_ns: &mut [u64]) -> Result<u64, &'static str> {
    if samples_ns.is_empty() {
        return Err("median requires at least one sample");
    }
    samples_ns.sort_unstable();
    let middle = samples_ns.len() / 2;
    if samples_ns.len() % 2 == 1 {
        samples_ns[middle]
            .checked_add(500)
            .map(|rounded| rounded / 1_000)
            .ok_or("odd median quantization overflow")
    } else {
        samples_ns[middle - 1]
            .checked_add(samples_ns[middle])
            .and_then(|sum| sum.checked_add(1_000))
            .map(|rounded| rounded / 2_000)
            .ok_or("even median quantization overflow")
    }
}

pub fn milliseconds_token(microseconds: u64) -> String {
    format!("{}.{:03}", microseconds / 1_000, microseconds % 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_evidence_statistic_matrix() {
        let mut odd = [2_499, 500, 1_500];
        assert_eq!(median_microseconds(&mut odd), Ok(2));

        let mut even = [9_000, 1_000, 2_000, 8_000];
        assert_eq!(median_microseconds(&mut even), Ok(5));

        let mut half_up = [0, 1_000];
        assert_eq!(median_microseconds(&mut half_up), Ok(1));

        let mut equal_middle = [1_500, 1_500, 1_500, 1_500];
        assert_eq!(median_microseconds(&mut equal_middle), Ok(2));

        let mut low_outlier = [0, 10_000, 10_000, 10_000];
        assert_eq!(median_microseconds(&mut low_outlier), Ok(10));

        let mut odd_overflow = [u64::MAX];
        assert_eq!(
            median_microseconds(&mut odd_overflow),
            Err("odd median quantization overflow")
        );
        let mut even_overflow = [u64::MAX, u64::MAX];
        assert_eq!(
            median_microseconds(&mut even_overflow),
            Err("even median quantization overflow")
        );
        assert_eq!(
            median_microseconds(&mut []),
            Err("median requires at least one sample")
        );

        assert_eq!(milliseconds_token(0), "0.000");
        assert_eq!(milliseconds_token(999), "0.999");
        assert_eq!(milliseconds_token(1_001), "1.001");
        assert_eq!(elapsed_nanoseconds(Duration::from_nanos(7)), Ok(7));
    }
}
