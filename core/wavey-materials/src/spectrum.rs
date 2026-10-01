use serde::{Deserialize, Serialize};

/// A frequency-dependent quantity: a single value, or a table interpolated
/// linearly over log-frequency and held constant outside its range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Spectrum {
    Scalar(f64),
    Bands { freqs_hz: Vec<f64>, values: Vec<f64> },
}

impl Default for Spectrum {
    fn default() -> Self {
        Spectrum::Scalar(0.0)
    }
}

impl Spectrum {
    pub fn bands(freqs_hz: &[f64], values: &[f64]) -> Self {
        assert_eq!(freqs_hz.len(), values.len());
        Spectrum::Bands { freqs_hz: freqs_hz.to_vec(), values: values.to_vec() }
    }

    pub fn eval(&self, f: f64) -> f64 {
        match self {
            Spectrum::Scalar(v) => *v,
            Spectrum::Bands { freqs_hz, values } => {
                let n = freqs_hz.len();
                if n == 0 {
                    return 0.0;
                }
                if f <= freqs_hz[0] {
                    return values[0];
                }
                if f >= freqs_hz[n - 1] {
                    return values[n - 1];
                }
                let i = freqs_hz.partition_point(|x| *x <= f) - 1;
                let (f0, f1) = (freqs_hz[i].ln(), freqs_hz[i + 1].ln());
                let t = (f.ln() - f0) / (f1 - f0);
                values[i] + t * (values[i + 1] - values[i])
            }
        }
    }

    pub fn eval_all(&self, freqs: &[f64]) -> Vec<f64> {
        freqs.iter().map(|f| self.eval(*f)).collect()
    }

    /// Checks frequencies are positive and strictly increasing and values lie in `range`.
    pub fn validate(&self, range: std::ops::RangeInclusive<f64>) -> Result<(), String> {
        match self {
            Spectrum::Scalar(v) if range.contains(v) => Ok(()),
            Spectrum::Scalar(v) => Err(format!("value {v} outside {range:?}")),
            Spectrum::Bands { freqs_hz, values } => {
                if freqs_hz.is_empty() || freqs_hz.len() != values.len() {
                    return Err("band table needs matching, non-empty frequency and value lists".into());
                }
                if freqs_hz[0] <= 0.0 || freqs_hz.windows(2).any(|w| w[1] <= w[0]) {
                    return Err("band frequencies must be positive and increasing".into());
                }
                match values.iter().find(|v| !range.contains(v)) {
                    Some(v) => Err(format!("value {v} outside {range:?}")),
                    None => Ok(()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_interpolation() {
        let s = Spectrum::bands(&[100.0, 1000.0], &[0.0, 1.0]);
        assert_eq!(s.eval(50.0), 0.0);
        assert_eq!(s.eval(5000.0), 1.0);
        assert!((s.eval(316.227766) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn serde_forms() {
        let a: Spectrum = serde_json::from_str("0.3").unwrap();
        assert_eq!(a, Spectrum::Scalar(0.3));
        let b: Spectrum = serde_json::from_str(r#"{"freqs_hz":[125,250],"values":[0.1,0.2]}"#).unwrap();
        assert_eq!(b.eval(125.0), 0.1);
    }
}
