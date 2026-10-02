//! Image-source method: image generation and per-receiver path validation.

use std::sync::atomic::{AtomicBool, Ordering};

use glam::{DMat3, DVec3};
use rayon::prelude::*;
use wavey_geom::Room;

use crate::directivity::{Cell, Directivity};

pub const ROOT: u32 = u32::MAX;

/// Minimum source–receiver distance used in 1/r, to avoid the singularity.
pub const MIN_DISTANCE: f64 = 0.05;

#[derive(Debug, Clone, Copy)]
pub struct Image {
    pub pos: DVec3,
    /// Surface this image was mirrored across (ROOT for the source itself).
    pub surface: u32,
    pub parent: u32,
    pub order: u8,
}

/// `front[a * n + b]`: some vertex of surface `a` lies in front of surface
/// `b`'s plane. A reflection on `b` coming from `a` needs this.
fn front_table(room: &Room) -> Vec<bool> {
    let n = room.surfaces.len();
    let mut t = vec![false; n * n];
    for (a, sa) in room.surfaces.iter().enumerate() {
        for (b, sb) in room.surfaces.iter().enumerate() {
            t[a * n + b] = a != b
                && sa.triangles.iter().any(|&tri| {
                    room.triangles[tri as usize]
                        .iter()
                        .any(|&v| sb.signed_distance(room.positions[v as usize]) > 1e-9)
                });
        }
    }
    t
}

/// All images up to `max_order` that can possibly form a valid path.
pub fn generate_images(room: &Room, source: DVec3, max_order: u32) -> Vec<Image> {
    let n = room.surfaces.len();
    let front = front_table(room);
    let mut images = vec![Image { pos: source, surface: ROOT, parent: ROOT, order: 0 }];
    let mut level_start = 0;
    for order in 1..=max_order {
        let level_end = images.len();
        for i in level_start..level_end {
            let parent = images[i];
            for (s, surf) in room.surfaces.iter().enumerate() {
                if parent.surface == s as u32 {
                    continue;
                }
                // The (virtual) source must be in front of the reflecting plane…
                if surf.signed_distance(parent.pos) <= 1e-9 {
                    continue;
                }
                // …and the previous reflecting surface must face this one.
                if parent.surface != ROOT && !front[parent.surface as usize * n + s] {
                    continue;
                }
                images.push(Image { pos: surf.mirror(parent.pos), surface: s as u32, parent: i as u32, order: order as u8 });
            }
        }
        level_start = level_end;
    }
    images
}

/// One valid propagation path from a source to a receiver.
#[derive(Debug, Clone, Copy)]
pub struct PathEntry {
    /// Unfolded path length (m).
    pub r: f64,
    /// Directivity lookup for the departure direction.
    pub cell: Cell,
    /// Range into `PathSet::reflections`.
    pub refl_start: u32,
    pub order: u8,
}

/// Paths for many receivers, in compressed rows.
#[derive(Debug, Clone, Default)]
pub struct PathSet {
    /// `offsets[i]..offsets[i + 1]` are the paths of receiver `i`.
    pub offsets: Vec<u32>,
    pub paths: Vec<PathEntry>,
    /// (surface index, cos of incidence angle) per reflection.
    pub reflections: Vec<(u32, f32)>,
}

impl PathSet {
    pub fn receiver_paths(&self, i: usize) -> &[PathEntry] {
        &self.paths[self.offsets[i] as usize..self.offsets[i + 1] as usize]
    }

    pub fn reflections_of(&self, p: &PathEntry) -> &[(u32, f32)] {
        &self.reflections[p.refl_start as usize..p.refl_start as usize + p.order as usize]
    }
}

pub(crate) struct Validated {
    pub r: f64,
    pub departure: DVec3,
    pub refl: Vec<(u32, f32)>,
}

/// Checks the reflection chain of image `img` towards point `x`.
pub(crate) fn validate(room: &Room, images: &[Image], img: usize, x: DVec3) -> Option<Validated> {
    let skip = |surfaces: [u32; 2]| move |tri: u32| surfaces.contains(&room.tri_surface[tri as usize]);
    let mut refl = Vec::new();
    let mut point = x;
    let mut prev_surface = ROOT;
    let mut i = img;
    while images[i].parent != ROOT {
        let im = images[i];
        let s = &room.surfaces[im.surface as usize];
        let (da, db) = (s.signed_distance(point), s.signed_distance(im.pos));
        if da <= 1e-9 || db >= -1e-9 {
            return None;
        }
        let hit = point + (im.pos - point) * (da / (da - db));
        if !room.surface_contains(im.surface as usize, hit) {
            return None;
        }
        if room.bvh.occluded(point, hit, skip([prev_surface, im.surface])) {
            return None;
        }
        let cos = (im.pos - point).normalize().dot(s.normal).abs();
        refl.push((im.surface, cos as f32));
        prev_surface = im.surface;
        point = hit;
        i = im.parent as usize;
    }
    let source = images[i].pos;
    if room.bvh.occluded(point, source, skip([prev_surface, ROOT])) {
        return None;
    }
    let departure = (point - source).try_normalize().unwrap_or(DVec3::X);
    Some(Validated { r: (x - images[img].pos).length().max(MIN_DISTANCE), departure, refl })
}

/// Finds all valid paths from the source (images\[0\]) to each receiver.
/// Returns None if cancelled.
pub fn trace_paths(
    room: &Room,
    images: &[Image],
    to_local: DMat3,
    directivity: &Directivity,
    receivers: &[DVec3],
    cancel: &AtomicBool,
    progress: &(dyn Fn(usize) + Sync),
) -> Option<PathSet> {
    let per_receiver: Vec<Option<Vec<Validated>>> = receivers
        .par_iter()
        .map(|&x| {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            let v: Vec<Validated> = (0..images.len())
                .filter(|&k| {
                    // Fast reject: the receiver must face the last reflecting surface.
                    let s = images[k].surface;
                    s == ROOT || room.surfaces[s as usize].signed_distance(x) > 1e-9
                })
                .filter_map(|k| validate(room, images, k, x))
                .collect();
            progress(1);
            Some(v)
        })
        .collect();

    let mut set = PathSet { offsets: Vec::with_capacity(receivers.len() + 1), ..Default::default() };
    set.offsets.push(0);
    for v in per_receiver {
        for p in v? {
            set.paths.push(PathEntry {
                r: p.r,
                cell: directivity.cell(to_local * p.departure),
                refl_start: set.reflections.len() as u32,
                order: p.refl.len() as u8,
            });
            set.reflections.extend(p.refl);
        }
        set.offsets.push(set.paths.len() as u32);
    }
    Some(set)
}
