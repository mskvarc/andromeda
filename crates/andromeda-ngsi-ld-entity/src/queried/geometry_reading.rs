//! A `GeoProperty`'s geometry as the `q` evaluator reads it: its positions,
//! and the `bbox` and the foreign members the model keeps beside them.
//!
//! The model keeps a foreign member in the expanded form expansion gives it,
//! as it keeps a member of a structured value, so its values are read as
//! that member's are. A written fragment holds them in that same entity-state
//! form, since a geometry carries no NGSI-LD Null.

use crate::{expanded_value::ValueNode, geometry_value::GeometryValue, queried::value_node_reading::item_of, value_mode::Stored};
use andromeda_ngsi_ld_geometry::{bounding_box::BoundingBox, form::GeometryForm};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query_evaluation::queried_entity::{queried_geometry::QueriedGeometry, value_item::ValueItem};

impl<'a> QueriedGeometry<'a> for &'a GeometryValue {
    type Node = &'a ValueNode<Stored>;

    fn form(self) -> &'a GeometryForm {
        GeometryValue::form(self)
    }

    fn bbox(self) -> Option<&'a BoundingBox> {
        self.members()?.bbox()
    }

    fn foreign_member(self, member: &AttributeName) -> Option<impl Iterator<Item = ValueItem<'a, &'a ValueNode<Stored>>>> {
        let values = self.members()?.foreign().get(member.as_str())?;
        Some(values.iter().map(|value| item_of(value, None)))
    }
}
