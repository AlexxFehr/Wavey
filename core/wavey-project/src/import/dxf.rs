//! DXF (ASCII): 3DFACE, polyface and polygon-mesh POLYLINEs and MESH
//! entities become faces, grouped by layer; LINE, LWPOLYLINE, 2D/3D
//! POLYLINE, ARC and CIRCLE become underlay lines. Blocks are expanded at
//! their INSERTs (position, scale, rotation about z). `$INSUNITS` gives the
//! units. Entities are assumed to lie in the world coordinate system
//! (extrusion directions other than +z are not applied).

use std::collections::HashMap;

use glam::{DAffine3, DVec3};

use super::{RawModel, UpAxis};

#[derive(Debug, Clone)]
struct Entity {
    kind: String,
    pairs: Vec<(i32, String)>,
}

impl Entity {
    fn get(&self, code: i32) -> Option<&str> {
        self.pairs.iter().find(|p| p.0 == code).map(|p| p.1.as_str())
    }
    fn f(&self, code: i32) -> f64 {
        self.get(code).and_then(|v| v.trim().parse().ok()).unwrap_or(0.0)
    }
    fn i(&self, code: i32) -> i64 {
        self.get(code).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }
    fn point(&self, k: i32) -> DVec3 {
        DVec3::new(self.f(10 + k), self.f(20 + k), self.f(30 + k))
    }
    fn layer(&self) -> &str {
        self.get(8).unwrap_or("0")
    }
}

pub fn read(text: &str) -> Result<RawModel, String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() < 2 {
        return Err("empty DXF".into());
    }
    let mut pairs = Vec::with_capacity(lines.len() / 2);
    for c in lines.chunks_exact(2) {
        let code: i32 = c[0].trim().parse().map_err(|_| format!("bad DXF group code `{}` (binary DXF is not supported)", c[0].trim()))?;
        pairs.push((code, c[1].trim().to_string()));
    }

    let mut m = RawModel { up: UpAxis::Z, ..Default::default() };
    if let Some(w) = pairs.windows(2).find(|w| w[0].0 == 9 && w[0].1 == "$INSUNITS" && w[1].0 == 70) {
        m.unit_scale = match w[1].1.as_str() {
            "1" => Some(0.0254),
            "2" => Some(0.3048),
            "4" => Some(0.001),
            "5" => Some(0.01),
            "6" => Some(1.0),
            _ => None,
        };
    }
    let mut entities: Vec<Entity> = Vec::new();
    let mut blocks: HashMap<String, Vec<Entity>> = HashMap::new();
    let mut section = String::new();
    let mut block: Option<(String, Vec<Entity>)> = None;
    let mut i = 0;
    while i < pairs.len() {
        let (code, val) = &pairs[i];
        if *code != 0 {
            i += 1;
            continue;
        }
        // One entity: this 0 pair up to the next.
        let mut j = i + 1;
        while j < pairs.len() && pairs[j].0 != 0 {
            j += 1;
        }
        let e = Entity { kind: val.clone(), pairs: pairs[i + 1..j].to_vec() };
        match e.kind.as_str() {
            "SECTION" => section = e.get(2).unwrap_or("").to_string(),
            "ENDSEC" => section.clear(),
            "BLOCK" if section == "BLOCKS" => block = Some((e.get(2).unwrap_or("").to_string(), Vec::new())),
            "ENDBLK" => {
                if let Some((name, list)) = block.take() {
                    blocks.insert(name, list);
                }
            }
            _ if section == "BLOCKS" => {
                if let Some((_, list)) = &mut block {
                    list.push(e);
                }
            }
            _ if section == "ENTITIES" => entities.push(e),
            _ => {}
        }
        i = j;
    }
    emit(&entities, DAffine3::IDENTITY, &blocks, &mut m, 0);
    Ok(m)
}

