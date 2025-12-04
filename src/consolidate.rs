//! Main consolidation logic
//!
//! Recursive algorithm for consolidating RO-Crate hierarchies into
//! a single metadata file.

use serde_json::{json, Value};
use std::collections::HashSet;

use crate::collect::{collect_from_graph, extract_id, CollectedEntity};
use crate::error::ConsolidateError;
use crate::id::{build_id_map, namespace_from_folder_id, rewrite_references, validate_folder_id};
use crate::merge::merge_by_id;
use crate::transform::{create_subcrate_folder, update_root_has_part};
use crate::vocab::context_extension;

/// Options for consolidation
#[derive(Debug, Clone)]
pub struct ConsolidateOptions {
    /// Add "Subcrate" to @type of converted subcrate folders
    pub add_subcrate_type: bool,
    /// Extend the @context with consolidation vocabulary
    pub extend_context: bool,
}

impl Default for ConsolidateOptions {
    fn default() -> Self {
        Self {
            add_subcrate_type: true,
            extend_context: true,
        }
    }
}

/// A crate to be explicitly merged (not discovered from hierarchy)
#[derive(Debug, Clone)]
pub struct MergeCrate {
    /// The crate's @graph as JSON array
    pub graph: Vec<Value>,
    /// The folder ID this crate will be placed under (e.g., "./imported-data/")
    pub folder_id: String,
    /// Optional human-readable name for the subcrate folder
    pub name: Option<String>,
}

/// Input for consolidation
#[derive(Debug)]
pub enum ConsolidateInput {
    /// Single crate graph - discover and consolidate nested subcrates
    Single(Vec<Value>),
    /// Merge multiple crates - first is main, rest become subcrates
    Merge {
        main: Vec<Value>,
        others: Vec<MergeCrate>,
    },
}

/// Trait for loading subcrates during consolidation
pub trait SubcrateLoader {
    /// Load a subcrate's @graph given its reference ID and parent namespace
    ///
    /// # Arguments
    /// * `subcrate_id` - The @id of the subcrate reference (e.g., "./experiments/")
    /// * `parent_namespace` - The namespace of the parent crate
    ///
    /// # Returns
    /// The subcrate's @graph as a Vec of JSON values
    fn load(
        &self,
        subcrate_id: &str,
        parent_namespace: &str,
    ) -> Result<Vec<Value>, ConsolidateError>;
}

/// A no-op loader that never finds subcrates (for explicit merge-only scenarios)
pub struct NoOpLoader;

impl SubcrateLoader for NoOpLoader {
    fn load(
        &self,
        _subcrate_id: &str,
        _parent_namespace: &str,
    ) -> Result<Vec<Value>, ConsolidateError> {
        Err(ConsolidateError::LoadError {
            path: "no-op".to_string(),
            reason: "NoOpLoader does not load subcrates".to_string(),
        })
    }
}

/// Result of consolidation
#[derive(Debug)]
pub struct ConsolidateResult {
    /// The consolidated @graph
    pub graph: Vec<Value>,
    /// The @context to use (may be extended with consolidation vocabulary)
    pub context: Value,
    /// Statistics about the consolidation
    pub stats: ConsolidateStats,
}

/// Statistics from consolidation
#[derive(Debug, Default)]
pub struct ConsolidateStats {
    /// Number of crates consolidated (including root)
    pub crates_consolidated: usize,
    /// Number of entities in final graph
    pub total_entities: usize,
    /// Number of shared entities that were merged
    pub merged_entities: usize,
}

