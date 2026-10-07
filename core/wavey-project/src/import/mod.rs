//! Room models from CAD and mesh files: OBJ, glTF/GLB, STL and DXF.
//!
//! Reading gives a [`RawModel`] in the file's own units and axes; the
//! [`ImportOptions`] (scale, up axis, origin) turn it into metres with z up,
//! [`repair`](wavey_geom::repair) welds and closes it, and the material per
//! group makes it a [`MeshModel`]. DXF linework (lines, polylines, arcs) is
//! kept separately as tracing underlay.

mod dxf;
mod gltf;
mod obj;
mod stl;

use std::collections::BTreeMap;
use std::path::Path;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use wavey_geom::{RepairOptions, RepairReport};
use wavey_materials::Material;

use crate::project::{DEFAULT_MATERIAL, MeshFace, MeshGroup, MeshModel};

/// A model as read: file units and axes.
#[derive(Debug, Clone, Default)]
pub struct RawModel {
    pub vertices: Vec<DVec3>,
    /// Vertex-index polygons with a group each.
    pub faces: Vec<(Vec<u32>, u32)>,
    pub groups: Vec<String>,
    /// Line segments (underlay linework).
    pub lines: Vec<[DVec3; 2]>,
    /// Metres per file unit when the file says (DXF `$INSUNITS`).
    pub unit_scale: Option<f64>,
    /// The format's usual up axis.
    pub up: UpAxis,
}

impl RawModel {
    /// Index of the group called `name`, added if new.
    pub(crate) fn group(&mut self, name: &str) -> u32 {
        match self.groups.iter().position(|g| g == name) {
            Some(i) => i as u32,
            None => {
                self.groups.push(name.to_string());
                (self.groups.len() - 1) as u32
            }
        }
    }

    /// Adds a polygon given by positions (no shared vertices; repair welds).
    pub(crate) fn polygon(&mut self, pts: &[DVec3], group: u32) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(pts);
        self.faces.push(((0..pts.len() as u32).map(|i| base + i).collect(), group));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpAxis {
    #[default]
    Z,
    Y,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Keep the file's coordinates.
    Keep,
    /// Centre in plan, lowest point at z 0.
    #[default]
    Center,
    /// Keep plan coordinates, lowest point at z 0.
    FloorAtZero,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ImportOptions {
    /// Metres per file unit.
    pub scale: f64,
    pub up: UpAxis,
    pub origin: Origin,
}

/// Reads a model file by its extension.
pub fn read(path: &Path) -> Result<RawModel, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = || String::from_utf8_lossy(&bytes).into_owned();
    let raw = match ext.as_str() {
        "obj" => obj::read(&text())?,
        "stl" => stl::read(&bytes)?,
        "dxf" => dxf::read(&text())?,
        "gltf" | "glb" => gltf::read(&bytes, path.parent().unwrap_or(Path::new(".")))?,
        _ => return Err(format!("unsupported file type `.{ext}` (use OBJ, glTF/GLB, STL or DXF)")),
    };
    if raw.faces.is_empty() && raw.lines.is_empty() {
        return Err("the file has no faces or lines".into());
    }
    Ok(raw)
}

/// Options to start from: units from the file or from the model's size
/// (a room is metres across, not hundreds or thousands), the format's up
/// axis, centred.
pub fn suggest(raw: &RawModel) -> ImportOptions {
    let scale = raw.unit_scale.unwrap_or_else(|| {
        let (lo, hi) = bounds(raw.vertices.iter().chain(raw.lines.iter().flatten()));
        // Rooms are metres to a few hundred metres across: past 2000 units
        // it is millimetres, past 400 centimetres.
        let extent = (hi - lo).max_element();
        if extent > 2000.0 {
            0.001
        } else if extent > 400.0 {
            0.01
        } else {
            1.0
        }
    });
    ImportOptions { scale, up: raw.up, origin: Origin::Center }
}

fn bounds<'a>(pts: impl Iterator<Item = &'a DVec3>) -> (DVec3, DVec3) {
    let (lo, hi) = pts.fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(a, b), p| (a.min(*p), b.max(*p)));
    if lo.x.is_finite() { (lo, hi) } else { (DVec3::ZERO, DVec3::ZERO) }
}

/// The transform from file coordinates to metres with z up.
pub fn transform(raw: &RawModel, opts: &ImportOptions) -> impl Fn(DVec3) -> DVec3 + use<> {
    let axes = opts.up;
    let s = opts.scale;
    let turn = move |p: DVec3| match axes {
        UpAxis::Z => p * s,
        // Y up (right-handed): x stays, y_new = −z, z_new = y.
        UpAxis::Y => DVec3::new(p.x, -p.z, p.y) * s,
    };
    let (lo, hi) = bounds(raw.vertices.iter().chain(raw.lines.iter().flatten()));
    let pts = [lo, hi, DVec3::new(lo.x, lo.y, hi.z), DVec3::new(lo.x, hi.y, lo.z), DVec3::new(hi.x, lo.y, lo.z)].map(turn);
    let (tlo, thi) = bounds(pts.iter());
    let shift = match opts.origin {
        Origin::Keep => DVec3::ZERO,
        Origin::Center => DVec3::new(-(tlo.x + thi.x) / 2.0, -(tlo.y + thi.y) / 2.0, -tlo.z),
        Origin::FloorAtZero => DVec3::new(0.0, 0.0, -tlo.z),
    };
    move |p: DVec3| turn(p) + shift
}

