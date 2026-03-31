//! NGSI-LD vocabulary IRI constants (ETSI GS CIM 009 clause 4.6).
//!
//! Compile-time validated IRI constants for the NGSI-LD expanded vocabulary.
//! All constants are `&'static iref::Iri` produced via `static_iref::iri!()`.

use iref::Iri;
use static_iref::iri;

// -- Attribute type IRIs (clause 4.5) --

/// Expanded IRI for Property.
pub const PROPERTY: &Iri = iri!("https://uri.etsi.org/ngsi-ld/Property");
/// Expanded IRI for Relationship.
pub const RELATIONSHIP: &Iri = iri!("https://uri.etsi.org/ngsi-ld/Relationship");
/// Expanded IRI for GeoProperty.
pub const GEO_PROPERTY: &Iri = iri!("https://uri.etsi.org/ngsi-ld/GeoProperty");
/// Expanded IRI for LanguageProperty.
pub const LANGUAGE_PROPERTY: &Iri = iri!("https://uri.etsi.org/ngsi-ld/LanguageProperty");
/// Expanded IRI for VocabProperty.
pub const VOCAB_PROPERTY: &Iri = iri!("https://uri.etsi.org/ngsi-ld/VocabProperty");
/// Expanded IRI for JsonProperty.
pub const JSON_PROPERTY: &Iri = iri!("https://uri.etsi.org/ngsi-ld/JsonProperty");
/// Expanded IRI for ListProperty.
pub const LIST_PROPERTY: &Iri = iri!("https://uri.etsi.org/ngsi-ld/ListProperty");
/// Expanded IRI for ListRelationship.
pub const LIST_RELATIONSHIP: &Iri = iri!("https://uri.etsi.org/ngsi-ld/ListRelationship");

// -- Value container IRIs (clause 4.6.2) --

/// Expanded IRI for hasValue (Property value container).
pub const HAS_VALUE: &Iri = iri!("https://uri.etsi.org/ngsi-ld/hasValue");
/// Expanded IRI for hasObject (Relationship value container).
pub const HAS_OBJECT: &Iri = iri!("https://uri.etsi.org/ngsi-ld/hasObject");
/// Expanded IRI for hasLanguageMap (LanguageProperty value container).
pub const HAS_LANGUAGE_MAP: &Iri = iri!("https://uri.etsi.org/ngsi-ld/hasLanguageMap");
/// Expanded IRI for hasVocab (VocabProperty value container).
pub const HAS_VOCAB: &Iri = iri!("https://uri.etsi.org/ngsi-ld/hasVocab");
/// Expanded IRI for hasJSON (JsonProperty value container).
pub const HAS_JSON: &Iri = iri!("https://uri.etsi.org/ngsi-ld/hasJSON");
/// Expanded IRI for hasValueList (ListProperty value container).
pub const HAS_VALUE_LIST: &Iri = iri!("https://uri.etsi.org/ngsi-ld/hasValueList");
/// Expanded IRI for hasObjectList (ListRelationship value container).
pub const HAS_OBJECT_LIST: &Iri = iri!("https://uri.etsi.org/ngsi-ld/hasObjectList");

// -- Metadata IRIs (clause 4.6.3) --

/// Expanded IRI for observedAt.
pub const OBSERVED_AT: &Iri = iri!("https://uri.etsi.org/ngsi-ld/observedAt");
/// Expanded IRI for createdAt.
pub const CREATED_AT: &Iri = iri!("https://uri.etsi.org/ngsi-ld/createdAt");
/// Expanded IRI for modifiedAt.
pub const MODIFIED_AT: &Iri = iri!("https://uri.etsi.org/ngsi-ld/modifiedAt");
/// Expanded IRI for deletedAt.
pub const DELETED_AT: &Iri = iri!("https://uri.etsi.org/ngsi-ld/deletedAt");
/// Expanded IRI for datasetId.
pub const DATASET_ID: &Iri = iri!("https://uri.etsi.org/ngsi-ld/datasetId");
/// Expanded IRI for instanceId.
pub const INSTANCE_ID: &Iri = iri!("https://uri.etsi.org/ngsi-ld/instanceId");
/// Expanded IRI for unitCode.
pub const UNIT_CODE: &Iri = iri!("https://uri.etsi.org/ngsi-ld/unitCode");
/// Expanded IRI for scope.
pub const SCOPE: &Iri = iri!("https://uri.etsi.org/ngsi-ld/scope");
/// Expanded IRI for DateTime type.
pub const DATETIME_TYPE: &Iri = iri!("https://uri.etsi.org/ngsi-ld/DateTime");

// -- Short (compacted) attribute type names --

/// Short name for Property.
pub const PROPERTY_SHORT: &str = "Property";
/// Short name for Relationship.
pub const RELATIONSHIP_SHORT: &str = "Relationship";
/// Short name for GeoProperty.
pub const GEO_PROPERTY_SHORT: &str = "GeoProperty";
/// Short name for VocabProperty.
pub const VOCAB_PROPERTY_SHORT: &str = "VocabProperty";
/// Short name for LanguageProperty.
pub const LANGUAGE_PROPERTY_SHORT: &str = "LanguageProperty";
/// Short name for JsonProperty.
pub const JSON_PROPERTY_SHORT: &str = "JsonProperty";
/// Short name for ListProperty.
pub const LIST_PROPERTY_SHORT: &str = "ListProperty";
/// Short name for ListRelationship.
pub const LIST_RELATIONSHIP_SHORT: &str = "ListRelationship";

// -- Sentinel values --

/// A null/placeholder URI used when no meaningful IRI is available.
pub const NULL_URI: &Iri = iri!("urn:ngsi-ld:null");
/// The expanded IRI form of the null sentinel.
pub const NULL_IRI: &Iri = iri!("https://uri.etsi.org/ngsi-ld/null");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_type_iris_are_well_formed() {
        assert_eq!(PROPERTY.as_str(), "https://uri.etsi.org/ngsi-ld/Property");
        assert_eq!(RELATIONSHIP.as_str(), "https://uri.etsi.org/ngsi-ld/Relationship");
        assert_eq!(GEO_PROPERTY.as_str(), "https://uri.etsi.org/ngsi-ld/GeoProperty");
        assert_eq!(LIST_RELATIONSHIP.as_str(), "https://uri.etsi.org/ngsi-ld/ListRelationship");
    }

    #[test]
    fn value_container_iris_are_well_formed() {
        assert_eq!(HAS_VALUE.as_str(), "https://uri.etsi.org/ngsi-ld/hasValue");
        assert_eq!(HAS_OBJECT.as_str(), "https://uri.etsi.org/ngsi-ld/hasObject");
        assert_eq!(HAS_JSON.as_str(), "https://uri.etsi.org/ngsi-ld/hasJSON");
        assert_eq!(HAS_OBJECT_LIST.as_str(), "https://uri.etsi.org/ngsi-ld/hasObjectList");
    }

    #[test]
    fn metadata_iris_are_well_formed() {
        assert_eq!(OBSERVED_AT.as_str(), "https://uri.etsi.org/ngsi-ld/observedAt");
        assert_eq!(DATASET_ID.as_str(), "https://uri.etsi.org/ngsi-ld/datasetId");
        assert_eq!(SCOPE.as_str(), "https://uri.etsi.org/ngsi-ld/scope");
    }

    #[test]
    fn sentinel_iris_are_well_formed() {
        assert_eq!(NULL_URI.as_str(), "urn:ngsi-ld:null");
    }
}
