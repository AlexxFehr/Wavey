//! Sketch model, region detection, extrusion and spatial queries.

pub mod bvh;
pub mod csg;
pub mod regions;
pub mod repair;
pub mod room;
pub mod sketch;

pub use glam::{DVec2, DVec3};
pub use repair::{RepairOptions, RepairReport, repair};
pub use room::{GeomError, Room, Surface, SurfaceKey, SurfaceKind, build_room, room_from_mesh, room_from_polygons, triangulate};
pub use sketch::{SegmentId, Sketch, VertexId};

/// Version of the Wavey core, shared by the app and the CLI.
pub const CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
