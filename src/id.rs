//! ID classification and rewriting for RO-Crate consolidation
//!
//! Handles the transformation of entity @ids when consolidating subcrates
//! into a parent crate's namespace.

use std::collections::{HashMap, HashSet};

/// Classification of an entity @id
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdKind {
    /// Root entity: "./"
    Root,
    /// Relative path: "./foo", "./foo/bar", "foo/bar"
    Relative,
    /// Fragment identifier: "#person1", "#ctx-1"
    Fragment,
    /// Absolute URI: "https://...", "http://...", "urn:..."
    Absolute,
    /// Metadata descriptor: "ro-crate-metadata.json" or variants
    MetadataDescriptor,
}

/// Classify an @id string
pub fn classify_id(id: &str) -> IdKind {
    if id == "./" {
        IdKind::Root
    } else if id.ends_with("ro-crate-metadata.json") {
        IdKind::MetadataDescriptor
    } else if id.starts_with('#') {
        IdKind::Fragment
    } else if id.starts_with("http://")
        || id.starts_with("https://")
        || id.starts_with("urn:")
        || id.starts_with("mailto:")
        || id.starts_with("arcp:")
    {
        IdKind::Absolute
    } else {
        IdKind::Relative
    }
}

/// Rewrite an @id to include a namespace prefix
///
/// # Arguments
/// * `id` - The original @id
/// * `namespace` - The namespace prefix (e.g., "experiments" for ./experiments/)
/// * `used_fragments` - Set of already-used fragment IDs (for collision detection)
///
/// # Returns
/// The rewritten ID and whether it was actually changed
pub fn rewrite_id(
    id: &str,
    namespace: &str,
    used_fragments: &mut HashSet<String>,
) -> (String, bool) {
    if namespace.is_empty() {
        return (id.to_string(), false);
    }

    match classify_id(id) {
        IdKind::Root => {
            // "./" becomes "./namespace/"
            (format!("./{}/", namespace), true)
        }
        IdKind::Relative => {
            // "./foo" becomes "./namespace/foo"
            // "foo" becomes "./namespace/foo"
            let clean_id = id.strip_prefix("./").unwrap_or(id);
            (format!("./{}/{}", namespace, clean_id), true)
        }
        IdKind::Fragment => {
            // "#foo" stays "#foo" if unique, becomes "#namespace-foo" if collision
            if used_fragments.contains(id) {
                let new_id = format!("#{}-{}", namespace, &id[1..]);
                used_fragments.insert(new_id.clone());
                (new_id, true)
            } else {
                used_fragments.insert(id.to_string());
                (id.to_string(), false)
            }
        }
        IdKind::Absolute | IdKind::MetadataDescriptor => {
            // Absolute IDs are never rewritten
            // Metadata descriptors are dropped, not rewritten
            (id.to_string(), false)
        }
    }
}

/// Build an ID mapping for all entities in a namespace
///
/// # Arguments
/// * `ids` - Iterator of original @ids from a crate
/// * `namespace` - The namespace prefix to apply
/// * `used_fragments` - Mutable set tracking fragment ID usage across all crates
///
/// # Returns
/// HashMap from original ID to rewritten ID
pub fn build_id_map<'a>(
    ids: impl Iterator<Item = &'a str>,
    namespace: &str,
    used_fragments: &mut HashSet<String>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();

    for id in ids {
        let (rewritten, changed) = rewrite_id(id, namespace, used_fragments);
        if changed {
            map.insert(id.to_string(), rewritten);
        }
    }

    map
}

/// Rewrite @id references within a JSON value (recursive)
///
/// Finds all {"@id": "..."} patterns and rewrites them using the provided map
pub fn rewrite_references(value: &mut serde_json::Value, id_map: &HashMap<String, String>) {
    match value {
        serde_json::Value::Object(obj) => {
            // Check if this is an @id reference object
            if let Some(serde_json::Value::String(id_val)) = obj.get("@id") {
                if let Some(new_id) = id_map.get(id_val) {
                    obj.insert("@id".to_string(), serde_json::Value::String(new_id.clone()));
                }
            }
            // Recurse into all values
            for (_, v) in obj.iter_mut() {
                rewrite_references(v, id_map);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                rewrite_references(item, id_map);
            }
        }
        _ => {}
    }
}

/// Extract namespace from a folder-style @id
///
/// "./experiments/" -> "experiments"
/// "./data/raw/" -> "data/raw"
/// "https://example.org/crate/experiments/" -> "experiments"
pub fn namespace_from_folder_id(folder_id: &str) -> String {
    // Handle absolute URLs by extracting the last path segment
    if folder_id.starts_with("http://") || folder_id.starts_with("https://") {
        // Parse as URL and extract the path's last segment(s)
        let without_trailing = folder_id.trim_end_matches('/');
        if let Some(pos) = without_trailing.rfind('/') {
            let segment = &without_trailing[pos + 1..];
            if !segment.is_empty() {
                return segment.to_string();
            }
        }
        // Fallback: use the whole path after the host
        if let Some(start) = folder_id.find("://") {
            let after_scheme = &folder_id[start + 3..];
            if let Some(slash_pos) = after_scheme.find('/') {
                return after_scheme[slash_pos + 1..]
                    .trim_end_matches('/')
                    .to_string();
            }
        }
        return folder_id.to_string();
    }

    // Handle relative paths
    folder_id
        .strip_prefix("./")
        .unwrap_or(folder_id)
        .trim_end_matches('/')
        .to_string()
}

