use crate::material::Material;

const LIBRARY_JSON: &str = include_str!("library.json");

/// Built-in materials. Values are typical octave-band absorption
/// coefficients from published tables; treat them as starting points.
pub fn builtin_materials() -> Vec<Material> {
    let mut list: Vec<Material> = serde_json::from_str(LIBRARY_JSON).expect("valid built-in library");
    for m in &mut list {
        m.builtin = true;
    }
    list
}

/// People per m² that the library's standing audience stands for.
const STANDING_DENSITY: f64 = 3.0;

/// A standing crowd of `density` people per m² of floor it covers: the
/// library's standing audience (about 3 per m²) scaled by the share of the
/// floor each person covers, `α = 1 − (1 − α₃)^(density / 3)`. Sparse crowds
/// absorb less and packed ones more, up to 0.98.
pub fn crowd_material(density: f64) -> Material {
    let base = builtin_materials().into_iter().find(|m| m.id == "audience-standing").expect("the library has a standing audience");
    let density = density.clamp(0.1, 8.0);
    let scale = |a: f64| (1.0 - (1.0 - a).powf(density / STANDING_DENSITY)).clamp(0.0, 0.98);
    let absorption = match &base.absorption {
        crate::Spectrum::Bands { freqs_hz, values } => crate::Spectrum::Bands { freqs_hz: freqs_hz.clone(), values: values.iter().map(|a| scale(*a)).collect() },
        crate::Spectrum::Scalar(a) => crate::Spectrum::Scalar(scale(*a)),
    };
    Material {
        id: crowd_id(density),
        name: format!("Crowd, {density:.1} per m²"),
        absorption,
        builtin: false,
        source: Some("Scaled from the library's standing audience (about 3 per m²) by density; approximate".into()),
        ..base
    }
}

/// The id of the material of a crowd of `density` people per m².
pub fn crowd_id(density: f64) -> String {
    format!("crowd-{:.1}", density.clamp(0.1, 8.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_denser_crowd_absorbs_more_and_three_per_square_metre_is_the_library_audience() {
        let sparse = crowd_material(0.5);
        let dense = crowd_material(4.0);
        let lib = builtin_materials().into_iter().find(|m| m.id == "audience-standing").unwrap();
        let three = crowd_material(3.0);
        for f in [125.0, 500.0, 2000.0] {
            assert!(sparse.absorption.eval(f) < three.absorption.eval(f) && three.absorption.eval(f) < dense.absorption.eval(f), "{f}");
            assert!((three.absorption.eval(f) - lib.absorption.eval(f)).abs() < 1e-9);
            assert!(dense.absorption.eval(f) <= 0.98);
        }
        assert_eq!(three.id, "crowd-3.0");
        three.validate().unwrap();
    }

    #[test]
    fn library_is_valid() {
        let lib = builtin_materials();
        assert!(lib.len() >= 10);
        let mut ids: Vec<&str> = lib.iter().map(|m| m.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), lib.len(), "duplicate ids");
        for m in &lib {
            m.validate().unwrap_or_else(|e| panic!("{}: {e}", m.id));
        }
    }
}