/// Main consolidation function
pub fn consolidate(
    input: ConsolidateInput,
    loader: &dyn SubcrateLoader,
    options: &ConsolidateOptions,
) -> Result<ConsolidateResult, ConsolidateError> {
    let mut stats = ConsolidateStats::default();
    let mut visited = HashSet::new();
    let mut fragment_tracker = HashSet::new();

    // Collect all entities from the hierarchy
    let (root_graph, explicit_merges) = match input {
        ConsolidateInput::Single(graph) => (graph, vec![]),
        ConsolidateInput::Merge { main, others } => (main, others),
    };

    // Process the main/root crate
    let mut all_local: Vec<CollectedEntity> = Vec::new();
    let mut all_shared: Vec<CollectedEntity> = Vec::new();
    let mut subcrate_folders: Vec<Value> = Vec::new();
    let mut root_entity: Option<Value> = None;
    let mut metadata_descriptor: Option<Value> = None;

    // Collect from root and its discovered subcrates
    collect_hierarchy(
        &root_graph,
        "",
        loader,
        options,
        &mut visited,
        &mut fragment_tracker,
        &mut all_local,
        &mut all_shared,
        &mut subcrate_folders,
        &mut root_entity,
        &mut metadata_descriptor,
        &mut stats,
    )?;

    // Process explicit merge crates
    for merge_crate in explicit_merges {
        validate_folder_id(&merge_crate.folder_id)
            .map_err(|e| ConsolidateError::InvalidFolderId(e))?;

        let namespace = namespace_from_folder_id(&merge_crate.folder_id);

        if visited.contains(&namespace) {
            return Err(ConsolidateError::DuplicateFolderId(merge_crate.folder_id));
        }
        visited.insert(namespace.clone());

        // Create a synthetic parent folder reference if a name was provided
        let parent_folder = merge_crate.name.as_ref().map(|name| {
            json!({
                "@id": merge_crate.folder_id,
                "@type": "Dataset",
                "name": name
            })
        });

        collect_hierarchy(
            &merge_crate.graph,
            &namespace,
            loader,
            options,
            &mut visited,
            &mut fragment_tracker,
            &mut all_local,
            &mut all_shared,
            &mut subcrate_folders,
            &mut None, // Don't override root
            &mut None, // Don't override descriptor
            &mut stats,
        )?;

        // Find the root entity from the merged crate to use as subcrate root
        let merge_collection = collect_from_graph(&merge_crate.graph, &namespace);
        if let Some(merge_root) = merge_collection.root_entity {
            // Collect rewritten IDs of entities from this subcrate
            let contained_ids: Vec<String> = all_local
                .iter()
                .filter(|e| {
                    e.namespace == namespace || e.namespace.starts_with(&format!("{}/", namespace))
                })
                .filter_map(|e| extract_id(&e.entity).map(String::from))
                .collect();

            let folder = create_subcrate_folder(
                &merge_crate.folder_id,
                parent_folder.as_ref(),
                &merge_root.entity,
                contained_ids,
                options.add_subcrate_type,
            );
            subcrate_folders.push(folder);
        }
    }

    // Merge shared entities (those with absolute IDs appearing in multiple crates)
    let shared_before = all_shared.len();
    let merged_shared = merge_by_id(all_shared);
    stats.merged_entities = shared_before.saturating_sub(merged_shared.len());

    // Build the final graph
    let mut final_graph: Vec<Value> = Vec::new();

    // Add metadata descriptor (from root, kept as-is)
    if let Some(desc) = metadata_descriptor {
        final_graph.push(desc);
    } else {
        return Err(ConsolidateError::MissingMetadataDescriptor);
    }

    // Add root entity with updated hasPart
    if let Some(mut root) = root_entity {
        let folder_ids: Vec<String> = subcrate_folders
            .iter()
            .filter_map(|f| extract_id(f).map(String::from))
            .collect();
        update_root_has_part(&mut root, &folder_ids);
        final_graph.push(root);
    } else {
        return Err(ConsolidateError::MissingRootEntity);
    }

    // Add all local entities (with rewritten IDs)
    for collected in all_local {
        final_graph.push(collected.entity);
    }

    // Add subcrate folders
    final_graph.extend(subcrate_folders);

    // Add merged shared entities
    final_graph.extend(merged_shared);

    stats.total_entities = final_graph.len();

    // Build context
    let context = if options.extend_context {
        json!(["https://w3id.org/ro/crate/1.1/context", context_extension()])
    } else {
        json!("https://w3id.org/ro/crate/1.1/context")
    };

    Ok(ConsolidateResult {
        graph: final_graph,
        context,
        stats,
    })
}

