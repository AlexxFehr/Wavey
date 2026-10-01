use glam::DVec3;
use serde::{Deserialize, Serialize};
use wavey_geom::Room;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Microphone {
    pub id: u64,
    pub name: String,
    pub position: DVec3,
}

/// Orientation of a cut plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaneAxis {
    /// Horizontal plane z = offset (u = x, v = y).
    Horizontal,
    /// Vertical plane x = offset (u = y, v = z).
    ConstX,
    /// Vertical plane y = offset (u = x, v = z).
    ConstY,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CutPlane {
    pub id: u64,
    pub name: String,
    pub axis: PlaneAxis,
    pub offset: f64,
    /// Grid spacing (m).
    pub spacing: f64,
}

/// Receiver grid of a cut plane, covering the room bounds. Points outside
/// the room are kept in the layout but masked out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaneGrid {
    pub axis: PlaneAxis,
    pub offset: f64,
    /// Center of cell (0, 0) in plane coordinates.
    pub u0: f64,
    pub v0: f64,
    pub du: f64,
    pub dv: f64,
    pub nu: usize,
    pub nv: usize,
    /// Row-major (v outer, u inner); true where the point is inside the room.
    pub inside: Vec<bool>,
}

impl PlaneGrid {
    pub fn point(&self, iu: usize, iv: usize) -> DVec3 {
        let (u, v) = (self.u0 + iu as f64 * self.du, self.v0 + iv as f64 * self.dv);
        match self.axis {
            PlaneAxis::Horizontal => DVec3::new(u, v, self.offset),
            PlaneAxis::ConstX => DVec3::new(self.offset, u, v),
            PlaneAxis::ConstY => DVec3::new(u, self.offset, v),
        }
    }

    /// Points inside the room, in row-major order.
    pub fn inside_points(&self) -> Vec<DVec3> {
        let mut pts = Vec::new();
        for iv in 0..self.nv {
            for iu in 0..self.nu {
                if self.inside[iv * self.nu + iu] {
                    pts.push(self.point(iu, iv));
                }
            }
        }
        pts
    }

    pub fn inside_count(&self) -> usize {
        self.inside.iter().filter(|b| **b).count()
    }
}

/// Limit on plane resolution, to keep runs and memory bounded.
pub const MAX_PLANE_POINTS: usize = 1_000_000;

impl CutPlane {
    pub fn grid(&self, room: &Room) -> Result<PlaneGrid, String> {
        if !(self.spacing > 0.0) {
            return Err("cut plane spacing must be positive".into());
        }
        let (lo, hi) = room.bounds();
        let (ulo, uhi, vlo, vhi) = match self.axis {
            PlaneAxis::Horizontal => (lo.x, hi.x, lo.y, hi.y),
            PlaneAxis::ConstX => (lo.y, hi.y, lo.z, hi.z),
            PlaneAxis::ConstY => (lo.x, hi.x, lo.z, hi.z),
        };
        let nu = ((uhi - ulo) / self.spacing).ceil().max(1.0) as usize;
        let nv = ((vhi - vlo) / self.spacing).ceil().max(1.0) as usize;
        if nu * nv > MAX_PLANE_POINTS {
            return Err(format!("cut plane `{}` would have {} points; increase the spacing", self.name, nu * nv));
        }
        // Center the grid within the bounds.
        let u0 = (ulo + uhi) / 2.0 - (nu as f64 - 1.0) / 2.0 * self.spacing;
        let v0 = (vlo + vhi) / 2.0 - (nv as f64 - 1.0) / 2.0 * self.spacing;
        let mut g = PlaneGrid {
            axis: self.axis,
            offset: self.offset,
            u0,
            v0,
            du: self.spacing,
            dv: self.spacing,
            nu,
            nv,
            inside: Vec::new(),
        };
        g.inside = (0..nv)
            .flat_map(|iv| (0..nu).map(move |iu| (iu, iv)))
            .map(|(iu, iv)| room.contains(g.point(iu, iv)))
            .collect();
        Ok(g)
    }
}
