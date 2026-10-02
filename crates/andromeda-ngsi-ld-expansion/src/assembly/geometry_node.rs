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
    expanded_value::{ExpandedValue, ValueNode},
    geometry_value::{ExpandedGeoJsonMembers, GeometryValue},
    literal::{Literal, LiteralValue},
    member_shape::MemberShape,
    refusal::EntityRefusal,
    value_mode::Stored,
};
use andromeda_ngsi_ld_geometry::{
    bounding_box::{BoundingBox, BoundingBoxError},
    coordinate_path::CoordinatePath,
    form::{GeometryForm, PolygonRings},
    geojson::GeometryError,
    geometry_type::GeometryType,
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::{Position, PositionError},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_vocabulary::{expanded::properties, prefix};
use indexmap::IndexMap;
use std::str::FromStr;

/// The `GeoJSON` type token of a `GeometryCollection`, which clause 4.7.1 does
/// not admit as a `GeoProperty` value and [`GeometryType`] does not name.
const GEOMETRY_COLLECTION_TOKEN: &str = "GeometryCollection";

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
/// Returns [`EntityRefusal::ExpandedAttributeName`] for a foreign member name
/// that is no IRI, and the refusals of an expanded value for its values, the
/// NGSI-LD Null among them.
pub fn geometry_value(attribute: &AttributeName, geometry: ExpandedGeometry) -> Result<GeometryValue, EntityRefusal> {
    let ExpandedGeometry { form, bbox, foreign } = geometry;
    let foreign = foreign
        .into_iter()
        .map(|(name, items)| {
            let name = name.parse::<AttributeName>().map_err(EntityRefusal::ExpandedAttributeName)?;
            Ok((name, expanded_values::<Stored>(attribute, items)?))
        })
        .collect::<Result<IndexMap<_, _>, EntityRefusal>>()?;
    Ok(GeometryValue::with_members(form, ExpandedGeoJsonMembers::non_empty(bbox, foreign)))
}

/// The positions an expanded `GeoJSON` node describes.
fn geometry_form(node: &ExpandedNode) -> Result<GeometryForm, GeometryError> {
    let token = node
        .types
        .first()
        .and_then(|iri| iri.strip_prefix(prefix::GEOJSON.as_str()))
        .ok_or(GeometryError::MissingType)?;
    if token == GEOMETRY_COLLECTION_TOKEN {
        return Err(GeometryError::GeometryCollectionUnsupported);
    }
    let geometry_type = GeometryType::from_str(token).map_err(GeometryError::UnknownType)?;
    let coordinates = node.member(properties::COORDINATES.as_str()).ok_or(GeometryError::MissingCoordinates)?;
    // A `coordinates` member the `@list` container did not wrap names no
    // nesting the reader below can follow.
    let [coordinates @ ExpandedItem::List(_)] = coordinates else {
        return Err(GeometryError::MissingCoordinates);
    };
    let reader = CoordinateReader { geometry_type };
    let enclosing = &mut Vec::new();
    Ok(match geometry_type {
        GeometryType::Point => GeometryForm::Point(reader.position(coordinates, enclosing)?),
        GeometryType::MultiPoint => GeometryForm::MultiPoint(reader.each(coordinates, enclosing, |reader, item, enclosing: &mut Vec<usize>| {
            reader.position(item, enclosing)
        })?),
        GeometryType::LineString => GeometryForm::LineString(reader.line_string(coordinates, enclosing)?),
        GeometryType::MultiLineString => GeometryForm::MultiLineString(reader.each(coordinates, enclosing, CoordinateReader::line_string)?),
        GeometryType::Polygon => GeometryForm::Polygon(reader.rings(coordinates, enclosing)?),
        GeometryType::MultiPolygon => GeometryForm::MultiPolygon(reader.each(coordinates, enclosing, CoordinateReader::rings)?),
    })
}

/// Reads the expanded `coordinates` of one geometry the way its type nests
/// them (IETF RFC 7946 sections 3.1.2 through 3.1.7).
#[derive(Clone, Copy, Debug)]
struct CoordinateReader {
    /// The geometry type whose nesting is read, named in a nesting refusal.
    geometry_type: GeometryType,
}

impl CoordinateReader {
    /// The refusal of an item found where the type nests something else.
    fn misnested(self, enclosing: &[usize]) -> GeometryError {
        GeometryError::Misnested {
            geometry_type: self.geometry_type,
            at: CoordinatePath::new(enclosing.to_vec()),
        }
    }

    /// The items of the list `item` must be.
    fn list<'a>(self, item: &'a ExpandedItem, enclosing: &[usize]) -> Result<&'a [ExpandedItem], GeometryError> {
        match item {
            ExpandedItem::List(items) => Ok(items),
            ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::Node(_) | ExpandedItem::Geometry(_) => Err(self.misnested(enclosing)),
        }
    }

    /// Read `item` as one position.
    fn position(self, item: &ExpandedItem, enclosing: &[usize]) -> Result<Position, GeometryError> {
        let ordinates = self.list(item, enclosing)?;
        if ordinates.iter().any(|ordinate| matches!(ordinate, ExpandedItem::List(_))) {
            return Err(self.misnested(enclosing));
        }
        position(ordinates).map_err(|source| GeometryError::Position {
            at: CoordinatePath::new(enclosing.to_vec()),
            source,
        })
    }

    /// Read `item` as the positions of one `LineString`.
    fn line_string(self, item: &ExpandedItem, enclosing: &mut Vec<usize>) -> Result<LineStringCoordinates, GeometryError> {
        let positions = self.each(item, enclosing, |reader, item, enclosing: &mut Vec<usize>| reader.position(item, enclosing))?;
        LineStringCoordinates::try_new(positions).map_err(|source| GeometryError::LineString {
            at: CoordinatePath::new(enclosing.clone()), // clone: the refusal owns the path of the one line it names
            source,
        })
    }

    /// Read `item` as one Polygon ring.
    fn ring(self, item: &ExpandedItem, enclosing: &mut Vec<usize>) -> Result<LinearRing, GeometryError> {
        let positions = self.each(item, enclosing, |reader, item, enclosing: &mut Vec<usize>| reader.position(item, enclosing))?;
        LinearRing::try_new(positions).map_err(|source| GeometryError::LinearRing {
            at: CoordinatePath::new(enclosing.clone()), // clone: the refusal owns the path of the one ring it names
            source,
        })
    }

    /// Read `item` as the rings of one Polygon, exterior ring first.
    fn rings(self, item: &ExpandedItem, enclosing: &mut Vec<usize>) -> Result<PolygonRings, GeometryError> {
        self.each(item, enclosing, CoordinateReader::ring).map(PolygonRings::new)
    }

    /// Read every item of the list `item` with `read`, each one level deeper.
    fn each<T>(
        self,
        item: &ExpandedItem,
        enclosing: &mut Vec<usize>,
        read: impl Fn(CoordinateReader, &ExpandedItem, &mut Vec<usize>) -> Result<T, GeometryError>,
    ) -> Result<Vec<T>, GeometryError> {
        let items = self.list(item, enclosing)?;
        let mut read_items = Vec::with_capacity(items.len());
        for (index, nested) in items.iter().enumerate() {
            enclosing.push(index);
            let read_item = read(self, nested, enclosing);
            enclosing.pop();
            read_items.push(read_item?);
        }
        Ok(read_items)
    }
}

