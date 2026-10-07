//! glTF 2.0 (.gltf with embedded or external buffers, .glb): the default
//! scene's meshes with their node transforms, grouped by material.

use std::path::Path;

use glam::{DMat4, DVec3};

use super::{RawModel, UpAxis};

pub fn read(bytes: &[u8], dir: &Path) -> Result<RawModel, String> {
    let gltf::Gltf { document, blob } = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    for b in document.buffers() {
        let data = match b.source() {
            gltf::buffer::Source::Bin => blob.clone().ok_or("missing binary chunk")?,
            gltf::buffer::Source::Uri(uri) => match uri.strip_prefix("data:") {
                Some(rest) => {
                    let (_, b64) = rest.split_once(";base64,").ok_or("unsupported data URI")?;
                    base64(b64)?
                }
                None => std::fs::read(dir.join(uri.replace("%20", " "))).map_err(|e| format!("{uri}: {e}"))?,
            },
        };
        buffers.push(data);
    }
    // glTF is Y-up.
    let mut m = RawModel { up: UpAxis::Y, ..Default::default() };
    let scene = document.default_scene().or_else(|| document.scenes().next()).ok_or("no scene")?;
    for node in scene.nodes() {
        walk(&node, DMat4::IDENTITY, &buffers, &mut m);
    }
    Ok(m)
}

fn walk(node: &gltf::Node, parent: DMat4, buffers: &[Vec<u8>], m: &mut RawModel) {
    let local = DMat4::from_cols_array_2d(&node.transform().matrix().map(|c| c.map(|v| v as f64)));
    let xf = parent * local;
    if let Some(mesh) = node.mesh() {
        for prim in mesh.primitives() {
            let reader = prim.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
            let Some(pos) = reader.read_positions() else { continue };
            let pts: Vec<DVec3> = pos.map(|p| xf.transform_point3(DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64))).collect();
            let idx: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..pts.len() as u32).collect(),
            };
            let tris: Vec<[u32; 3]> = match prim.mode() {
                gltf::mesh::Mode::Triangles => idx.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
                gltf::mesh::Mode::TriangleStrip => {
                    (0..idx.len().saturating_sub(2)).map(|i| if i % 2 == 0 { [idx[i], idx[i + 1], idx[i + 2]] } else { [idx[i + 1], idx[i], idx[i + 2]] }).collect()
                }
                gltf::mesh::Mode::TriangleFan => (1..idx.len().saturating_sub(1)).map(|i| [idx[0], idx[i], idx[i + 1]]).collect(),
                _ => continue,
            };
            let name = prim.material().name().map(str::to_string).unwrap_or_else(|| match prim.material().index() {
                Some(i) => format!("material {i}"),
                None => "default".into(),
            });
            let g = m.group(&name);
            for t in tris {
                if t.iter().all(|&i| (i as usize) < pts.len()) {
                    m.polygon(&t.map(|i| pts[i as usize]), g);
                }
            }
        }
    }
    for child in node.children() {
        walk(&child, xf, buffers, m);
    }
}

fn base64(s: &str) -> Result<Vec<u8>, String> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes() {
        if c == b'=' || c.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | val(c).ok_or("bad base64")? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_triangle_with_node_transform() {
        // One triangle (0,0,0) (1,0,0) (0,1,0) as f32 positions, node moved by (0, 0, 2).
        let pos: Vec<u8> = [0f32, 0., 0., 1., 0., 0., 0., 1., 0.].iter().flat_map(|v| v.to_le_bytes()).collect();
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut b64 = String::new();
        for c in pos.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for k in 0..4 {
                b64.push(if k <= c.len() { T[(n >> (18 - 6 * k)) as usize & 63] as char } else { '=' });
            }
        }
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"mesh":0,"translation":[0,0,2]}}],
            "materials":[{{"name":"Wood"}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"material":0}}]}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}}],
            "bufferViews":[{{"buffer":0,"byteLength":36}}],
            "buffers":[{{"byteLength":36,"uri":"data:application/octet-stream;base64,{b64}"}}]}}"#
        );
        let m = read(json.as_bytes(), Path::new(".")).unwrap();
        assert_eq!(m.groups, vec!["Wood".to_string()]);
        assert_eq!(m.faces.len(), 1);
        assert_eq!(m.vertices[1], DVec3::new(1.0, 0.0, 2.0));
        assert_eq!(m.up, UpAxis::Y);
    }
}