/// A material for a group name from keywords, among `materials`.
pub fn guess_material(name: &str, materials: &BTreeMap<String, Material>) -> String {
    let n = name.to_lowercase();
    const RULES: [(&[&str], &str); 14] = [
        (&["glass", "glaz", "window", "fenster", "glas"], "glass-window"),
        (&["carpet", "rug", "teppich"], "carpet-thin"),
        (&["curtain", "drape", "velour", "vorhang"], "curtain-heavy"),
        (&["audience", "seat", "chair", "stuhl", "publikum"], "seats-empty-upholstered"),
        (&["linoleum", "vinyl", "pvc"], "linoleum"),
        (&["parquet", "parkett"], "wood-floor"),
        (&["wood", "timber", "oak", "holz", "veneer"], "wood-panel"),
        (&["acoustic", "absorb", "akustik", "mineral"], "acoustic-tile"),
        (&["gypsum", "drywall", "plasterboard", "gips"], "gypsum-board"),
        (&["plaster", "putz"], "plaster"),
        (&["brick", "masonry", "ziegel"], "brick"),
        (&["floor", "boden"], "wood-floor"),
        (&["concrete", "beton", "cement"], "concrete-painted"),
        (&["opening", "void", "door_open", "proscenium"], "anechoic"),
    ];
    for (words, id) in RULES {
        if words.iter().any(|w| n.contains(w)) && materials.contains_key(id) {
            return id.to_string();
        }
    }
    if materials.contains_key(DEFAULT_MATERIAL) { DEFAULT_MATERIAL.to_string() } else { materials.keys().next().cloned().unwrap_or_default() }
}

/// The room mesh from a raw model: transformed, repaired, with a material
/// per group (`materials[group name]`, else guessed).
pub fn to_mesh(
    raw: &RawModel,
    opts: &ImportOptions,
    materials: &BTreeMap<String, String>,
    library: &BTreeMap<String, Material>,
) -> Result<(MeshModel, RepairReport), String> {
    if raw.faces.is_empty() {
        return Err("the file has no faces (only lines: use it as underlay)".into());
    }
    let tf = transform(raw, opts);
    let verts: Vec<DVec3> = raw.vertices.iter().map(|p| tf(*p)).collect();
    let (v, f, report) = wavey_geom::repair(&verts, &raw.faces, RepairOptions::default());
    // Only groups that still have faces, renumbered.
    let mut used: Vec<u32> = f.iter().map(|x| x.1).collect();
    used.sort();
    used.dedup();
    let renum: BTreeMap<u32, u32> = used.iter().enumerate().map(|(i, g)| (*g, i as u32)).collect();
    let groups = used
        .iter()
        .map(|&g| {
            let name = raw.groups.get(g as usize).cloned().unwrap_or_else(|| format!("group {g}"));
            let material = materials.get(&name).cloned().unwrap_or_else(|| guess_material(&name, library));
            MeshGroup { name, material }
        })
        .collect();
    let mesh = MeshModel {
        vertices: v.iter().map(|p| p.to_array()).collect(),
        faces: f.into_iter().map(|(v, g)| MeshFace { v, group: renum[&g] }).collect(),
        groups,
    };
    Ok((mesh, report))
}

/// Underlay linework in metres (z up) from a raw model's lines.
pub fn to_lines(raw: &RawModel, opts: &ImportOptions) -> Vec<[DVec3; 2]> {
    let tf = transform(raw, opts);
    raw.lines.iter().map(|l| [tf(l[0]), tf(l[1])]).collect()
}

