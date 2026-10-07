//! STL, ASCII or binary: loose triangles, one group.

use glam::DVec3;

use super::{RawModel, UpAxis};

pub fn read(bytes: &[u8]) -> Result<RawModel, String> {
    let mut m = RawModel { up: UpAxis::Z, ..Default::default() };
    let g = m.group("stl");
    // Binary when the size matches the triangle count (ASCII files may
    // also start with "solid").
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        if bytes.len() == 84 + 50 * n {
            let f = |o: usize| f32::from_le_bytes(bytes[o..o + 4].try_into().unwrap()) as f64;
            for t in 0..n {
                let o = 84 + 50 * t + 12;
                let pts: Vec<DVec3> = (0..3).map(|k| DVec3::new(f(o + 12 * k), f(o + 12 * k + 4), f(o + 12 * k + 8))).collect();
                m.polygon(&pts, g);
            }
            return Ok(m);
        }
    }
    let text = String::from_utf8_lossy(bytes);
    let mut pts: Vec<DVec3> = Vec::new();
    for line in text.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        match p.first().copied() {
            Some("vertex") if p.len() >= 4 => {
                let c: Vec<f64> = p[1..4].iter().map(|x| x.parse::<f64>().map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
                pts.push(DVec3::new(c[0], c[1], c[2]));
            }
            Some("endloop") => {
                if pts.len() >= 3 {
                    m.polygon(&pts, g);
                }
                pts.clear();
            }
            _ => {}
        }
    }
    Ok(m)
}
