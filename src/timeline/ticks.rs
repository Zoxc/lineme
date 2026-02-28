//! Shared helpers for computing nice time tick intervals and formatting labels.

/// Choose a "nice" interval (power-of-ten multiplier) for a target ns interval.
pub fn nice_interval(ns_interval: f64) -> f64 {
    if !ns_interval.is_finite() || ns_interval <= 0.0 {
        return 0.0;
    }

    let log10 = ns_interval.log10().floor();
    let base = 10.0f64.powf(log10);
    let ratio = ns_interval / base;
    if ratio <= 1.0 {
        base
    } else if ratio <= 2.0 {
        base * 2.0
    } else if ratio <= 5.0 {
        base * 5.0
    } else {
        base * 10.0
    }
}

/// Format a time label for the given `relative_ns` value using the
/// magnitude of `nice_interval` to choose an appropriate unit.
pub fn format_time_label(relative_ns: f64, nice_interval: f64) -> String {
    if nice_interval >= 1_000_000_000.0 {
        format!("{:.2} s", relative_ns / 1_000_000_000.0)
    } else if nice_interval >= 1_000_000.0 {
        format!("{:.2} ms", relative_ns / 1_000_000.0)
    } else if nice_interval >= 1_000.0 {
        format!("{:.2} µs", relative_ns / 1_000.0)
    } else {
        format!("{:.0} ns", relative_ns)
    }
}
