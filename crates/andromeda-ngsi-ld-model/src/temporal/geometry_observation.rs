//! The geometry of one observation as a durable intent carries it: the compact
//! `GeoJSON` of its positions, and beside them the `bbox` and foreign members
//! (IETF RFC 7946 sections 5 and 6.1) as the stored JSON a history row keeps
//! them in, so a history row read back from an intent renders the geometry it
//! recorded (ETSI GS CIM 009 v1.9.1 clause 4.6.4).

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_entity::{
    geometry_value::{ExpandedGeoJsonMembers, GeometryValue},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde::{Deserialize, Serializer, ser::SerializeStruct};

/// Write the geometry of an observation as its intent wire form.
///
/// # Errors
///
/// Returns the serializer's failure.
pub(crate) fn serialize_geometry<S: Serializer>(value: &GeometryValue, serializer: S) -> Result<S::Ok, S::Error> {
    let mut wire = serializer.serialize_struct("GeometryObservation", 2)?;
    wire.serialize_field("form", &value.form().to_geojson())?;
    match value.members() {
        Some(members) => wire.serialize_field("members", members)?,
        None => wire.skip_field("members")?,
    }
    wire.end()
}

/// The geometry of an observation as its intent wire form, its members still
/// raw.
#[derive(Deserialize)]
pub(crate) struct GeometryWire {
    /// The positions, as compact `GeoJSON`.
    form: Geometry,
    /// The stored JSON of the members, when the geometry has any.
    members: Option<JsonLiteral>,
}

impl GeometryWire {
    /// The geometry of `attribute` the wire form stands for.
    ///
    /// # Errors
    ///
    /// Returns the [`EntityRefusal`] of members that are not the stored members
    /// of a geometry.
    pub(crate) fn into_geometry(self, attribute: &AttributeName) -> Result<GeometryValue, EntityRefusal> {
        let (form, _) = self.form.into_parts();
        let members = self
            .members
            .map(|stored| ExpandedGeoJsonMembers::from_stored(attribute, &stored.to_string()))
            .transpose()?
            .flatten();
        Ok(GeometryValue::with_members(form, members))
    }
}