/// Read one position: every member a number, two or three of them, longitude
/// and latitude on the CRS84 axes.
fn position(ordinates: &[ExpandedItem]) -> Result<Position, PositionError> {
    let numbers = ordinates
        .iter()
        .enumerate()
        .map(|(index, ordinate)| ordinate_value(ordinate).ok_or(PositionError::NonNumericOrdinate { index }))
        .collect::<Result<Vec<f64>, PositionError>>()?;
    match numbers.as_slice() {
        [longitude, latitude] => Position::try_new(*longitude, *latitude, None),
        [longitude, latitude, altitude] => Position::try_new(*longitude, *latitude, Some(*altitude)),
        [] | [_] => Err(PositionError::TooFewOrdinates { ordinates: numbers.len() }),
        [_, _, _, _, ..] => Err(PositionError::TooManyOrdinates { ordinates: numbers.len() }),
    }
}

/// The double one ordinate reads as, the nearest to its exact text.
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
    let type_iri = format!("{}{}", prefix::GEOJSON.as_str(), form.geometry_type());
    let type_name = type_iri.parse::<NgsiLdReference>().map_err(|_| malformed())?;
    let coordinates = properties::COORDINATES.as_str().parse::<AttributeName>().map_err(|_| malformed())?;
    let list = coordinate_list::<M>(&form).ok_or_else(malformed)?;
    let mut members = IndexMap::with_capacity(2 + foreign.len());
    members.insert(coordinates, M::values(vec![list]));
    if let Some(bbox) = bbox {
        let name = properties::BBOX.as_str().parse::<AttributeName>().map_err(|_| malformed())?;
        let ordinates = bbox.ordinates().map(ordinate_literal).collect::<Option<Vec<_>>>().ok_or_else(malformed)?;
        members.insert(name, M::values(vec![ExpandedValue::List(ordinates)]));
    }
    for (name, items) in foreign {
        let name = name.parse::<AttributeName>().map_err(EntityRefusal::ExpandedAttributeName)?;
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
