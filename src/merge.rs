//! Union merge logic for RO-Crate entities
//!
//! Implements the union merge strategy for combining entities with
//! the same @id from different crates.

use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::collect::CollectedEntity;

/// Merge two JSON values using union strategy
///
/// - Equal values: keep as-is
/// - Different scalars: convert to array with both values
/// - Arrays: union of unique elements
/// - Objects: recursive merge of keys
pub fn union_merge_values(a: &Value, b: &Value) -> Value {
    if values_equal(a, b) {
        return a.clone();
    }

    match (a, b) {
        // Both arrays: union unique elements
        (Value::Array(arr_a), Value::Array(arr_b)) => {
            let mut result = arr_a.clone();
            for item in arr_b {
                if !contains_value(&result, item) {
                    result.push(item.clone());
                }
            }
            Value::Array(result)
        }
        // One array, one scalar: add scalar to array if not present
        (Value::Array(arr), other) | (other, Value::Array(arr)) => {
            let mut result = arr.clone();
            if !contains_value(&result, other) {
                result.push(other.clone());
            }
            Value::Array(result)
        }
        // Both objects: recursive merge
        (Value::Object(obj_a), Value::Object(obj_b)) => {
            let merged = merge_objects(obj_a, obj_b);
            Value::Object(merged)
        }
        // Different scalars: create array with both
        _ => {
            Value::Array(vec![a.clone(), b.clone()])
        }
    }
}

/// Merge two JSON objects, combining their keys
fn merge_objects(a: &Map<String, Value>, b: &Map<String, Value>) -> Map<String, Value> {
    let mut result = a.clone();

    for (key, value_b) in b {
        match result.get(key) {
            Some(value_a) => {
                // Key exists in both: merge values
                let merged = union_merge_values(value_a, value_b);
                result.insert(key.clone(), merged);
            }
            None => {
                // Key only in b: add it
                result.insert(key.clone(), value_b.clone());
            }
        }
    }

    result
}

/// Check if two values are semantically equal
/// Handles @id reference normalization
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(obj_a), Value::Object(obj_b)) => {
            // Special case: both are @id references
            if obj_a.len() == 1 && obj_b.len() == 1 {
                if let (Some(id_a), Some(id_b)) = (obj_a.get("@id"), obj_b.get("@id")) {
                    return id_a == id_b;
                }
            }
            obj_a == obj_b
        }
        _ => a == b,
    }
}

/// Check if an array contains a value (using semantic equality)
fn contains_value(arr: &[Value], value: &Value) -> bool {
    arr.iter().any(|v| values_equal(v, value))
}

/// Merge two entities with the same @id using union strategy
///
/// Special handling:
/// - @id: must be identical (not merged)
/// - @type: always produces array of unique types
/// - Other properties: union merge
pub fn union_merge_entities(a: &Value, b: &Value) -> Value {
    let obj_a = match a.as_object() {
        Some(o) => o,
        None => return a.clone(),
    };
    let obj_b = match b.as_object() {
        Some(o) => o,
        None => return a.clone(),
    };

    let mut result = Map::new();

    // @id must be the same - take from a
    if let Some(id) = obj_a.get("@id") {
        result.insert("@id".to_string(), id.clone());
    }

    // @type: merge into unique array
    let types_a = extract_types_as_vec(obj_a);
    let types_b = extract_types_as_vec(obj_b);
    let merged_types = merge_type_arrays(&types_a, &types_b);
    if !merged_types.is_empty() {
        if merged_types.len() == 1 {
            result.insert("@type".to_string(), Value::String(merged_types[0].clone()));
        } else {
            result.insert(
                "@type".to_string(),
                Value::Array(merged_types.into_iter().map(Value::String).collect()),
            );
        }
    }

    // Collect all other keys from both objects
    let mut all_keys: Vec<&String> = obj_a
        .keys()
        .chain(obj_b.keys())
        .filter(|k| *k != "@id" && *k != "@type")
        .collect();
    all_keys.sort();
    all_keys.dedup();

    for key in all_keys {
        let merged = match (obj_a.get(key), obj_b.get(key)) {
            (Some(va), Some(vb)) => union_merge_values(va, vb),
            (Some(v), None) | (None, Some(v)) => v.clone(),
            (None, None) => continue,
        };
        result.insert(key.clone(), merged);
    }

    Value::Object(result)
}

