//! What clause 4.9 reads of a `GeoProperty`'s geometry: the `GeoJSON`
//! geometry object it is.
//!
//! IETF RFC 7946 gives a geometry its positions, a `bbox` (section 5) and any
//! foreign members (section 6.1). Expansion keeps a foreign member as it keeps
//! a member of a structured value, by expanded name, so the members within it
//! are nodes of the model's entity state whichever value tree holds the
//! geometry; a geometry therefore reads them through a node type of its own.

use crate::queried_entity::{attribute_reading::QueriedValueNode, value_item::ValueItem};
use andromeda_ngsi_ld_geometry::{bounding_box::BoundingBox, form::GeometryForm};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::iter::Empty;

/// A geometry as the `q` evaluator reads it.
pub trait QueriedGeometry<'a>: Copy {
    /// One node of a structured value a foreign member holds.
    type Node: QueriedValueNode<'a>;

    /// The positions.
    fn form(self) -> &'a GeometryForm;

    /// The `bbox` written beside the positions, none when none was.
    fn bbox(self) -> Option<&'a BoundingBox>;

    /// The values of the foreign member keyed by the expanded name `member`,
    /// in the order they are held, or `None` when the geometry has no such
    /// member.
    fn foreign_member(self, member: &AttributeName) -> Option<impl Iterator<Item = ValueItem<'a, Self::Node>>>;
}

/// The geometry a holder of no `GeoProperty` geometry never yields,
/// uninhabited, and so also no node of one.
#[derive(Clone, Copy, Debug)]
pub enum NoGeometry {}

impl<'a> QueriedGeometry<'a> for NoGeometry {
    type Node = NoGeometry;

    fn form(self) -> &'a GeometryForm {
        match self {}
    }

    fn bbox(self) -> Option<&'a BoundingBox> {
        match self {}
    }

    // The type has no value, so no geometry of it holds a member.
    fn foreign_member(self, _member: &AttributeName) -> Option<impl Iterator<Item = ValueItem<'a, NoGeometry>>> {
        None::<Empty<ValueItem<'a, NoGeometry>>>
    }
}

impl<'a> QueriedValueNode<'a> for NoGeometry {
    fn id(self) -> Option<&'a str> {
        match self {}
    }

    // The type has no value, so no node of it holds a member.
    fn member_values(self, _member: &AttributeName) -> Option<impl Iterator<Item = ValueItem<'a, NoGeometry>>> {
        None::<Empty<ValueItem<'a, NoGeometry>>>
    }
}
