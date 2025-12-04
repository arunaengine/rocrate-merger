//! Vocabulary definitions for RO-Crate consolidation
//!
//! Defines types and properties used to mark consolidated subcrates
//! and track entity provenance.

/// Base namespace for consolidation vocabulary
pub const CONSOLIDATE_NS: &str = "https://w3id.org/ro/terms/consolidate/";

/// Type for a Dataset that was originally a standalone RO-Crate
/// Added to @type array alongside "Dataset"
pub const SUBCRATE_TYPE: &str = "https://w3id.org/ro/terms/consolidate/Subcrate";

/// Short form of Subcrate type (for use with extended context)
pub const SUBCRATE_TYPE_SHORT: &str = "Subcrate";

/// Property on a Subcrate listing all entities that originated from it
/// Value is an array of @id references
pub const CONSOLIDATED_ENTITIES: &str = "https://w3id.org/ro/terms/consolidate/consolidatedEntities";

/// Short form of consolidatedEntities property
pub const CONSOLIDATED_ENTITIES_SHORT: &str = "consolidatedEntities";

/// RO-Crate conformsTo URL prefix (to detect subcrate references)
pub const ROCRATE_PROFILE_PREFIX: &str = "https://w3id.org/ro/crate/";

/// Standard metadata descriptor filename
pub const METADATA_DESCRIPTOR_ID: &str = "ro-crate-metadata.json";

/// Root entity ID
pub const ROOT_ENTITY_ID: &str = "./";

/// Context extension for consolidation vocabulary
/// Should be added to the RO-Crate context when using consolidation features
pub fn context_extension() -> serde_json::Value {
    serde_json::json!({
        "Subcrate": SUBCRATE_TYPE,
        "consolidatedEntities": {
            "@id": CONSOLIDATED_ENTITIES,
            "@container": "@set",
            "@type": "@id"
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_extension() {
        let ext = context_extension();
        assert!(ext.get("Subcrate").is_some());
        assert!(ext.get("consolidatedEntities").is_some());
    }
}
