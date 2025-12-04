//! Transformation logic for subcrate entities
//!
//! Handles converting subcrate root entities into Subcrate-typed folder
//! entities during consolidation.

use serde_json::{json, Map, Value};

use crate::collect::extract_types;
use crate::merge::union_merge_values;
use crate::vocab::{CONSOLIDATED_ENTITIES_SHORT, ROCRATE_PROFILE_PREFIX, SUBCRATE_TYPE_SHORT};

/// Create a Subcrate-typed folder entity from a subcrate's root
///
/// This merges:
/// - The parent crate's folder reference (if any)
/// - The subcrate's root entity properties
///
/// And transforms it by:
/// - Adding "Subcrate" to @type
/// - Stripping conformsTo (RO-Crate spec)
/// - Stripping subjectOf (metadata references)
/// - Setting consolidatedEntities to all entity IDs from this subcrate
///
/// # Arguments
/// * `folder_id` - The folder @id (e.g., "./experiments/")
/// * `parent_folder` - The parent's reference to this folder (optional)
/// * `subcrate_root` - The subcrate's root entity ("./")
/// * `consolidated_entity_ids` - List of all @ids of entities that came from this subcrate
/// * `add_subcrate_type` - Whether to add the Subcrate type
pub fn create_subcrate_folder(
    folder_id: &str,
    parent_folder: Option<&Value>,
    subcrate_root: &Value,
    consolidated_entity_ids: Vec<String>,
    add_subcrate_type: bool,
) -> Value {
    let mut result = Map::new();

    // Set the @id
    result.insert("@id".to_string(), json!(folder_id));

    // Start with parent folder properties if available
    if let Some(Value::Object(parent)) = parent_folder {
        for (key, value) in parent {
            if key != "@id" && key != "@type" {
                // Skip properties we want to strip
                if should_strip_property(key, value) {
                    continue;
                }
                result.insert(key.clone(), value.clone());
            }
        }
    }

    // Merge in subcrate root properties (overwriting/merging as needed)
    if let Some(obj) = subcrate_root.as_object() {
        for (key, value) in obj {
            if key == "@id" || key == "@type" {
                continue;
            }
            // Skip properties we want to strip
            if should_strip_property(key, value) {
                continue;
            }

            match result.get(key) {
                Some(existing) => {
                    let merged = union_merge_values(existing, value);
                    result.insert(key.clone(), merged);
                }
                None => {
                    result.insert(key.clone(), value.clone());
                }
            }
        }
    }

    // Build @type array
    let mut types: Vec<String> = vec!["Dataset".to_string()];
    if add_subcrate_type {
        types.push(SUBCRATE_TYPE_SHORT.to_string());
    }

    // Add any other types from parent/subcrate (except Dataset which we already have)
    if let Some(Value::Object(parent)) = parent_folder {
        for t in extract_types(&Value::Object(parent.clone())) {
            if t != "Dataset" && !types.contains(&t) {
                types.push(t);
            }
        }
    }
    for t in extract_types(subcrate_root) {
        if t != "Dataset" && !types.contains(&t) {
            types.push(t);
        }
    }

    if types.len() == 1 {
        result.insert("@type".to_string(), json!(types[0]));
    } else {
        result.insert("@type".to_string(), json!(types));
    }

    // Set consolidatedEntities to reference all entities from this subcrate
    if !consolidated_entity_ids.is_empty() {
        let entities_list: Vec<Value> = consolidated_entity_ids
            .into_iter()
            .map(|id| json!({"@id": id}))
            .collect();
        result.insert(CONSOLIDATED_ENTITIES_SHORT.to_string(), json!(entities_list));
    }

    Value::Object(result)
}

/// Check if a property should be stripped during subcrate transformation
fn should_strip_property(key: &str, value: &Value) -> bool {
    match key {
        // Strip subjectOf (metadata file references)
        "subjectOf" => true,
        // Strip conformsTo if it points to RO-Crate spec
        "conformsTo" => is_rocrate_conforms_to(value),
        _ => false,
    }
}

/// Check if a conformsTo URL indicates an RO-Crate
fn is_rocrate_conformance(id: &str) -> bool {
    // Match both with and without trailing slash
    id.starts_with(ROCRATE_PROFILE_PREFIX)
        || id == "https://w3id.org/ro/crate"
        || id.starts_with("https://w3id.org/ro/crate#")
}

/// Check if a conformsTo value points to RO-Crate specification
fn is_rocrate_conforms_to(value: &Value) -> bool {
    let check_id = |v: &Value| -> bool {
        v.get("@id")
            .and_then(|id| id.as_str())
            .map(is_rocrate_conformance)
            .unwrap_or(false)
    };

    match value {
        Value::Object(_) => check_id(value),
        Value::Array(arr) => {
            // If ALL entries are RO-Crate specs, strip entirely
            // If mixed, we'd need more complex logic (keep non-RO-Crate ones)
            // For now, strip if any is RO-Crate
            arr.iter().any(check_id)
        }
        Value::String(s) => is_rocrate_conformance(s),
        _ => false,
    }
}