/// Extract @type as a vec of strings
fn extract_types_as_vec(obj: &Map<String, Value>) -> Vec<String> {
    match obj.get("@type") {
        Some(Value::String(t)) => vec![t.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}

/// Merge two type arrays, keeping unique values
fn merge_type_arrays(a: &[String], b: &[String]) -> Vec<String> {
    let mut result = a.to_vec();
    for t in b {
        if !result.contains(t) {
            result.push(t.clone());
        }
    }
    result
}

/// Group collected entities by @id and merge duplicates
///
/// Returns a vec of merged entities (as JSON Values)
pub fn merge_by_id(entities: Vec<CollectedEntity>) -> Vec<Value> {
    let mut by_id: HashMap<String, Vec<Value>> = HashMap::new();

    for collected in entities {
        by_id
            .entry(collected.original_id)
            .or_default()
            .push(collected.entity);
    }

    by_id
        .into_iter()
        .map(|(_, mut entities)| {
            if entities.len() == 1 {
                entities.pop().unwrap()
            } else {
                // Merge all entities with same ID
                entities
                    .into_iter()
                    .reduce(|acc, e| union_merge_entities(&acc, &e))
                    .unwrap()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_union_merge_scalars_equal() {
        let a = json!("test");
        let b = json!("test");
        assert_eq!(union_merge_values(&a, &b), json!("test"));
    }

    #[test]
    fn test_union_merge_scalars_different() {
        let a = json!("Donald Trump");
        let b = json!("Donald J. Trump");
        let result = union_merge_values(&a, &b);
        assert_eq!(result, json!(["Donald Trump", "Donald J. Trump"]));
    }

    #[test]
    fn test_union_merge_arrays() {
        let a = json!(["a", "b"]);
        let b = json!(["b", "c"]);
        let result = union_merge_values(&a, &b);
        assert_eq!(result, json!(["a", "b", "c"]));
    }

    #[test]
    fn test_union_merge_array_and_scalar() {
        let a = json!(["a", "b"]);
        let b = json!("c");
        let result = union_merge_values(&a, &b);
        assert_eq!(result, json!(["a", "b", "c"]));
    }

    #[test]
    fn test_union_merge_objects() {
        let a = json!({"name": "Alice", "age": 30});
        let b = json!({"name": "Alice", "city": "NYC"});
        let result = union_merge_values(&a, &b);

        let obj = result.as_object().unwrap();
        assert_eq!(obj.get("name"), Some(&json!("Alice")));
        assert_eq!(obj.get("age"), Some(&json!(30)));
        assert_eq!(obj.get("city"), Some(&json!("NYC")));
    }

    #[test]
    fn test_union_merge_entities() {
        let a = json!({
            "@id": "https://orcid.org/0000-0001",
            "@type": "Person",
            "name": "Donald Trump"
        });
        let b = json!({
            "@id": "https://orcid.org/0000-0001",
            "@type": ["Person", "Author"],
            "name": "Donald J. Trump",
            "affiliation": {"@id": "https://example.org"}
        });

        let result = union_merge_entities(&a, &b);
        let obj = result.as_object().unwrap();

        // @id unchanged
        assert_eq!(obj.get("@id"), Some(&json!("https://orcid.org/0000-0001")));

        // @type merged to unique array
        let types = obj.get("@type").unwrap();
        assert!(types.as_array().unwrap().contains(&json!("Person")));
        assert!(types.as_array().unwrap().contains(&json!("Author")));

        // name merged to array
        let name = obj.get("name").unwrap();
        assert!(name.as_array().unwrap().contains(&json!("Donald Trump")));
        assert!(name.as_array().unwrap().contains(&json!("Donald J. Trump")));

        // affiliation from b added
        assert!(obj.contains_key("affiliation"));
    }

    #[test]
    fn test_merge_by_id() {
        let entities = vec![
            CollectedEntity {
                entity: json!({"@id": "https://orcid.org/1", "name": "Alice"}),
                original_id: "https://orcid.org/1".to_string(),
                namespace: "".to_string(),
            },
            CollectedEntity {
                entity: json!({"@id": "https://orcid.org/1", "name": "Alice Smith"}),
                original_id: "https://orcid.org/1".to_string(),
                namespace: "experiments".to_string(),
            },
            CollectedEntity {
                entity: json!({"@id": "https://orcid.org/2", "name": "Bob"}),
                original_id: "https://orcid.org/2".to_string(),
                namespace: "".to_string(),
            },
        ];

        let merged = merge_by_id(entities);
        assert_eq!(merged.len(), 2);

        // Find the merged entity for orcid/1
        let alice = merged
            .iter()
            .find(|e| e.get("@id") == Some(&json!("https://orcid.org/1")))
            .unwrap();
        let name = alice.get("name").unwrap();
        // Should be array with both names
        assert!(name.is_array());
    }

    #[test]
    fn test_id_reference_dedup() {
        let a = json!([{"@id": "#person1"}, {"@id": "#person2"}]);
        let b = json!([{"@id": "#person1"}, {"@id": "#person3"}]);
        let result = union_merge_values(&a, &b);

        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 3); // person1 not duplicated
    }
}
