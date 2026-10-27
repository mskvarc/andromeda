//! The `GeoJSON` node expansion makes of a geometry (clause 4.7, IETF RFC 7946
//! section 3.1), read into its positions.
//!
//! Expansion under the core context turns a `GeoJSON` geometry into a node typed
//! with the geometry's IRI under the `GeoJSON` vocabulary, whose `coordinates`
//! are nested list objects of numeric value objects. A `GeoProperty` value is
//! read from that node into its positions, refusing what RFC 7946 does not
//! admit with the same refusals every other geometry reading raises; a
//! geometry the transport already read stands for that node. The node's `bbox` (IETF RFC
//! 7946 section 5, the core context's `geojson:bbox` `@list` term) is read into
//! a bounding box, and every other member is a foreign member (section 6.1),
//! kept as the expanded values it holds.

use crate::{
    assembly::expanded_value::expanded_values,
    expanded_tree::{ExpandedGeometry, ExpandedItem, ExpandedNode, Scalar},
};
use andromeda_ngsi_ld_entity::{
    expanded_geometry_type::expanded_geometry_type,
    expanded_value_name::ExpandedValueName,
    geometry_value::{ExpandedGeoJsonMembers, GeometryValue},
    refusal::EntityRefusal,
    value_mode::Stored,
};
use andromeda_ngsi_ld_geometry::{
    bounding_box::{BoundingBox, BoundingBoxError},
    coordinate_tape::CoordinateTape,
    form::GeometryForm,
    geojson::GeometryError,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use indexmap::IndexMap;

/// Read an expanded `GeoJSON` node as the geometry it describes, with its
/// `bbox` and its foreign members.
///
/// # Errors
///
/// Returns [`GeometryError::GeometryCollectionUnsupported`] for a collection,
/// [`GeometryError::MissingType`] and [`GeometryError::UnknownType`] for a node
/// naming no geometry type, [`GeometryError::MissingCoordinates`], the
/// nesting, line, ring and position refusals of the coordinates, and
/// [`GeometryError::InvalidBbox`] for a `bbox` RFC 7946 section 5 refuses.
pub fn geometry_of(node: ExpandedNode) -> Result<ExpandedGeometry, GeometryError> {
    let form = geometry_form(&node)?;
    let mut bbox = None;
    let mut foreign = Vec::new();
    for (name, items) in node.members {
        if name == properties::BBOX.as_str() {
            bbox = Some(bounding_box(&items)?);
        } else if name != properties::COORDINATES.as_str() {
            foreign.push((name, items));
        }
    }
    Ok(ExpandedGeometry { form, bbox, foreign })
}

/// The bounding box an expanded `bbox` member holds: one list of numbers.
fn bounding_box(items: &[ExpandedItem]) -> Result<BoundingBox, GeometryError> {
    let [ExpandedItem::List(ordinates)] = items else {
        return Err(GeometryError::InvalidBbox(BoundingBoxError::NotNumbers));
    };
    let ordinates = ordinates
        .iter()
        .map(ordinate_value)
        .collect::<Option<Vec<f64>>>()
        .ok_or(GeometryError::InvalidBbox(BoundingBoxError::NotNumbers))?;
    Ok(BoundingBox::try_from_ordinates(&ordinates)?)
}

/// The geometry of a `GeoProperty` as the model holds it, its foreign members
/// read as state values.
///
/// # Errors
///
/// Returns [`EntityRefusal::ExpandedMemberName`] for a foreign member name
/// that expansion never keeps, and the refusals of an expanded value for its values, the
/// NGSI-LD Null among them.
pub fn geometry_value(attribute: &AttributeName, geometry: ExpandedGeometry) -> Result<GeometryValue, EntityRefusal> {
    let ExpandedGeometry { form, bbox, foreign } = geometry;
    let foreign = foreign
        .into_iter()
        .map(|(name, items)| {
            let name = name.parse::<ExpandedValueName>().map_err(EntityRefusal::ExpandedMemberName)?;
            Ok((name, expanded_values::<Stored>(attribute, items)?))
        })
        .collect::<Result<IndexMap<_, _>, EntityRefusal>>()?;
    Ok(GeometryValue::with_members(form, ExpandedGeoJsonMembers::non_empty(bbox, foreign)))
}

/// The positions an expanded `GeoJSON` node describes.
fn geometry_form(node: &ExpandedNode) -> Result<GeometryForm, GeometryError> {
    let geometry_type = expanded_geometry_type(&node.types)?;
    let coordinates = node.member(properties::COORDINATES.as_str()).ok_or(GeometryError::MissingCoordinates)?;
    coordinate_tape(coordinates).read_as(geometry_type)
}

/// The items of an expanded `coordinates` member recorded onto a coordinate
/// tape, which reads them as the geometry type nests them: the member holds
/// its one `@list`, each list is an array and each numeric value object an
/// ordinate. A member holding anything but one item is one member that is no
/// array, refused at the root.
fn coordinate_tape(items: &[ExpandedItem]) -> CoordinateTape {
    let mut tape = CoordinateTape::default();
    match items {
        [item] => record(&mut tape, item),
        [] | [_, _, ..] => tape.other(),
    }
    tape
}

/// Record one expanded coordinate item, and every item below it.
fn record(tape: &mut CoordinateTape, item: &ExpandedItem) {
    match item {
        ExpandedItem::List(items) => {
            tape.open();
            for item in items {
                record(tape, item);
            }
            tape.close();
        }
        ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::Node(_) | ExpandedItem::Geometry(_) => tape.number(ordinate_value(item)),
    }
}

/// The double one ordinate reads as, the nearest to its exact text; `None`
/// for an item that is no number.
fn ordinate_value(item: &ExpandedItem) -> Option<f64> {
    match item {
        ExpandedItem::Value(value) => match &value.scalar {
            Scalar::Number(text) => text.parse::<f64>().ok(),
            Scalar::String(_) | Scalar::Boolean(_) => None,
        },
        ExpandedItem::Json(_) | ExpandedItem::Node(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_) => None,
    }
}
