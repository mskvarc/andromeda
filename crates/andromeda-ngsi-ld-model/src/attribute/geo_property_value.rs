//! The value of a `GeoProperty` instance (ETSI GS CIM 009 clauses 4.7 and
//! 4.5.0).
//!
//! Clause 4.7.1 makes the value a `GeoJSON` geometry, any RFC 7946 geometry but
//! a `GeometryCollection`; clause 4.7.2 also admits the geometry written as a
//! JSON string, which is kept as written; and clause 4.5.0 uses the NGSI-LD
//! Null as the value of an element a notification, a merge or a temporal
//! evolution reports deleted.

use crate::attribute::geo_property::{TypedGeometryRefusal, typed_geometry};
use andromeda_ngsi_ld_entity::{geo_value::EncodedGeometry, ngsi_ld_null::is_ngsi_ld_null};
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geojson::GeometryError, geometry::Geometry};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use serde::{Serialize, Serializer};
use thiserror::Error;

/// The value of one `GeoProperty` instance.
#[derive(Debug, Clone, PartialEq)]
pub enum GeoPropertyValue {
    /// A `GeoJSON` geometry, its `bbox` and foreign members kept.
    Geometry(Geometry),
    /// A geometry written as a JSON string (clause 4.7.2), kept as its text.
    Encoded(EncodedGeometry),
    /// The NGSI-LD Null: the geometry was deleted (clause 4.5.0).
    Null,
}

/// Why a compact node is no `GeoProperty` value.
#[derive(Debug, Error)]
pub enum GeoValueRefusal {
    /// The geometry object is not read as the typed geometry.
    #[error(transparent)]
    Typed(#[from] TypedGeometryRefusal),
    /// The geometry object is no `GeoJSON` geometry object the reader reads.
    #[error("the value is no GeoJSON geometry object: {0}")]
    Written(#[source] serde_json::Error),
    /// A string is no geometry clause 4.7.2 admits, or the value is neither a
    /// geometry object nor a string.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

// Hand-written: each variant is written as the JSON value it stands for, which
// no derive expresses without buffering.
impl Serialize for GeoPropertyValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            GeoPropertyValue::Geometry(geometry) => geometry.serialize(serializer),
            GeoPropertyValue::Encoded(encoded) => serializer.serialize_str(encoded.as_str()),
            GeoPropertyValue::Null => serializer.serialize_str(NULL_URI.as_str()),
        }
    }
}

impl GeoPropertyValue {
    /// The `GeoProperty` value a compact node is.
    ///
    /// # Errors
    ///
    /// Returns the node's [`CompactNode::Unreadable`] when it has no JSON form;
    /// the inner result refuses a geometry object that is no geometry clause
    /// 4.7.1 admits, a string that is none, and any other value.
    pub fn from_node<'n, N: CompactNode<'n>>(node: N) -> Result<Result<GeoPropertyValue, GeoValueRefusal>, N::Unreadable> {
        Ok(match node.shape() {
            NodeShape::Geometry(form) => Ok(GeoPropertyValue::Geometry(typed(form))),
            NodeShape::String(text) if is_ngsi_ld_null(&text) => Ok(GeoPropertyValue::Null),
            NodeShape::String(text) => EncodedGeometry::parse(text.into_owned())
                .map(GeoPropertyValue::Encoded)
                .map_err(GeoValueRefusal::from),
            NodeShape::Object(_) => node
                .read_geometry()?
                .map_err(GeoValueRefusal::Written)
                .and_then(|written| typed_geometry(written).map_err(GeoValueRefusal::from))
                .map(GeoPropertyValue::Geometry),
            NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Array(_) => Err(GeoValueRefusal::Geometry(GeometryError::NotAnObject)),
        })
    }

    /// The geometry, however it was written; none for the NGSI-LD Null.
    #[must_use]
    pub const fn form(&self) -> Option<&GeometryForm> {
        match self {
            GeoPropertyValue::Geometry(geometry) => Some(geometry.form()),
            GeoPropertyValue::Encoded(encoded) => Some(encoded.form()),
            GeoPropertyValue::Null => None,
        }
    }
}

/// The geometry a node holds typed.
fn typed(form: &GeometryForm) -> Geometry {
    // clone: the compact value owns its geometry, the tree keeps the one it was written from
    Geometry::new(form.clone())
}
