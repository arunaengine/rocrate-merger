//! RO-Crate Consolidation Library
//!
//! This library provides tools for consolidating RO-Crate hierarchies
//! (nested subcrates) into a single flat metadata file, and for merging
//! independent RO-Crates.
//!
//! # Overview
//!
//! RO-Crates can contain references to nested subcrates (Dataset entities
//! that conform to the RO-Crate specification). This library consolidates
//! these hierarchies by:
//!
//! 1. Collecting all entities from all crates in the hierarchy
//! 2. Rewriting relative @ids to include their namespace path
//! 3. Merging entities with the same absolute @id using union semantics
//! 4. Transforming subcrate roots into Subcrate-typed folder entities
//! 5. Producing a single flat @graph with all entities
//!
//! # Vocabulary
//!
//! The library introduces a small vocabulary extension for tracking
//! consolidation provenance:
//!
//! - `Subcrate` type: Added to @type of folders that were originally standalone crates
//! - `consolidatedEntities` property: On each Subcrate, lists all entities that originated from it
//!
//! # Usage
//!
//! ## Consolidate a crate with nested subcrates
//!
//! ```ignore
//! use rocrate_consolidate::{consolidate, ConsolidateInput, ConsolidateOptions, NoOpLoader};
//!
//! let graph: Vec<serde_json::Value> = // load your crate's @graph
//! let result = consolidate(
//!     ConsolidateInput::Single(graph),
//!     &YourLoader,  // implements SubcrateLoader
//!     &ConsolidateOptions::default(),
//! )?;
//!
//! println!("{}", to_json_string(&result, true)?);
//! ```
//!
//! ## Merge two independent crates
//!
//! ```ignore
//! use rocrate_consolidate::{consolidate, ConsolidateInput, MergeCrate, NoOpLoader};
//!
//! let main_graph = // load main crate
//! let other_graph = // load other crate
//!
//! let result = consolidate(
//!     ConsolidateInput::Merge {
//!         main: main_graph,
//!         others: vec![
//!             MergeCrate {
//!                 graph: other_graph,
//!                 folder_id: "./imported-data/".to_string(),
//!                 name: Some("Imported Dataset".to_string()),
//!             },
//!         ],
//!     },
//!     &NoOpLoader,
//!     &ConsolidateOptions::default(),
//! )?;
//! ```

pub mod collect;
pub mod consolidate;
pub mod error;
pub mod id;
pub mod merge;
pub mod transform;
pub mod vocab;

// Re-export main types for convenience
pub use crate::consolidate::{
    consolidate, to_json_string, to_jsonld, ConsolidateInput, ConsolidateOptions,
    ConsolidateResult, ConsolidateStats, MergeCrate, NoOpLoader, SubcrateLoader,
};
pub use crate::error::ConsolidateError;
pub use crate::vocab::{
    CONSOLIDATED_ENTITIES, CONSOLIDATED_ENTITIES_SHORT, CONSOLIDATE_NS, SUBCRATE_TYPE,
    SUBCRATE_TYPE_SHORT,
};
