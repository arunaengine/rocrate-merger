//! Entity collection from RO-Crate graphs
//!
//! Handles walking a crate's graph and collecting entities with
//! provenance tracking for consolidation.

use serde_json::Value;
use std::collections::HashSet;

use crate::id::{classify_id, IdKind};
use crate::vocab::{ROCRATE_PROFILE_PREFIX, ROOT_ENTITY_ID};

/// An entity collected from a crate's graph with provenance info
#[derive(Debug, Clone)]
pub struct CollectedEntity {
    /// The entity as a JSON value
    pub entity: Value,
    /// Original @id before any rewriting
    pub original_id: String,
    /// Namespace path this entity came from (empty string for root crate)
    pub namespace: String,
}

/// Result of collecting entities from a single crate
#[derive(Debug)]
pub struct CrateCollection {
    /// Entities with relative/local IDs (will be namespaced)
    pub local_entities: Vec<CollectedEntity>,
    /// Entities with absolute IDs (will be merged across crates)
    pub shared_entities: Vec<CollectedEntity>,
    /// IDs of discovered subcrate references (Dataset + conformsTo RO-Crate)
    pub subcrate_ids: Vec<String>,
    /// The root entity ("./") if found
    pub root_entity: Option<CollectedEntity>,
    /// The metadata descriptor entity if found
    pub metadata_descriptor: Option<CollectedEntity>,
}

/// Collect entities from a crate's graph (as JSON array)
pub fn collect_from_graph(graph: &[Value], namespace: &str) -> CrateCollection {
    let mut local_entities = Vec::new();
    let mut shared_entities = Vec::new();
    let mut subcrate_ids = Vec::new();
    let mut root_entity = None;
    let mut metadata_descriptor = None;

    for entity in graph {
        let id = match extract_id(entity) {
            Some(id) => id,
            None => continue,
        };

        let collected = CollectedEntity {
            entity: entity.clone(),
            original_id: id.to_string(),
            namespace: namespace.to_string(),
        };

        match classify_id(id) {
            IdKind::Root => {
                root_entity = Some(collected);
            }
            IdKind::MetadataDescriptor => {
                metadata_descriptor = Some(collected);
            }
            IdKind::Absolute => {
                // Check if this absolute URL is a subcrate reference
                if is_subcrate_ref(entity) {
                    subcrate_ids.push(id.to_string());
                }
                shared_entities.push(collected);
            }
            IdKind::Relative | IdKind::Fragment => {
                if is_subcrate_ref(entity) && id != ROOT_ENTITY_ID {
                    subcrate_ids.push(id.to_string());
                }
                local_entities.push(collected);
            }
        }
    }

    CrateCollection {
        local_entities,
        shared_entities,
        subcrate_ids,
        root_entity,
        metadata_descriptor,
    }
}

/// Extract @id from an entity
pub fn extract_id(entity: &Value) -> Option<&str> {
    entity.get("@id").and_then(|v| v.as_str())
}

