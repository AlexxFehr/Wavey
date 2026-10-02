//! The Wavey document: project data, undo/redo, solved runs and the
//! `.wavey` package format.

mod document;
pub mod meta;
mod project;

pub use document::{Document, ProjectError, Run, RunMeta};
pub use project::{DEFAULT_MATERIAL, Project, SCHEMA_VERSION};
