//! A `GeoJSON` geometry as the model holds it (clause 4.7): its positions, and
//! the members expansion keeps beside them.
//!
//! IETF RFC 7946 gives a geometry a `bbox` (section 5) and lets it carry
//! foreign members (section 6.1); clause 4.6.4 returns the content of a value
//! as provided. The normative core `@context` (Annex B) gives `bbox` a `@list`
//! term in the `GeoJSON` vocabulary, so the expanded node holds it as
//! `https://purl.org/geojson/vocab#bbox`; a foreign member has no core term and
//! expands, as the member of any node does, under the request's context
//! (clause 4.7.2: a geometry's JSON syntax "is only preserved in the regular
//! JSON-LD representation"). The model keeps both in their expanded form, as it
//! keeps the members of a structured Property value.

use crate::expanded_value::ExpandedValue;
use andromeda_ngsi_ld_geometry::{bounding_box::BoundingBox, form::GeometryForm};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use indexmap::IndexMap;

/// A geometry: its positions, and its `bbox` and foreign members when either
/// was written.
#[derive(Clone, Debug, PartialEq)]
pub struct GeometryValue {
    /// The positions, as written.
    form: GeometryForm,
    /// The members beside the positions; `None` for a geometry without them,
    /// which then holds one empty pointer for them.
    members: Option<Box<ExpandedGeoJsonMembers>>,
}

impl GeometryValue {
    /// A geometry of positions alone.
    #[must_use]
    pub const fn new(form: GeometryForm) -> GeometryValue {
        GeometryValue { form, members: None }
    }

    /// A geometry of `form` and the members written beside it.
    #[must_use]
    pub fn with_members(form: GeometryForm, members: Option<ExpandedGeoJsonMembers>) -> GeometryValue {
        GeometryValue {
            form,
            members: members.map(Box::new),
        }
    }

    /// The positions.
    #[must_use]
    pub const fn form(&self) -> &GeometryForm {
        &self.form
    }

    /// The members beside the positions, when any was written.
    #[must_use]
    pub fn members(&self) -> Option<&ExpandedGeoJsonMembers> {
        self.members.as_deref()
    }

    /// The positions and the members, apart.
    #[must_use]
    pub fn into_parts(self) -> (GeometryForm, Option<ExpandedGeoJsonMembers>) {
        (self.form, self.members.map(|members| *members))
    }
}

/// The `bbox` and the foreign members of one geometry node, the foreign
/// members by expanded name.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpandedGeoJsonMembers {
    /// The bounding box, when one was written.
    bbox: Option<BoundingBox>,
    /// The foreign members by expanded name, in the order they were read.
    foreign: IndexMap<AttributeName, Vec<ExpandedValue>>,
}

impl ExpandedGeoJsonMembers {
    /// The members `bbox` and `foreign` make, or `None` when there is neither.
    #[must_use]
    pub fn non_empty(bbox: Option<BoundingBox>, foreign: IndexMap<AttributeName, Vec<ExpandedValue>>) -> Option<ExpandedGeoJsonMembers> {
        (bbox.is_some() || !foreign.is_empty()).then_some(ExpandedGeoJsonMembers { bbox, foreign })
    }

    /// The bounding box, when one was written.
    #[must_use]
    pub const fn bbox(&self) -> Option<&BoundingBox> {
        self.bbox.as_ref()
    }

    /// The foreign members by expanded name.
    #[must_use]
    pub const fn foreign(&self) -> &IndexMap<AttributeName, Vec<ExpandedValue>> {
        &self.foreign
    }
}