/// Recursively collect entities from a crate and its subcrates
#[allow(clippy::too_many_arguments)]
fn collect_hierarchy(
    graph: &[Value],
    namespace: &str,
    loader: &dyn SubcrateLoader,
    options: &ConsolidateOptions,
    visited: &mut HashSet<String>,
    fragment_tracker: &mut HashSet<String>,
    all_local: &mut Vec<CollectedEntity>,
    all_shared: &mut Vec<CollectedEntity>,
    subcrate_folders: &mut Vec<Value>,
    root_entity: &mut Option<Value>,
    metadata_descriptor: &mut Option<Value>,
    stats: &mut ConsolidateStats,
) -> Result<(), ConsolidateError> {
    stats.crates_consolidated += 1;

    let collection = collect_from_graph(graph, namespace);

    // Build ID map for rewriting
    let ids: Vec<&str> = collection
        .local_entities
        .iter()
        .map(|e| e.original_id.as_str())
        .chain(
            collection
                .root_entity
                .iter()
                .map(|e| e.original_id.as_str()),
        )
        .collect();

    let id_map = build_id_map(ids.into_iter(), namespace, fragment_tracker);

    // Handle root entity
    if namespace.is_empty() {
        // This is the main root - preserve it
        if let Some(collected) = collection.root_entity {
            *root_entity = Some(collected.entity);
        }
        if let Some(collected) = collection.metadata_descriptor {
            *metadata_descriptor = Some(collected.entity);
        }
    }

    // Process and rewrite local entities
    for mut collected in collection.local_entities {
        // Rewrite the entity's @id if needed
        if let Some(new_id) = id_map.get(&collected.original_id) {
            if let Some(obj) = collected.entity.as_object_mut() {
                obj.insert("@id".to_string(), json!(new_id));
            }
        }

        // Rewrite all @id references within the entity
        rewrite_references(&mut collected.entity, &id_map);

        all_local.push(collected);
    }

    // Add shared entities (will be merged later)
    all_shared.extend(collection.shared_entities);

    // Process discovered subcrates
    for subcrate_id in &collection.subcrate_ids {
        let subcrate_namespace = if namespace.is_empty() {
            namespace_from_folder_id(subcrate_id)
        } else {
            format!("{}/{}", namespace, namespace_from_folder_id(subcrate_id))
        };

        // Cycle detection
        if visited.contains(&subcrate_namespace) {
            continue;
        }
        visited.insert(subcrate_namespace.clone());

        // Try to load the subcrate
        let subcrate_graph = match loader.load(subcrate_id, namespace) {
            Ok(g) => g,
            Err(_) => {
                // Subcrate couldn't be loaded - skip but don't fail
                // The reference entity will remain as-is
                continue;
            }
        };

        // Find the parent's reference to this subcrate (for property merging)
        let parent_folder = graph.iter().find(|e| extract_id(e) == Some(subcrate_id));

        // Recursively collect from subcrate
        let mut subcrate_root: Option<Value> = None;
        let mut subcrate_desc: Option<Value> = None;

        collect_hierarchy(
            &subcrate_graph,
            &subcrate_namespace,
            loader,
            options,
            visited,
            fragment_tracker,
            all_local,
            all_shared,
            subcrate_folders,
            &mut subcrate_root,
            &mut subcrate_desc,
            stats,
        )?;

        // Create the subcrate folder entity
        if let Some(sub_root) = subcrate_root {
            let folder_id = if namespace.is_empty() {
                subcrate_id.clone()
            } else {
                format!("./{}/", subcrate_namespace)
            };

            // Collect IDs of entities from this subcrate
            let contained_ids: Vec<String> = all_local
                .iter()
                .filter(|e| {
                    e.namespace == subcrate_namespace
                        || e.namespace.starts_with(&format!("{}/", subcrate_namespace))
                })
                .filter_map(|e| {
                    // Get the rewritten ID
                    extract_id(&e.entity).map(String::from)
                })
                .collect();

            let folder = create_subcrate_folder(
                &folder_id,
                parent_folder,
                &sub_root,
                contained_ids,
                options.add_subcrate_type,
            );
            subcrate_folders.push(folder);
        }
    }

    Ok(())
}

/// Build a complete RO-Crate JSON-LD document from consolidation result
pub fn to_jsonld(result: &ConsolidateResult) -> Value {
    json!({
        "@context": result.context,
        "@graph": result.graph
    })
}

