//! Wavefront OBJ: `v` vertices, `f` polygons, groups from `usemtl` (else
//! `g`/`o` names).

use glam::DVec3;

use super::{RawModel, UpAxis};

pub fn read(text: &str) -> Result<RawModel, String> {
    let mut m = RawModel { up: UpAxis::Z, ..Default::default() };
    let has_materials = text.lines().any(|l| l.trim_start().starts_with("usemtl"));
    let mut group: Option<u32> = None;
    for (ln, line) in text.lines().enumerate() {
        let p: Vec<&str> = line.split_whitespace().collect();
        let err = |e: String| format!("line {}: {e}", ln + 1);
        match p.first().copied() {
            Some("v") if p.len() >= 4 => {
                let c: Vec<f64> = p[1..4].iter().map(|x| x.parse::<f64>().map_err(|e| err(e.to_string()))).collect::<Result<_, _>>()?;
                m.vertices.push(DVec3::new(c[0], c[1], c[2]));
            }
            Some("usemtl") if p.len() >= 2 => group = Some(m.group(&p[1..].join(" "))),
            Some("g" | "o") if p.len() >= 2 && !has_materials => group = Some(m.group(&p[1..].join(" "))),
            Some("f") if p.len() >= 4 => {
                let g = match group {
                    Some(g) => g,
                    None => *group.insert(m.group("default")),
                };
                let n = m.vertices.len() as i64;
                let v: Vec<u32> = p[1..]
                    .iter()
                    .map(|t| {
                        let i: i64 = t.split('/').next().unwrap_or("0").parse().map_err(|e: std::num::ParseIntError| err(e.to_string()))?;
                        let i = if i < 0 { n + i } else { i - 1 };
                        if i < 0 || i >= n {
                            return Err(err(format!("vertex {} out of range", i + 1)));
                        }
                        Ok(i as u32)
                    })
                    .collect::<Result<_, String>>()?;
                m.faces.push((v, g));
            }
            _ => {}
        }
    }
    Ok(m)
}
