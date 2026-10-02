//! The members of a `GeoJSON` geometry object beside its `type` and its
//! `coordinates`: the `bbox` of IETF RFC 7946 section 5 and the foreign members
//! of section 6.1, kept so a geometry reads back as it was written (ETSI GS CIM
//! 009 v1.9.1 clause 4.6.4).

use crate::{bounding_box::BoundingBox, foreign_members::ForeignMembers};

/// The `bbox` and the foreign members of one geometry object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeoJsonMembers {
    /// The bounding box, when one was written.
    bbox: Option<BoundingBox>,
    /// The foreign members, by name.
    foreign: ForeignMembers,
}

impl GeoJsonMembers {
    /// The members `bbox` and `foreign` make, or `None` when there is neither a
    /// box nor a foreign member, so a geometry without them holds nothing.
    #[must_use]
    pub fn non_empty(bbox: Option<BoundingBox>, foreign: ForeignMembers) -> Option<GeoJsonMembers> {
        (bbox.is_some() || !foreign.is_empty()).then_some(GeoJsonMembers { bbox, foreign })
    }

    /// The bounding box, when one was written.
    #[must_use]
    pub const fn bbox(&self) -> Option<&BoundingBox> {
        self.bbox.as_ref()
    }

    /// The foreign members, by name.
    #[must_use]
    pub const fn foreign(&self) -> &ForeignMembers {
        &self.foreign
    }
}
