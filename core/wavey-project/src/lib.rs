//! The Wavey document: project data, undo/redo and the `.wavey` package format.

mod document;
mod project;

pub use document::{Document, ProjectError};
pub use project::{Project, SCHEMA_VERSION};