/// OBJ text as a mesh (metres, z up, as stored) with the default material:
/// the project builder's path.
pub fn obj_mesh(text: &str, scale: f64) -> Result<MeshModel, String> {
    let raw = obj::read(text)?;
    if raw.faces.is_empty() {
        return Err("no faces in OBJ".into());
    }
    let groups = raw.groups.iter().map(|n| MeshGroup { name: n.clone(), material: DEFAULT_MATERIAL.into() }).collect();
    Ok(MeshModel {
        vertices: raw.vertices.iter().map(|p| (*p * scale).to_array()).collect(),
        faces: raw.faces.into_iter().map(|(v, group)| MeshFace { v, group }).collect(),
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE_OBJ: &str = "v 0 0 0\nv 4000 0 0\nv 4000 0 3000\nv 0 0 3000\nv 0 -2500 0\nv 4000 -2500 0\nv 4000 -2500 3000\nv 0 -2500 3000\n\
        usemtl Glass_Window\nf 1 2 3 4\nusemtl Concrete\nf 5 8 7 6\nf 1 5 6 2\nf 4 3 7 8\nf 1 4 8 5\nf 2 6 7 3\n";

    #[test]
    fn obj_in_millimetres_with_y_up() {
        let raw = obj::read(CUBE_OBJ).unwrap();
        let mut opts = suggest(&raw);
        assert_eq!(opts.scale, 0.001);
        opts.up = UpAxis::Y;
        let lib = wavey_materials::builtin_materials().into_iter().map(|m| (m.id.clone(), m)).collect();
        let (mesh, report) = to_mesh(&raw, &opts, &BTreeMap::new(), &lib).unwrap();
        assert!(report.watertight());
        assert_eq!(mesh.groups[0].material, "glass-window");
        let room = mesh.build().unwrap();
        let (lo, hi) = room.bounds();
        // File y (−2.5 m … 0) is up; file z (0 … 3 m) becomes −y.
        assert!((hi - lo - DVec3::new(4.0, 3.0, 2.5)).length() < 1e-9, "{lo} {hi}");
        assert!(lo.z.abs() < 1e-12 && (lo.x + hi.x).abs() < 1e-12);
        assert!((room.volume() - 30.0).abs() < 1e-9);
    }

    #[test]
    fn stl_ascii_and_binary_agree() {
        let tri = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| (a, b, c);
        let tris = [tri([0., 0., 0.], [1., 0., 0.], [0., 1., 0.]), tri([0., 0., 0.], [0., 1., 0.], [0., 0., 1.])];
        let mut ascii = String::from("solid test\n");
        let mut bin = vec![0u8; 80];
        bin.extend((tris.len() as u32).to_le_bytes());
        for (a, b, c) in tris {
            ascii += "facet normal 0 0 0\nouter loop\n";
            bin.extend([0f32; 3].iter().flat_map(|x| x.to_le_bytes()));
            for v in [a, b, c] {
                ascii += &format!("vertex {} {} {}\n", v[0], v[1], v[2]);
                bin.extend(v.iter().flat_map(|x| x.to_le_bytes()));
            }
            ascii += "endloop\nendfacet\n";
            bin.extend([0u8; 2]);
        }
        ascii += "endsolid test\n";
        let (a, b) = (stl::read(ascii.as_bytes()).unwrap(), stl::read(&bin).unwrap());
        assert_eq!(a.faces.len(), 2);
        assert_eq!(a.vertices, b.vertices);
    }

    #[test]
    fn dxf_faces_lines_and_units() {
        let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n\
            0\n3DFACE\n8\nWalls\n10\n0\n20\n0\n30\n0\n11\n1000\n21\n0\n31\n0\n12\n1000\n22\n0\n32\n2000\n13\n0\n23\n0\n33\n2000\n\
            0\nLINE\n8\nPlan\n10\n0\n20\n0\n30\n0\n11\n5000\n21\n0\n31\n0\n\
            0\nLWPOLYLINE\n8\nPlan\n90\n3\n70\n1\n10\n0\n20\n0\n10\n1000\n20\n0\n10\n1000\n20\n1000\n\
            0\nARC\n8\nPlan\n10\n0\n20\n0\n30\n0\n40\n1000\n50\n0\n51\n90\n\
            0\nENDSEC\n0\nEOF\n";
        let raw = dxf::read(dxf).unwrap();
        assert_eq!(raw.unit_scale, Some(0.001));
        assert_eq!(raw.faces.len(), 1);
        assert_eq!(raw.groups, vec!["Walls".to_string()]);
        // Line, triangle outline (closed) and a quarter arc in 10° steps.
        assert_eq!(raw.lines.len(), 1 + 3 + 9);
    }

    #[test]
    fn dxf_polyface_mesh() {
        // A square as a polyface mesh: 4 vertices, one quad face.
        let mut s = String::from("0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n8\nRoom\n66\n1\n70\n64\n71\n4\n72\n1\n");
        for (x, y) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
            s += &format!("0\nVERTEX\n8\nRoom\n10\n{x}\n20\n{y}\n30\n0\n70\n192\n");
        }
        s += "0\nVERTEX\n8\nRoom\n10\n0\n20\n0\n30\n0\n70\n128\n71\n1\n72\n2\n73\n3\n74\n-4\n0\nSEQEND\n0\nENDSEC\n0\nEOF\n";
        let raw = dxf::read(&s).unwrap();
        assert_eq!(raw.faces.len(), 1);
        assert_eq!(raw.faces[0].0.len(), 4);
    }

    #[test]
    fn material_keywords() {
        let lib: BTreeMap<String, Material> = wavey_materials::builtin_materials().into_iter().map(|m| (m.id.clone(), m)).collect();
        assert_eq!(guess_material("Seating_Block_A", &lib), "seats-empty-upholstered");
        assert_eq!(guess_material("Wand Beton", &lib), "concrete-painted");
        assert_eq!(guess_material("Layer0", &lib), DEFAULT_MATERIAL);
    }
}
