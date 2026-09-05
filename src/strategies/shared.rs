#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Signal {
    Hold,
    Buy,
    Sell,
    Close,
}

pub fn price_cross_signal(indicator: &[f64], close: &[f64]) -> Vec<Signal> {
    let n = close.len().min(indicator.len());
    let mut sig = vec![Signal::Hold; n];
    for i in 1..n {
        if indicator[i].is_nan() || indicator[i - 1].is_nan() {
            continue;
        }
        if close[i - 1] < indicator[i - 1] && close[i] >= indicator[i] {
            sig[i] = Signal::Buy;
        } else if close[i - 1] > indicator[i - 1] && close[i] <= indicator[i] {
            sig[i] = Signal::Sell;
        }
    }
    sig
}

pub fn bars_per_year(tf: &str) -> f64 {
    match tf {
        "1m" => 525600.0,
        "5m" => 105120.0,
        "15m" => 35040.0,
        "30m" => 17520.0,
        "1h" => 8760.0,
        "2h" => 4380.0,
        "4h" => 2190.0,
        "6h" => 1460.0,
        "12h" => 730.0,
        "1d" => 365.0,
        "1w" => 52.0,
        _ => 365.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_cross_buy_then_sell() {
        let close = vec![10.0, 12.0, 15.0, 14.0, 18.0];
        let indicator = vec![11.0, 13.0, 13.0, 15.0, 16.0];
        let sig = price_cross_signal(&indicator, &close);
        assert_eq!(sig[2], Signal::Buy);
        assert_eq!(sig[3], Signal::Sell);
    }

    #[test]
    fn test_price_cross_all_hold_when_nan() {
        let close = vec![10.0, 11.0, 12.0];
        let indicator = vec![f64::NAN, f64::NAN, f64::NAN];
        let sig = price_cross_signal(&indicator, &close);
        assert!(sig.iter().all(|&s| s == Signal::Hold));
    }

    #[test]
    fn test_bars_per_year() {
        assert_eq!(bars_per_year("1h"), 8760.0);
        assert_eq!(bars_per_year("1d"), 365.0);
        assert_eq!(bars_per_year("unknown"), 365.0);
    }
}