/// Extract @type as a list of type names
pub fn extract_types(entity: &Value) -> Vec<String> {
    match entity.get("@type") {
        Some(Value::String(t)) => vec![t.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}

/// Check if an entity has a specific @type
pub fn has_type(entity: &Value, type_name: &str) -> bool {
    extract_types(entity).iter().any(|t| t == type_name)
}

/// Check if a conformsTo URL indicates an RO-Crate
fn is_rocrate_conformance(id: &str) -> bool {
    // Match both with and without trailing slash
    id.starts_with(ROCRATE_PROFILE_PREFIX)
        || id == "https://w3id.org/ro/crate"
        || id.starts_with("https://w3id.org/ro/crate#")
}

/// Check if an entity conforms to the RO-Crate specification
pub fn conforms_to_rocrate(entity: &Value) -> bool {
    let conforms_to = match entity.get("conformsTo") {
        Some(v) => v,
        None => return false,
    };

    let check_id = |v: &Value| -> bool {
        v.get("@id")
            .and_then(|id| id.as_str())
            .map(is_rocrate_conformance)
            .unwrap_or(false)
    };

    match conforms_to {
        Value::Object(_) => check_id(conforms_to),
        Value::Array(arr) => arr.iter().any(check_id),
        Value::String(s) => is_rocrate_conformance(s),
        _ => false,
    }
}

/// Check if an entity is a subcrate reference
pub fn is_subcrate_ref(entity: &Value) -> bool {
    has_type(entity, "Dataset") && conforms_to_rocrate(entity)
}

/// Check if an entity is the metadata descriptor
pub fn is_metadata_descriptor(entity: &Value) -> bool {
    if let Some(id) = extract_id(entity) {
        classify_id(id) == IdKind::MetadataDescriptor
    } else {
        false
    }
}

/// Get all @id values referenced within an entity's properties
pub fn get_referenced_ids(entity: &Value) -> HashSet<String> {
    let mut ids = HashSet::new();
    collect_referenced_ids(entity, &mut ids);
    ids
}

fn collect_referenced_ids(value: &Value, ids: &mut HashSet<String>) {
    match value {
        Value::Object(obj) => {
            if let Some(Value::String(id)) = obj.get("@id") {
                if obj.len() == 1 {
                    ids.insert(id.clone());
                }
            }
            for (key, v) in obj {
                if key != "@id" && key != "@type" {
                    collect_referenced_ids(v, ids);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_referenced_ids(item, ids);
            }
        }
        _ => {}
    }
}

/// Extract the subjectOf URL from an entity
pub fn extract_subject_of(entity: &Value) -> Option<String> {
    let subject_of = entity.get("subjectOf")?;

    let extract_id_val = |v: &Value| -> Option<String> {
        v.get("@id").and_then(|id| id.as_str()).map(String::from)
    };

    match subject_of {
        Value::Object(_) => extract_id_val(subject_of),
        Value::Array(arr) => {
            for item in arr {
                if let Some(id) = extract_id_val(item) {
                    if id.contains("ro-crate-metadata") || id.ends_with(".json") {
                        return Some(id);
                    }
                }
            }
            arr.first().and_then(extract_id_val)
        }
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_types() {
        let single = json!({"@type": "Person"});
        assert_eq!(extract_types(&single), vec!["Person"]);

        let multiple = json!({"@type": ["Dataset", "SoftwareSourceCode"]});
        assert_eq!(extract_types(&multiple), vec!["Dataset", "SoftwareSourceCode"]);
    }

    #[test]
    fn test_is_subcrate_ref() {
        let subcrate = json!({
            "@id": "./experiments/",
            "@type": "Dataset",
            "conformsTo": {"@id": "https://w3id.org/ro/crate/1.2"}
        });
        assert!(is_subcrate_ref(&subcrate));

        let regular = json!({"@id": "./data/", "@type": "Dataset"});
        assert!(!is_subcrate_ref(&regular));
    }

    #[test]
    fn test_collect_from_graph() {
        let graph = vec![
            json!({
                "@id": "ro-crate-metadata.json",
                "@type": "CreativeWork",
                "about": {"@id": "./"}
            }),
            json!({
                "@id": "./",
                "@type": "Dataset",
                "name": "Root"
            }),
            json!({
                "@id": "./data.csv",
                "@type": "File"
            }),
            json!({
                "@id": "https://orcid.org/0000-0001",
                "@type": "Person",
                "name": "Test"
            }),
            json!({
                "@id": "./experiments/",
                "@type": "Dataset",
                "conformsTo": {"@id": "https://w3id.org/ro/crate/1.2"}
            }),
        ];

        let collection = collect_from_graph(&graph, "");

        assert!(collection.root_entity.is_some());
        assert!(collection.metadata_descriptor.is_some());
        assert_eq!(collection.local_entities.len(), 2); // data.csv and experiments/
        assert_eq!(collection.shared_entities.len(), 1); // orcid
        assert_eq!(collection.subcrate_ids.len(), 1);
        assert_eq!(collection.subcrate_ids[0], "./experiments/");
    }

    #[test]
    fn test_get_referenced_ids() {
        let entity = json!({
            "@id": "./data.csv",
            "author": {"@id": "#person1"},
            "hasPart": [
                {"@id": "./file1.txt"},
                {"@id": "./file2.txt"}
            ]
        });

        let refs = get_referenced_ids(&entity);
        assert!(refs.contains("#person1"));
        assert!(refs.contains("./file1.txt"));
        assert!(refs.contains("./file2.txt"));
        assert!(!refs.contains("./data.csv")); // own ID not included
    }
}
