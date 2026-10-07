use glam::DVec3;
use serde::{Deserialize, Serialize};
use wavey_geom::{Room, SurfaceKey, SurfaceKind};

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
    /// An audience area: the grid follows the floor (raked seating, stepped
    /// floors, balconies) at a height above it instead of lying flat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drape: Option<Drape>,
}

/// Where an audience area lies: `height` above the chosen floor surfaces
/// (all floors when empty).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Drape {
    /// Ear height above the floor (m): 1.2 seated, 1.7 standing.
    pub height: f64,
    #[serde(default)]
    pub surfaces: Vec<SurfaceKey>,
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
    /// Audience areas: height of each point (row-major like `inside`);
    /// empty for flat planes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub z: Vec<f64>,
}

impl PlaneGrid {
    pub fn point(&self, iu: usize, iv: usize) -> DVec3 {
        let (u, v) = (self.u0 + iu as f64 * self.du, self.v0 + iv as f64 * self.dv);
        if !self.z.is_empty() {
            return DVec3::new(u, v, self.z[iv * self.nu + iu]);
        }
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
        if let Some(d) = &self.drape {
            return self.draped(room, d);
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
            z: Vec::new(),
        };
        g.inside = (0..nv)
            .flat_map(|iv| (0..nu).map(move |iu| (iu, iv)))
            .map(|(iu, iv)| room.contains(g.point(iu, iv)))
            .collect();
        Ok(g)
    }

    /// The audience-area grid: over the plan extent of the chosen floors; a
    /// point is inside where looking down from above finds one of them
    /// (through anything else, such as a balcony over the stalls), at
    /// `height` above it, inside the room.
    fn draped(&self, room: &Room, d: &Drape) -> Result<PlaneGrid, String> {
        let chosen: Vec<bool> = room
            .surfaces
            .iter()
            .map(|s| if d.surfaces.is_empty() { s.kind == SurfaceKind::Floor } else { d.surfaces.contains(&s.key) })
            .collect();
        let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
        for (t, s) in room.triangles.iter().zip(&room.tri_surface) {
            if chosen[*s as usize] {
                for &i in t {
                    lo = lo.min(room.positions[i as usize]);
                    hi = hi.max(room.positions[i as usize]);
                }
            }
        }
        if !lo.x.is_finite() {
            return Err(format!("audience area `{}` has no floor surfaces", self.name));
        }
        let nu = ((hi.x - lo.x) / self.spacing).ceil().max(1.0) as usize;
        let nv = ((hi.y - lo.y) / self.spacing).ceil().max(1.0) as usize;
        if nu * nv > MAX_PLANE_POINTS {
            return Err(format!("audience area `{}` would have {} points; increase the spacing", self.name, nu * nv));
        }
        let u0 = (lo.x + hi.x) / 2.0 - (nu as f64 - 1.0) / 2.0 * self.spacing;
        let v0 = (lo.y + hi.y) / 2.0 - (nv as f64 - 1.0) / 2.0 * self.spacing;
        let top = room.bounds().1.z + 1.0;
        let mut inside = Vec::with_capacity(nu * nv);
        let mut z = Vec::with_capacity(nu * nv);
        for iv in 0..nv {
            for iu in 0..nu {
                let o = DVec3::new(u0 + iu as f64 * self.spacing, v0 + iv as f64 * self.spacing, top);
                let hit = room
                    .bvh
                    .all_hits(o, DVec3::NEG_Z, 0.0, f64::INFINITY)
                    .into_iter()
                    .find(|h| chosen[room.tri_surface[h.triangle as usize] as usize]);
                let p = hit.map(|h| o.z - h.t + d.height);
                let ok = p.is_some_and(|pz| room.contains(DVec3::new(o.x, o.y, pz)));
                inside.push(ok);
                z.push(if ok { p.unwrap() } else { f64::NAN });
            }
        }
        Ok(PlaneGrid { axis: PlaneAxis::Horizontal, offset: d.height, u0, v0, du: self.spacing, dv: self.spacing, nu, nv, inside, z })
    }
}