/// Strip RO-Crate specific properties from an entity
///
/// Used when keeping a subcrate reference but removing its "subcrate-ness"
pub fn strip_rocrate_properties(entity: &mut Value) {
    if let Some(obj) = entity.as_object_mut() {
        // Remove subjectOf
        obj.remove("subjectOf");

        // Remove or filter conformsTo
        if let Some(conforms_to) = obj.get("conformsTo").cloned() {
            if is_rocrate_conforms_to(&conforms_to) {
                // Check if there are non-RO-Crate conformsTo values to keep
                if let Value::Array(arr) = &conforms_to {
                    let filtered: Vec<&Value> = arr
                        .iter()
                        .filter(|v| {
                            !v.get("@id")
                                .and_then(|id| id.as_str())
                                .map(is_rocrate_conformance)
                                .unwrap_or(false)
                        })
                        .collect();

                    if filtered.is_empty() {
                        obj.remove("conformsTo");
                    } else if filtered.len() == 1 {
                        obj.insert("conformsTo".to_string(), filtered[0].clone());
                    } else {
                        obj.insert(
                            "conformsTo".to_string(),
                            Value::Array(filtered.into_iter().cloned().collect()),
                        );
                    }
                } else {
                    obj.remove("conformsTo");
                }
            }
        }
    }
}

/// Update the root entity's hasPart to include subcrate folders
pub fn update_root_has_part(root: &mut Value, subcrate_folder_ids: &[String]) {
    if let Some(obj) = root.as_object_mut() {
        let mut has_part: Vec<Value> = match obj.get("hasPart") {
            Some(Value::Array(arr)) => arr.clone(),
            Some(v) => vec![v.clone()],
            None => vec![],
        };

        for folder_id in subcrate_folder_ids {
            let reference = json!({"@id": folder_id});
            if !has_part.iter().any(|v| v == &reference) {
                has_part.push(reference);
            }
        }

        if !has_part.is_empty() {
            obj.insert("hasPart".to_string(), json!(has_part));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_subcrate_folder_basic() {
        let subcrate_root = json!({
            "@id": "./",
            "@type": "Dataset",
            "name": "Experiment Data",
            "description": "Results from experiment",
            "conformsTo": {"@id": "https://w3id.org/ro/crate/1.2"}
        });

        let result = create_subcrate_folder(
            "./experiments/",
            None,
            &subcrate_root,
            vec!["./experiments/data.csv".to_string(), "#experiments-person1".to_string()],
            true,
        );

        let obj = result.as_object().unwrap();
        assert_eq!(obj.get("@id"), Some(&json!("./experiments/")));

        // Check @type includes both Dataset and Subcrate
        let types = obj.get("@type").unwrap();
        assert!(types.as_array().unwrap().contains(&json!("Dataset")));
        assert!(types.as_array().unwrap().contains(&json!("Subcrate")));

        // Check properties preserved
        assert_eq!(obj.get("name"), Some(&json!("Experiment Data")));
        assert_eq!(obj.get("description"), Some(&json!("Results from experiment")));

        // Check conformsTo stripped
        assert!(!obj.contains_key("conformsTo"));

        // Check consolidatedEntities set (not hasPart)
        let consolidated = obj.get("consolidatedEntities").unwrap().as_array().unwrap();
        assert_eq!(consolidated.len(), 2);
        assert!(consolidated.contains(&json!({"@id": "./experiments/data.csv"})));
        assert!(consolidated.contains(&json!({"@id": "#experiments-person1"})));
    }

    #[test]
    fn test_create_subcrate_folder_with_parent() {
        let parent_folder = json!({
            "@id": "./experiments/",
            "@type": "Dataset",
            "name": "Experiments Folder",
            "conformsTo": {"@id": "https://w3id.org/ro/crate/1.2"}
        });

        let subcrate_root = json!({
            "@id": "./",
            "@type": "Dataset",
            "description": "Detailed description from subcrate",
            "author": {"@id": "https://orcid.org/0000-0001"}
        });

        let result = create_subcrate_folder(
            "./experiments/",
            Some(&parent_folder),
            &subcrate_root,
            vec![],
            true,
        );

        let obj = result.as_object().unwrap();

        // Name from parent
        assert_eq!(obj.get("name"), Some(&json!("Experiments Folder")));
        // Description from subcrate
        assert_eq!(
            obj.get("description"),
            Some(&json!("Detailed description from subcrate"))
        );
        // Author from subcrate
        assert!(obj.contains_key("author"));
    }

    #[test]
    fn test_strip_rocrate_properties() {
        let mut entity = json!({
            "@id": "./folder/",
            "@type": "Dataset",
            "conformsTo": {"@id": "https://w3id.org/ro/crate/1.2"},
            "subjectOf": {"@id": "./folder/ro-crate-metadata.json"},
            "name": "Keep this"
        });

        strip_rocrate_properties(&mut entity);

        let obj = entity.as_object().unwrap();
        assert!(!obj.contains_key("conformsTo"));
        assert!(!obj.contains_key("subjectOf"));
        assert_eq!(obj.get("name"), Some(&json!("Keep this")));
    }

    #[test]
    fn test_update_root_has_part() {
        let mut root = json!({
            "@id": "./",
            "@type": "Dataset",
            "hasPart": [{"@id": "./existing.csv"}]
        });

        update_root_has_part(&mut root, &["./experiments/".to_string(), "./data/".to_string()]);

        let has_part = root.get("hasPart").unwrap().as_array().unwrap();
        assert_eq!(has_part.len(), 3);
    }

    #[test]
    fn test_without_subcrate_type() {
        let subcrate_root = json!({
            "@id": "./",
            "@type": "Dataset"
        });

        let result = create_subcrate_folder(
            "./experiments/",
            None,
            &subcrate_root,
            vec![],
            false, // don't add Subcrate type
        );

        let types = result.get("@type").unwrap();
        // Should be just "Dataset" as a string, not array
        assert_eq!(types, &json!("Dataset"));
    }
}
