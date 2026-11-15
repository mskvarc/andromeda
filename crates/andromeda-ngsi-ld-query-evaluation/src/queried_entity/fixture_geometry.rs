//! A `GeoProperty` geometry read off an expanded fixture: its positions and
//! the `bbox` and the foreign members its `GeoJSON` node carries beside them
//! (IETF RFC 7946 sections 5 and 6.1), as the evaluator reads a geometry.

use crate::queried_entity::{
    expanded_fixture::{FixtureItem, FixtureNode, attribute_name, item_of},
    queried_geometry::QueriedGeometry,
    value_item::ValueItem,
};
use andromeda_ngsi_ld_geometry::{bounding_box::BoundingBox, form::GeometryForm, position::Position};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde_json::Value;

/// A `GeoProperty` geometry decoded from an expanded fixture: its positions
/// and the members its node carries beside them.
#[derive(Debug)]
pub(crate) struct FixtureGeometry {
    /// The positions.
    form: GeometryForm,
    /// The `bbox`, when the node carries one.
    bbox: Option<BoundingBox>,
    /// The foreign members with their values, by expanded name.
    foreign: Vec<(AttributeName, Vec<FixtureItem>)>,
}

/// Decode one expanded `GeoJSON` Point node into its geometry, with the
/// `bbox` and the foreign members beside its positions, any other geometry
/// node into a value no path addresses, and the clause 4.7.2 string form as
/// the literal it is.
pub(crate) fn geometry_of(node: &Value) -> FixtureItem {
    if node.get("@value").is_some() {
        return item_of(node);
    }
    let point = node
        .get("@type")
        .and_then(Value::as_array)
        .is_some_and(|types| types.iter().any(|declared| declared == GEOJSON_POINT));
    match (point, list_numbers(node, GEOJSON_COORDINATES).as_deref()) {
        (true, Some(&[longitude, latitude])) => FixtureItem::Geometry(FixtureGeometry {
            form: GeometryForm::Point(Position::try_new(longitude, latitude, None).expect("a CRS84 position")),
            bbox: list_numbers(node, GEOJSON_BBOX).map(|ordinates| BoundingBox::try_from_ordinates(&ordinates).expect("an RFC 7946 bbox")),
            foreign: node
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(key, _)| !key.starts_with('@') && ![GEOJSON_COORDINATES, GEOJSON_BBOX].contains(&key.as_str()))
                .map(|(key, values)| {
                    (
                        attribute_name(key),
                        values.as_array().map_or_else(Vec::new, |values| values.iter().map(item_of).collect()),
                    )
                })
                .collect(),
        }),
        (true | false, Some(_) | None) => FixtureItem::Opaque,
    }
}

/// The numbers of the one list the member `name` of `node` holds.
fn list_numbers(node: &Value, name: &str) -> Option<Vec<f64>> {
    node.get(name)
        .and_then(|member| member.get(0))
        .and_then(|list| list.get("@list"))
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|item| item.get("@value").and_then(Value::as_f64)).collect())
}

/// The expanded type of a `GeoJSON` Point.
const GEOJSON_POINT: &str = "https://purl.org/geojson/vocab#Point";

/// The expanded name of a `GeoJSON` geometry's `coordinates`.
const GEOJSON_COORDINATES: &str = "https://purl.org/geojson/vocab#coordinates";

/// The expanded name of a `GeoJSON` geometry's `bbox`.
const GEOJSON_BBOX: &str = "https://purl.org/geojson/vocab#bbox";

impl<'a> QueriedGeometry<'a> for &'a FixtureGeometry {
    type Node = &'a FixtureNode;

    fn form(self) -> &'a GeometryForm {
        &self.form
    }

    fn bbox(self) -> Option<&'a BoundingBox> {
        self.bbox.as_ref()
    }

    fn foreign_member(self, member: &AttributeName) -> Option<impl Iterator<Item = ValueItem<'a, &'a FixtureNode>>> {
        self.foreign
            .iter()
            .find(|(name, _)| name == member)
            .map(|(_, values)| values.iter().map(FixtureItem::view))
    }
}
