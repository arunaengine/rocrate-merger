//! Error types for RO-Crate consolidation

use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConsolidateError {
    #[error("Failed to load crate from {path}: {reason}")]
    LoadError { path: String, reason: String },

    #[error("Invalid crate structure: {0}")]
    InvalidStructure(String),

    #[error("Cycle detected: crate '{0}' references itself in hierarchy")]
    CycleDetected(String),

    #[error("Invalid folder ID '{0}': must be a relative path ending with '/'")]
    InvalidFolderId(String),

    #[error("Duplicate folder ID '{0}': already used by another crate")]
    DuplicateFolderId(String),

    #[error("Missing root entity in crate")]
    MissingRootEntity,

    #[error("Missing metadata descriptor in crate")]
    MissingMetadataDescriptor,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Invalid path: {0}")]
    InvalidPath(PathBuf),
}
