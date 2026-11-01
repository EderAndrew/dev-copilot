//! Percentiles and summaries.

/// p50, p95 and max of a sample, with its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Number of values.
    pub count: usize,
    /// Median (nearest rank).
    pub p50: i64,
    /// 95th percentile (nearest rank).
    pub p95: i64,
    /// Largest value.
    pub max: i64,
}

/// Nearest-rank percentile of an ascending, non-empty slice: the value at rank
/// `ceil(p / 100 × n)`. `p` is in `(0, 100]`.
#[must_use]
pub fn percentile(sorted: &[i64], p: u32) -> Option<i64> {
    if sorted.is_empty() || p == 0 || p > 100 {
        return None;
    }
    let n = sorted.len();
    let rank = (usize::try_from(p).ok()? * n).div_ceil(100).max(1);
    sorted.get(rank - 1).copied()
}

/// Summary of `values`, or `None` when there are none.
#[must_use]
pub fn summarize(mut values: Vec<i64>) -> Option<Summary> {
    values.sort_unstable();
    Some(Summary {
        count: values.len(),
        p50: percentile(&values, 50)?,
        p95: percentile(&values, 95)?,
        max: *values.last()?,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn nearest_rank() {
        let values: Vec<i64> = (1..=100).collect();
        assert_eq!(percentile(&values, 50), Some(50));
        assert_eq!(percentile(&values, 95), Some(95));
        assert_eq!(percentile(&values, 100), Some(100));
        assert_eq!(percentile(&[7], 95), Some(7));
        // n = 20: rank ceil(19) = 19.
        let twenty: Vec<i64> = (1..=20).collect();
        assert_eq!(percentile(&twenty, 95), Some(19));
        // n = 3: p50 rank ceil(1.5) = 2.
        assert_eq!(percentile(&[1, 2, 3], 50), Some(2));
    }

    #[test]
    fn edge_cases() {
        assert_eq!(percentile(&[], 50), None);
        assert_eq!(percentile(&[1], 0), None);
        assert_eq!(percentile(&[1], 101), None);
        assert_eq!(summarize(Vec::new()), None);
    }

    #[test]
    fn summary_sorts_its_input() {
        assert_eq!(
            summarize(vec![30, 10, 20, 40]).unwrap(),
            Summary {
                count: 4,
                p50: 20,
                p95: 40,
                max: 40
            }
        );
    }
}
