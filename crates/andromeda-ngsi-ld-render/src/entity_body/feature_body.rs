//! The clause 4.5.16 `GeoJSON` envelope of a typed entity.
//!
//! Clause 4.5.16.2: a Feature carries the entity's `id` as its own, the
//! geometry clause 4.5.16.1 selects, and every other member of the entity --
//! `type` and the selected `GeoProperty` included -- in its `properties`.
//! Clause 4.5.17 gives the simplified rendering the same envelope.

use crate::{
    entity_body::{
        feature_geometry::{FeatureGeometry, feature_geometry},
        typed_entity::{IdentifierMember, TypedEntity},
    },
    geometry_selection::GeometrySelection,
};
use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

/// The `type` a Feature names itself by (IETF RFC 7946 section 3.2).
const FEATURE: &str = "Feature";

/// One entity as a `GeoJSON` Feature.
#[derive(Debug)]
pub(crate) struct FeatureBody<'a> {
    /// The entity whose members become the Feature's `properties`.
    entity: TypedEntity<'a>,
    /// The geometry clause 4.5.16.1 selected.
    geometry: FeatureGeometry<'a>,
    /// The `@context` the Feature carries, when it is the whole body.
    context: Option<&'a Value>,
}

impl<'a> FeatureBody<'a> {
    /// The Feature of `entity`, its geometry selected by `selection`.
    ///
    /// # Errors
    ///
    /// Returns the failure of rendering a selected member that is no plain
    /// `GeoProperty`.
    pub(crate) fn new(entity: &TypedEntity<'a>, selection: GeometrySelection<'_>, context: Option<&'a Value>) -> Result<FeatureBody<'a>, serde_json::Error> {
        let properties = TypedEntity {
            identifier: IdentifierMember::Promoted,
            context: None,
            ..*entity
        };
        let geometry = feature_geometry(&properties, selection)?;
        Ok(FeatureBody {
            entity: properties,
            geometry,
            context,
        })
    }
}

impl Serialize for FeatureBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut feature = serializer.serialize_map(None)?;
        feature.serialize_entry("type", FEATURE)?;
        feature.serialize_entry("geometry", &self.geometry)?;
        if self.entity.keeps(&ProjectionTerm::Id) {
            feature.serialize_entry("id", &self.entity.entity.id)?;
        }
        feature.serialize_entry("properties", &self.entity)?;
        if let Some(context) = self.context {
            feature.serialize_entry("@context", context)?;
        }
        feature.end()
    }
}
