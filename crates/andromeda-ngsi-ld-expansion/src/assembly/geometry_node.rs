//! The `GeoJSON` node expansion makes of a geometry (clause 4.7, IETF RFC 7946
//! section 3.1), read into its positions and written back from them.
//!
//! Expansion under the core context turns a `GeoJSON` geometry into a node typed
//! with the geometry's IRI under the `GeoJSON` vocabulary, whose `coordinates`
//! are nested list objects of numeric value objects. A `GeoProperty` value is
//! read from that node into its positions, refusing what RFC 7946 does not
//! admit with the same refusals every other geometry reading raises; a
//! geometry the transport already read stands for that node, and as the value
//! of a plain Property it is written back into it. The node's `bbox` (IETF RFC
//! 7946 section 5, the core context's `geojson:bbox` `@list` term) is read into
//! a bounding box, and every other member is a foreign member (section 6.1),
//! kept as the expanded values it holds.

use crate::{
    assembly::expanded_value::{MemberMode, expanded_values, node_member},
    expanded_tree::{ExpandedGeometry, ExpandedItem, ExpandedNode, Scalar},
};
use andromeda_ngsi_ld_entity::{
    expanded_geometry_type::{expanded_geometry_type, geometry_type_iri},
    expanded_value::{ExpandedValue, ValueNode},
    expanded_value_name::ExpandedValueName,
    geometry_value::{ExpandedGeoJsonMembers, GeometryValue},
    literal::{Literal, LiteralValue},
    member_shape::MemberShape,
    refusal::EntityRefusal,
    value_mode::Stored,
};
use andromeda_ngsi_ld_geometry::{
    bounding_box::{BoundingBox, BoundingBoxError},
    coordinate_tape::CoordinateTape,
    form::{GeometryForm, PolygonRings},
    geojson::GeometryError,
    position::Position,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_number::JsonNumber;
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

/// The `GeoJSON` node expansion makes of `geometry`, as the value of a plain
/// Property: typed with the geometry's IRI, `coordinates` nested lists of
/// numbers, each the decimal of the shortest spelling that reads back as its
/// double, the `bbox` one list of such numbers, and every foreign member as
/// the member of any node.
///
/// # Errors
///
/// Returns [`EntityRefusal::Malformed`] should a vocabulary IRI not read as a
/// name, which the generated vocabulary rules out, and the refusals of a
/// structured value's members for the foreign members.
pub fn geometry_value_node<M: MemberMode>(attribute: &AttributeName, geometry: ExpandedGeometry) -> Result<ExpandedValue<M>, EntityRefusal> {
    let malformed = || EntityRefusal::Malformed {
        attribute: Some(attribute.clone()), // clone: the refusal names the attribute the caller keeps
        member: MemberShape::Geometry,
    };
    let ExpandedGeometry { form, bbox, foreign } = geometry;
    let type_name = geometry_type_iri(form.geometry_type()).parse::<ExpandedValueName>().map_err(|_| malformed())?;
    let coordinates = properties::COORDINATES.as_str().parse::<ExpandedValueName>().map_err(|_| malformed())?;
    let list = coordinate_list::<M>(&form).ok_or_else(malformed)?;
    let mut members = IndexMap::with_capacity(2 + foreign.len());
    members.insert(coordinates, M::values(vec![list]));
    if let Some(bbox) = bbox {
        let name = properties::BBOX.as_str().parse::<ExpandedValueName>().map_err(|_| malformed())?;
        let ordinates = bbox.ordinates().map(ordinate_literal).collect::<Option<Vec<_>>>().ok_or_else(malformed)?;
        members.insert(name, M::values(vec![ExpandedValue::List(ordinates)]));
    }
    for (name, items) in foreign {
        let name = name.parse::<ExpandedValueName>().map_err(EntityRefusal::ExpandedMemberName)?;
        members.insert(name, node_member::<M>(attribute, items)?);
    }
    Ok(ExpandedValue::Node(ValueNode::new(None, vec![type_name], members)))
}

/// The nested coordinate lists of `form`.
fn coordinate_list<M: MemberMode>(form: &GeometryForm) -> Option<ExpandedValue<M>> {
    match form {
        GeometryForm::Point(point) => position_list(point),
        GeometryForm::MultiPoint(points) => positions_list(points),
        GeometryForm::LineString(line) => positions_list(line.positions()),
        GeometryForm::MultiLineString(lines) => lines
            .iter()
            .map(|line| positions_list(line.positions()))
            .collect::<Option<Vec<_>>>()
            .map(ExpandedValue::List),
        GeometryForm::Polygon(rings) => rings_list(rings),
        GeometryForm::MultiPolygon(polygons) => polygons.iter().map(rings_list).collect::<Option<Vec<_>>>().map(ExpandedValue::List),
    }
}

/// The list of the rings of one Polygon.
fn rings_list<M: MemberMode>(rings: &PolygonRings) -> Option<ExpandedValue<M>> {
    rings
        .rings()
        .iter()
        .map(|ring| positions_list(ring.positions()))
        .collect::<Option<Vec<_>>>()
        .map(ExpandedValue::List)
}

/// The list of several positions.
fn positions_list<M: MemberMode>(positions: &[Position]) -> Option<ExpandedValue<M>> {
    positions.iter().map(position_list).collect::<Option<Vec<_>>>().map(ExpandedValue::List)
}

/// The list of one position's ordinates.
fn position_list<M: MemberMode>(position: &Position) -> Option<ExpandedValue<M>> {
    [Some(position.longitude()), Some(position.latitude()), position.altitude()]
        .into_iter()
        .flatten()
        .map(ordinate_literal)
        .collect::<Option<Vec<_>>>()
        .map(ExpandedValue::List)
}

/// One ordinate as the numeric value object it expands to.
fn ordinate_literal<M: MemberMode>(ordinate: f64) -> Option<ExpandedValue<M>> {
    ordinate
        .to_string()
        .parse::<JsonNumber>()
        .ok()
        .map(|number| ExpandedValue::Literal(Literal::plain(LiteralValue::Number(number))))
}
