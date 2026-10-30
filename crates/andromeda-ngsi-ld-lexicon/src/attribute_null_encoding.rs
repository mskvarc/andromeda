//! The spelling of the NGSI-LD Null that deletes the value of one attribute
//! family (ETSI GS CIM 009 clause 4.5.0).
//!
//! Clause 4.5.0 uses the Null URI "in all places where URI strings are valid
//! JSON values", "an array with one single NGSI-LD Null" where an array is
//! required, and `{"@none": "urn:ngsi-ld:null"}` for a `languageMap`. The
//! same spelling deletes a value in a merge and reports a deleted value in a
//! notification or a temporal evolution, so it is fixed by the family alone.

use andromeda_ngsi_ld_vocabulary::{keyword, ngsi_ld_null::NULL_URI};
use serde::{Serialize, Serializer, ser::SerializeMap};
use std::iter::once;

/// The NGSI-LD Null in the spelling one family's value member takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NullEncoding {
    /// The Null URI itself: `value`, `object`, `vocab` and `json`.
    Uri,
    /// An array of the one Null, the list families' encoding (clauses
    /// 4.5.21.2 and 4.5.22.2).
    List,
    /// A language map holding the Null under `@none` (clauses 4.5.18.2 and
    /// 4.5.18.3).
    LanguageMap,
}

// Hand-written: each encoding is written as the JSON it spells, which no
// derive writes for unit variants.
impl Serialize for NullEncoding {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            NullEncoding::Uri => serializer.serialize_str(NULL_URI.as_str()),
            NullEncoding::List => serializer.collect_seq(once(NULL_URI.as_str())),
            NullEncoding::LanguageMap => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(keyword::NONE, NULL_URI.as_str())?;
                map.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_null_encoding::NullEncoding, attribute_type::AttributeType};
    use serde_json::json;
    use std::error::Error;

    #[test]
    fn each_family_is_deleted_by_the_null_its_value_member_admits() -> Result<(), Box<dyn Error>> {
        for (family, encoding, written) in [
            (AttributeType::Property, NullEncoding::Uri, json!("urn:ngsi-ld:null")),
            (AttributeType::GeoProperty, NullEncoding::Uri, json!("urn:ngsi-ld:null")),
            (AttributeType::Relationship, NullEncoding::Uri, json!("urn:ngsi-ld:null")),
            (AttributeType::VocabProperty, NullEncoding::Uri, json!("urn:ngsi-ld:null")),
            (AttributeType::JsonProperty, NullEncoding::Uri, json!("urn:ngsi-ld:null")),
            (AttributeType::ListProperty, NullEncoding::List, json!(["urn:ngsi-ld:null"])),
            (AttributeType::ListRelationship, NullEncoding::List, json!(["urn:ngsi-ld:null"])),
            (AttributeType::LanguageProperty, NullEncoding::LanguageMap, json!({"@none": "urn:ngsi-ld:null"})),
        ] {
            pretty_assertions::assert_eq!(family.null_encoding(), encoding, "{family}");
            pretty_assertions::assert_eq!(serde_json::to_value(encoding)?, written, "{family}");
        }
        Ok(())
    }
}
