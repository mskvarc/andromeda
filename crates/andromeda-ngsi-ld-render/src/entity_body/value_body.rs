//! A Property value written as the client's own JSON or as its typed geometry.

use crate::entity_body::geometry_body::GeometryBody;
use andromeda_ngsi_ld_model::attribute::value::NgsiLdValue;
use serde::{Serialize, Serializer};

/// One clause 4.6.3 value: the client's JSON as it was written, or a clause
/// 4.7 geometry written from its positions.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueBody<'a>(pub(crate) &'a NgsiLdValue);

impl Serialize for ValueBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            NgsiLdValue::Json(json) => json.serialize(serializer),
            NgsiLdValue::Geometry(geometry) => GeometryBody(geometry.form()).serialize(serializer),
        }
    }
}

/// An ordered list of values, as a `ListProperty` holds them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ValueListBody<'a>(pub(crate) &'a [NgsiLdValue]);

impl Serialize for ValueListBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().map(ValueBody))
    }
}