/// Validate a folder ID for use as a subcrate location
pub fn validate_folder_id(folder_id: &str) -> Result<(), String> {
    if folder_id.is_empty() {
        return Err("Folder ID cannot be empty".to_string());
    }
    if folder_id == "./" {
        return Err("Folder ID cannot be root './'".to_string());
    }
    if !folder_id.ends_with('/') {
        return Err(format!("Folder ID must end with '/': {}", folder_id));
    }
    if folder_id.starts_with("http://") || folder_id.starts_with("https://") {
        return Err("Folder ID cannot be an absolute URL".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_id() {
        assert_eq!(classify_id("./"), IdKind::Root);
        assert_eq!(classify_id("./data.csv"), IdKind::Relative);
        assert_eq!(classify_id("./experiments/"), IdKind::Relative);
        assert_eq!(classify_id("data.csv"), IdKind::Relative);
        assert_eq!(classify_id("#person1"), IdKind::Fragment);
        assert_eq!(classify_id("https://orcid.org/0000-0001"), IdKind::Absolute);
        assert_eq!(classify_id("http://example.org"), IdKind::Absolute);
        assert_eq!(classify_id("ro-crate-metadata.json"), IdKind::MetadataDescriptor);
        assert_eq!(
            classify_id("prefix-ro-crate-metadata.json"),
            IdKind::MetadataDescriptor
        );
    }

    #[test]
    fn test_rewrite_id_relative() {
        let mut fragments = HashSet::new();

        let (result, changed) = rewrite_id("./data.csv", "experiments", &mut fragments);
        assert_eq!(result, "./experiments/data.csv");
        assert!(changed);

        let (result, changed) = rewrite_id("data.csv", "experiments", &mut fragments);
        assert_eq!(result, "./experiments/data.csv");
        assert!(changed);

        let (result, changed) = rewrite_id("./", "experiments", &mut fragments);
        assert_eq!(result, "./experiments/");
        assert!(changed);
    }

    #[test]
    fn test_rewrite_id_fragment_no_collision() {
        let mut fragments = HashSet::new();

        let (result, changed) = rewrite_id("#person1", "experiments", &mut fragments);
        assert_eq!(result, "#person1");
        assert!(!changed);
        assert!(fragments.contains("#person1"));
    }

    #[test]
    fn test_rewrite_id_fragment_collision() {
        let mut fragments = HashSet::new();
        fragments.insert("#person1".to_string());

        let (result, changed) = rewrite_id("#person1", "experiments", &mut fragments);
        assert_eq!(result, "#experiments-person1");
        assert!(changed);
    }

    #[test]
    fn test_rewrite_id_absolute_unchanged() {
        let mut fragments = HashSet::new();

        let (result, changed) =
            rewrite_id("https://orcid.org/0000-0001", "experiments", &mut fragments);
        assert_eq!(result, "https://orcid.org/0000-0001");
        assert!(!changed);
    }

    #[test]
    fn test_rewrite_id_empty_namespace() {
        let mut fragments = HashSet::new();

        let (result, changed) = rewrite_id("./data.csv", "", &mut fragments);
        assert_eq!(result, "./data.csv");
        assert!(!changed);
    }

    #[test]
    fn test_namespace_from_folder_id() {
        assert_eq!(namespace_from_folder_id("./experiments/"), "experiments");
        assert_eq!(namespace_from_folder_id("./data/raw/"), "data/raw");
        assert_eq!(namespace_from_folder_id("experiments/"), "experiments");
    }

    #[test]
    fn test_validate_folder_id() {
        assert!(validate_folder_id("./experiments/").is_ok());
        assert!(validate_folder_id("./data/raw/").is_ok());
        assert!(validate_folder_id("experiments/").is_ok());

        assert!(validate_folder_id("").is_err());
        assert!(validate_folder_id("./").is_err());
        assert!(validate_folder_id("./experiments").is_err()); // missing trailing /
        assert!(validate_folder_id("https://example.org/").is_err());
    }

    #[test]
    fn test_rewrite_references() {
        let mut value = serde_json::json!({
            "@id": "./data.csv",
            "author": {"@id": "#person1"},
            "hasPart": [
                {"@id": "./file1.txt"},
                {"@id": "https://external.org/resource"}
            ]
        });

        let mut id_map = HashMap::new();
        id_map.insert("./data.csv".to_string(), "./experiments/data.csv".to_string());
        id_map.insert("#person1".to_string(), "#experiments-person1".to_string());
        id_map.insert("./file1.txt".to_string(), "./experiments/file1.txt".to_string());

        rewrite_references(&mut value, &id_map);

        assert_eq!(value["@id"], "./experiments/data.csv");
        assert_eq!(value["author"]["@id"], "#experiments-person1");
        assert_eq!(value["hasPart"][0]["@id"], "./experiments/file1.txt");
        // External reference unchanged (not in map)
        assert_eq!(value["hasPart"][1]["@id"], "https://external.org/resource");
    }
}
