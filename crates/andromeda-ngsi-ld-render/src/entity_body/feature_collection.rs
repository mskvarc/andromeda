//! The clause 4.5.16.3 `GeoJSON` `FeatureCollection` of typed entities.
//!
//! The collection carries the body's `@context` beside its features, none of
//! which carries one of its own (clauses 4.5.16.3 and 4.5.17.2: "separate
//! @context elements for each Feature will not be present").

use crate::{contextual_body::open_object, entity_body::feature_body::FeatureBody};
use andromeda_json_literal::JsonLiteral;
use serde::{Serialize, Serializer, ser::SerializeMap};

/// The `type` a `FeatureCollection` names itself by (IETF RFC 7946 section
/// 3.3).
const FEATURE_COLLECTION: &str = "FeatureCollection";

/// The clause 4.5.16.3 `FeatureCollection`, carrying the body's `@context`
/// beside its features.
pub(crate) struct FeatureCollection<'a> {
    /// The features.
    pub(crate) features: Vec<FeatureBody<'a>>,
    /// The `@context`, if the body carries one.
    pub(crate) context: Option<&'a JsonLiteral>,
}

impl Serialize for FeatureCollection<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut collection = open_object(serializer, self.context, None)?;
        collection.serialize_entry("type", FEATURE_COLLECTION)?;
        collection.serialize_entry("features", &self.features)?;
        collection.end()
    }
}
