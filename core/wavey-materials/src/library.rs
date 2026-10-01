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

#[cfg(test)]
mod tests {
    use super::*;

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