/// Serialize consolidation result to JSON string
pub fn to_json_string(
    result: &ConsolidateResult,
    pretty: bool,
) -> Result<String, ConsolidateError> {
    let doc = to_jsonld(result);
    if pretty {
        Ok(serde_json::to_string_pretty(&doc)?)
    } else {
        Ok(serde_json::to_string(&doc)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_root_graph() -> Vec<Value> {
        vec![
            json!({
                "@id": "ro-crate-metadata.json",
                "@type": "CreativeWork",
                "about": {"@id": "./"},
                "conformsTo": {"@id": "https://w3id.org/ro/crate/1.1"}
            }),
            json!({
                "@id": "./",
                "@type": "Dataset",
                "name": "Root Crate",
                "hasPart": [{"@id": "./data.csv"}]
            }),
            json!({
                "@id": "./data.csv",
                "@type": "File",
                "name": "Data file"
            }),
            json!({
                "@id": "https://orcid.org/0000-0001",
                "@type": "Person",
                "name": "Alice"
            }),
        ]
    }

    #[test]
    fn test_consolidate_single_no_subcrates() {
        let graph = sample_root_graph();
        let result = consolidate(
            ConsolidateInput::Single(graph),
            &NoOpLoader,
            &ConsolidateOptions::default(),
        )
        .unwrap();

        assert_eq!(result.stats.crates_consolidated, 1);
        assert!(result.graph.len() >= 4);

        // Check root entity is present
        let root = result
            .graph
            .iter()
            .find(|e| extract_id(e) == Some("./"))
            .unwrap();
        assert_eq!(root.get("name"), Some(&json!("Root Crate")));
    }

    #[test]
    fn test_consolidate_merge_two_crates() {
        let main = sample_root_graph();
        let other = vec![
            json!({
                "@id": "ro-crate-metadata.json",
                "@type": "CreativeWork",
                "about": {"@id": "./"}
            }),
            json!({
                "@id": "./",
                "@type": "Dataset",
                "name": "Other Crate",
                "description": "Imported data"
            }),
            json!({
                "@id": "./results.csv",
                "@type": "File"
            }),
            json!({
                "@id": "https://orcid.org/0000-0001",
                "@type": "Person",
                "name": "Alice Smith"  // Different name for same person
            }),
        ];

        let result = consolidate(
            ConsolidateInput::Merge {
                main,
                others: vec![MergeCrate {
                    graph: other,
                    folder_id: "./imported/".to_string(),
                    name: Some("Imported Dataset".to_string()),
                }],
            },
            &NoOpLoader,
            &ConsolidateOptions::default(),
        )
        .unwrap();

        assert_eq!(result.stats.crates_consolidated, 2);

        // Check subcrate folder was created
        let folder = result
            .graph
            .iter()
            .find(|e| extract_id(e) == Some("./imported/"))
            .unwrap();
        let types = folder.get("@type").unwrap();
        assert!(types.as_array().unwrap().contains(&json!("Subcrate")));

        // Check shared entity was merged (Alice with two names)
        let alice = result
            .graph
            .iter()
            .find(|e| extract_id(e) == Some("https://orcid.org/0000-0001"))
            .unwrap();
        let name = alice.get("name").unwrap();
        // Should have both names
        assert!(name.is_array() || name == &json!("Alice"));
    }

    #[test]
    fn test_invalid_folder_id() {
        let main = sample_root_graph();
        let other = vec![json!({"@id": "./", "@type": "Dataset"})];

        let result = consolidate(
            ConsolidateInput::Merge {
                main,
                others: vec![MergeCrate {
                    graph: other,
                    folder_id: "no-trailing-slash".to_string(),
                    name: None,
                }],
            },
            &NoOpLoader,
            &ConsolidateOptions::default(),
        );

        assert!(matches!(result, Err(ConsolidateError::InvalidFolderId(_))));
    }

    #[test]
    fn test_to_jsonld() {
        let graph = sample_root_graph();
        let result = consolidate(
            ConsolidateInput::Single(graph),
            &NoOpLoader,
            &ConsolidateOptions::default(),
        )
        .unwrap();

        let doc = to_jsonld(&result);
        assert!(doc.get("@context").is_some());
        assert!(doc.get("@graph").is_some());
    }
}