fn emit(list: &[Entity], xf: DAffine3, blocks: &HashMap<String, Vec<Entity>>, m: &mut RawModel, depth: u32) {
    let mut k = 0;
    while k < list.len() {
        let e = &list[k];
        k += 1;
        let tp = |p: DVec3| xf.transform_point3(p);
        let line = |m: &mut RawModel, a: DVec3, b: DVec3| {
            if a.distance(b) > 0.0 {
                m.lines.push([tp(a), tp(b)]);
            }
        };
        match e.kind.as_str() {
            "3DFACE" => {
                let mut pts: Vec<DVec3> = (0..4).map(|i| e.point(i)).collect();
                if pts[3] == pts[2] {
                    pts.pop();
                }
                let g = m.group(e.layer());
                m.polygon(&pts.into_iter().map(tp).collect::<Vec<_>>(), g);
            }
            "LINE" => line(m, e.point(0), e.point(1)),
            "LWPOLYLINE" => {
                let z = e.f(38);
                let xs: Vec<f64> = e.pairs.iter().filter(|p| p.0 == 10).filter_map(|p| p.1.trim().parse().ok()).collect();
                let ys: Vec<f64> = e.pairs.iter().filter(|p| p.0 == 20).filter_map(|p| p.1.trim().parse().ok()).collect();
                let pts: Vec<DVec3> = xs.iter().zip(&ys).map(|(x, y)| DVec3::new(*x, *y, z)).collect();
                polyline(m, &pts, e.i(70) & 1 != 0, &line);
            }
            "ARC" | "CIRCLE" => {
                let (c, r) = (e.point(0), e.f(40));
                let (a0, mut a1) = if e.kind == "ARC" { (e.f(50), e.f(51)) } else { (0.0, 360.0) };
                if a1 <= a0 {
                    a1 += 360.0;
                }
                let n = ((a1 - a0) / 10.0).ceil().max(1.0) as usize;
                let pts: Vec<DVec3> = (0..=n)
                    .map(|s| {
                        let a = (a0 + (a1 - a0) * s as f64 / n as f64).to_radians();
                        c + DVec3::new(r * a.cos(), r * a.sin(), 0.0)
                    })
                    .collect();
                polyline(m, &pts, false, &line);
            }
            "POLYLINE" => {
                let flags = e.i(70);
                let mut verts: Vec<&Entity> = Vec::new();
                while k < list.len() && list[k].kind == "VERTEX" {
                    verts.push(&list[k]);
                    k += 1;
                }
                if k < list.len() && list[k].kind == "SEQEND" {
                    k += 1;
                }
                let g = m.group(e.layer());
                if flags & 64 != 0 {
                    // Polyface mesh: coordinate vertices, then face records.
                    let coords: Vec<DVec3> = verts.iter().filter(|v| v.i(70) & 64 != 0).map(|v| tp(v.point(0))).collect();
                    for v in verts.iter().filter(|v| v.i(70) & 128 != 0 && v.i(70) & 64 == 0) {
                        let idx: Vec<usize> = (71..=74).map(|c| v.i(c).unsigned_abs() as usize).filter(|&i| i > 0 && i <= coords.len()).collect();
                        let mut pts: Vec<DVec3> = idx.iter().map(|&i| coords[i - 1]).collect();
                        pts.dedup();
                        if pts.len() >= 3 {
                            m.polygon(&pts, g);
                        }
                    }
                } else if flags & 16 != 0 {
                    // M × N polygon mesh.
                    let (mm, nn) = (e.i(71).max(0) as usize, e.i(72).max(0) as usize);
                    let pts: Vec<DVec3> = verts.iter().map(|v| tp(v.point(0))).collect();
                    if mm * nn == pts.len() {
                        let (cm, cn) = (flags & 1 != 0, flags & 32 != 0);
                        let rows = if cm { mm } else { mm.saturating_sub(1) };
                        let cols = if cn { nn } else { nn.saturating_sub(1) };
                        for a in 0..rows {
                            for b in 0..cols {
                                let at = |i: usize, j: usize| pts[(i % mm) * nn + (j % nn)];
                                m.polygon(&[at(a, b), at(a, b + 1), at(a + 1, b + 1), at(a + 1, b)], g);
                            }
                        }
                    }
                } else {
                    let pts: Vec<DVec3> = verts.iter().map(|v| v.point(0)).collect();
                    polyline(m, &pts, flags & 1 != 0, &line);
                }
            }
            "MESH" => {
                // 92: vertex count, then 10/20/30 each; 93: face list size, then 90 values (n, i…).
                let mut coords: Vec<DVec3> = Vec::new();
                let mut list90: Vec<usize> = Vec::new();
                let mut it = e.pairs.iter().peekable();
                let mut in_faces = false;
                while let Some((c, v)) = it.next() {
                    match c {
                        10 if !in_faces => {
                            let x: f64 = v.trim().parse().unwrap_or(0.0);
                            let y: f64 = it.next().and_then(|p| p.1.trim().parse().ok()).unwrap_or(0.0);
                            let z: f64 = it.next().and_then(|p| p.1.trim().parse().ok()).unwrap_or(0.0);
                            coords.push(tp(DVec3::new(x, y, z)));
                        }
                        93 => in_faces = true,
                        90 if in_faces => list90.push(v.trim().parse().unwrap_or(0)),
                        94 | 95 => in_faces = false,
                        _ => {}
                    }
                }
                let g = m.group(e.layer());
                let mut p = 0;
                while p < list90.len() {
                    let n = list90[p];
                    let idx = &list90[(p + 1).min(list90.len())..(p + 1 + n).min(list90.len())];
                    if idx.len() >= 3 && idx.iter().all(|&i| i < coords.len()) {
                        m.polygon(&idx.iter().map(|&i| coords[i]).collect::<Vec<_>>(), g);
                    }
                    p += n + 1;
                }
            }
            "INSERT" if depth < 8 => {
                if let Some(b) = e.get(2).and_then(|n| blocks.get(n)) {
                    let s = |c: i32| if e.get(c).is_some() { e.f(c) } else { 1.0 };
                    let local = DAffine3::from_translation(e.point(0))
                        * DAffine3::from_rotation_z(e.f(50).to_radians())
                        * DAffine3::from_scale(DVec3::new(s(41), s(42), s(43)));
                    emit(b, xf * local, blocks, m, depth + 1);
                }
            }
            _ => {}
        }
    }
}

fn polyline(m: &mut RawModel, pts: &[DVec3], closed: bool, line: &dyn Fn(&mut RawModel, DVec3, DVec3)) {
    for w in pts.windows(2) {
        line(m, w[0], w[1]);
    }
    if closed && pts.len() > 2 {
        line(m, pts[pts.len() - 1], pts[0]);
    }
}
