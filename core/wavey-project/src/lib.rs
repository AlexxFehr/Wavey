//! The Wavey document: project data, undo/redo, solved runs and the
//! `.wavey` package format.

pub mod areas;
pub mod assembly;
mod document;
pub mod import;
pub mod meta;
pub mod model;
mod project;

pub use document::{Document, ProjectError, Run, RunMeta};
pub use project::{DEFAULT_MATERIAL, MeshFace, MeshGroup, MeshModel, Project, SCHEMA_VERSION};
