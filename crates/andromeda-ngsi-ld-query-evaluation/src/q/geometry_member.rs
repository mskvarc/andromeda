//! The members of a `GeoProperty`'s geometry a trailing path reads (clause
//! 4.9), as [`GeometryPath`] tells them apart: the `type`, the `coordinates`
//! and the `bbox` of the `GeoJSON` geometry object it is (IETF RFC 7946
//! sections 3.1 and 5), and its foreign members (section 6.1).
//!
//! The geometry is held as positions, so each described member is built from
//! them: the type's token as a String, the coordinates as nested arrays of
//! numbers, a position an array of its longitude, its latitude and, where it
//! has one, its altitude, and the `bbox` as the array of its numbers. A
//! foreign member is read as the member of a structured value is.

use crate::{
    q::{
        compound_member::resolve_member_values,
        target_value::{ResolvedTarget, TargetValue},
    },
    queried_entity::queried_geometry::QueriedGeometry,
};
use andromeda_ngsi_ld_geometry::{
    bounding_box::BoundingBox,
    coordinate_notation::{COORDINATE_DECIMALS, COORDINATE_SIGNIFICANT_DIGITS, CoordinateNotation},
    form::GeometryForm,
    geometry_member::GeometryMember,
    position::Position,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::filter::{
    reading::{
        family_reading::{MultiValueReading, TargetOrigin},
        geometry_path::GeometryPath,
    },
    trailing_path::MemberPath,
};
use std::borrow::Cow;

/// The member of `geometry` the path `members` reaches.
///
/// A `geometries` member belongs to a `GeometryCollection`, which no
/// `GeoProperty` holds (clause 4.7.1).
pub(crate) fn geometry_member<'a, G: QueriedGeometry<'a>>(geometry: G, members: &MemberPath) -> Option<ResolvedTarget<'a>> {
    let value = match GeometryPath::of(members) {
        GeometryPath::Described(GeometryMember::Type) => TargetValue::Text(Cow::Owned(geometry.form().geometry_type().to_string())),
        GeometryPath::Described(GeometryMember::Coordinates) => coordinates(geometry.form())?,
        GeometryPath::Described(GeometryMember::Bbox) => bounding_box(geometry.bbox()?)?,
        GeometryPath::Described(GeometryMember::Geometries) | GeometryPath::Nothing => return None,
        GeometryPath::Foreign(path) => return foreign_member(geometry, path),
    };
    Some(ResolvedTarget {
        value,
        origin: TargetOrigin::AttributeValue,
        reading: MultiValueReading::WholeArray,
    })
}

/// The member a foreign path reaches: the member of `geometry` its first name
/// keys, and within it the member the rest of the path reaches.
fn foreign_member<'a, G: QueriedGeometry<'a>>(geometry: G, path: &[AttributeName]) -> Option<ResolvedTarget<'a>> {
    let (first, rest) = path.split_first()?;
    resolve_member_values(geometry.foreign_member(first)?, rest)
}

/// The numbers of `bbox` in RFC 7946 section 5 order, each the number the
/// store keeps it as: its shortest decimal spelling.
fn bounding_box(bbox: &BoundingBox) -> Option<TargetValue<'static>> {
    nested(
        bbox.ordinates()
            .map(|ordinate| JsonNumber::try_from(ordinate).ok().map(|number| TargetValue::Number(Cow::Owned(number)))),
    )
}

/// The `coordinates` of `form`, nested as RFC 7946 section 3.1 nests them.
fn coordinates(form: &GeometryForm) -> Option<TargetValue<'static>> {
    match form {
        GeometryForm::Point(point) => position(point),
        GeometryForm::MultiPoint(points) => positions(points),
        GeometryForm::LineString(line) => positions(line.positions()),
        GeometryForm::MultiLineString(lines) => nested(lines.iter().map(|line| positions(line.positions()))),
        GeometryForm::Polygon(polygon) => nested(polygon.rings().iter().map(|ring| positions(ring.positions()))),
        GeometryForm::MultiPolygon(polygons) => nested(
            polygons
                .iter()
                .map(|polygon| nested(polygon.rings().iter().map(|ring| positions(ring.positions())))),
        ),
    }
}

/// An array of the coordinates of each of `points`.
fn positions(points: &[Position]) -> Option<TargetValue<'static>> {
    nested(points.iter().map(position))
}

/// The array of `items`, none when one of them is none.
fn nested(items: impl Iterator<Item = Option<TargetValue<'static>>>) -> Option<TargetValue<'static>> {
    items.collect::<Option<Vec<_>>>().map(TargetValue::Array)
}

/// The array of a position's coordinates; none when one is no finite number,
/// which no position admitted as CRS84 holds.
fn position(point: &Position) -> Option<TargetValue<'static>> {
    nested(
        [Some(point.longitude()), Some(point.latitude()), point.altitude()]
            .into_iter()
            .flatten()
            .map(|value| coordinate(value).map(|number| TargetValue::Number(Cow::Owned(number)))),
    )
}

/// The number a coordinate reads as: the one the store writes it back as
/// ([`CoordinateNotation`]), its shortest spelling rounded half to even.
fn coordinate(value: f64) -> Option<JsonNumber> {
    let shortest = JsonNumber::try_from(value).ok()?;
    match CoordinateNotation::of(value) {
        CoordinateNotation::FixedPoint => shortest.rounded_to_decimals(COORDINATE_DECIMALS),
        CoordinateNotation::Exponent => shortest.rounded_to_significant_digits(COORDINATE_SIGNIFICANT_DIGITS),
    }
    .ok()
}

#[cfg(test)]
mod tests {
    use crate::{
        q::{geometry_member::geometry_member, target_value::TargetValue},
        queried_entity::{attribute_reading::QueriedInstance, expanded_fixture::FixtureInstance},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_ngsi_ld_query::filter::trailing_path::MemberPath;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::borrow::Cow;

    /// The prefix of the `GeoJSON` vocabulary the core `@context` maps
    /// `coordinates` and `bbox` to.
    const GEOJSON: &str = "https://purl.org/geojson/vocab#";

    fn names(names: &[&str]) -> Vec<AttributeName> {
        names.iter().map(|name| name.parse().expect("a member name")).collect()
    }

    /// An expanded `GeoProperty` instance holding the Point at `longitude`,
    /// `latitude`, whose geometry node carries `extra` beside its positions.
    fn located(longitude: f64, latitude: f64, extra: &Value) -> FixtureInstance {
        let mut node = json!({
            "@type": [format!("{GEOJSON}Point")],
            format!("{GEOJSON}coordinates"): [{"@list": [{"@value": longitude}, {"@value": latitude}]}]
        });
        if let (Some(members), Some(extra)) = (node.as_object_mut(), extra.as_object()) {
            members.extend(extra.iter().map(|(name, value)| (name.clone(), value.clone())));
        }
        FixtureInstance::from_expanded(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [node]
        }))
    }

    /// The Kiosk's `location`: a Point with a `bbox`, the foreign member
    /// `title` and the foreign member `meta` holding `source`.
    fn kiosk() -> FixtureInstance {
        located(
            13.4,
            52.5,
            &json!({
                format!("{GEOJSON}bbox"): [{"@list": [{"@value": 13.3}, {"@value": 52.4}, {"@value": 13.5}, {"@value": 52.6}]}],
                "https://example.com/title": [{"@value": "Gate"}],
                "https://example.com/meta": [{"https://example.com/source": [{"@value": "survey"}]}]
            }),
        )
    }

    /// The target the path the client wrote as `written`, and expansion
    /// addressed as `addressed`, reaches in the geometry `instance` holds.
    fn member<'a>(instance: &'a FixtureInstance, written: &[&str], addressed: &[&str]) -> Option<TargetValue<'a>> {
        let path = MemberPath {
            addressed: names(addressed),
            written: names(written),
        };
        geometry_member(instance.geometry()?, &path).map(|target| target.value)
    }

    /// A path written as `names`, which expansion leaves as written.
    fn written<'a>(instance: &'a FixtureInstance, names: &[&str]) -> Option<TargetValue<'a>> {
        member(instance, names, names)
    }

    fn spelled(text: &str) -> TargetValue<'static> {
        TargetValue::Number(Cow::Owned(text.parse::<JsonNumber>().expect("a number")))
    }

    fn text(value: &str) -> TargetValue<'_> {
        TargetValue::Text(Cow::Borrowed(value))
    }

    /// Clause 4.9 evaluates `location[type]` against the geometry object,
    /// whose `type` member is its type's token.
    #[test]
    fn the_type_member_is_the_token_of_the_geometry_type() {
        let instance = located(13.4, 52.5, &json!({}));
        assert_eq!(written(&instance, &["type"]), Some(text("Point")));
    }

    /// RFC 7946 section 3.1.2: a Point's `coordinates` is one position.
    #[test]
    fn the_coordinates_of_a_point_are_its_one_position() {
        let instance = located(13.4, 52.5, &json!({}));
        assert_eq!(
            member(&instance, &["coordinates"], &[&format!("{GEOJSON}coordinates")]),
            Some(TargetValue::Array(vec![spelled("13.4"), spelled("52.5")]))
        );
    }

    /// The store writes a coordinate back as its shortest spelling rounded
    /// half to even to fifteen fractional digits, so `52.6`, whose double is
    /// `52.60000000000000142...`, reads as `52.6`, and `0.12345678901234568`
    /// as `0.123456789012346`.
    #[test]
    fn a_coordinate_reads_as_its_shortest_spelling_at_fifteen_decimals() {
        for (longitude, latitude, expected) in [
            (-74.006, 40.7128, ["-74.006", "40.7128"]),
            (8.8, 52.6, ["8.8", "52.6"]),
            (8.8, 53.075_171, ["8.8", "53.075171"]),
            (0.123_456_789_012_345_68, 52.5, ["0.123456789012346", "52.5"]),
        ] {
            let instance = located(longitude, latitude, &json!({}));
            assert_eq!(
                written(&instance, &["coordinates"]),
                Some(TargetValue::Array(expected.map(spelled).to_vec())),
                "{longitude} {latitude}"
            );
        }
    }

    /// A coordinate of magnitude `1e-8` or less is written in exponent
    /// notation, its mantissa at fifteen fractional digits, so it keeps
    /// sixteen significant digits.
    #[test]
    fn a_tiny_coordinate_reads_at_sixteen_significant_digits() {
        let instance = located(1.234_567_890_123_456_7e-9, 0.0, &json!({}));
        assert_eq!(
            written(&instance, &["coordinates"]),
            Some(TargetValue::Array(vec![spelled("1.234567890123457e-9"), spelled("0")]))
        );
    }

    /// RFC 7946 section 5: the `bbox` is the array of the south-west corner's
    /// axes followed by the north-east corner's, as written.
    #[test]
    fn the_bbox_member_is_the_array_of_its_numbers() {
        assert_eq!(
            member(&kiosk(), &["bbox"], &[&format!("{GEOJSON}bbox")]),
            Some(TargetValue::Array(["13.3", "52.4", "13.5", "52.6"].map(spelled).to_vec()))
        );
        assert_eq!(written(&located(13.4, 52.5, &json!({})), &["bbox"]), None);
    }

    /// RFC 7946 section 6.1: a foreign member is read by its expanded name,
    /// as a member of a structured value is, and so are the members within it;
    /// one holding a node is an Object, which no operand equals.
    #[test]
    fn a_foreign_member_and_the_members_within_it_are_read_by_expanded_name() {
        let instance = kiosk();
        assert_eq!(member(&instance, &["title"], &["https://example.com/title"]), Some(text("Gate")));
        assert_eq!(
            member(&instance, &["meta", "source"], &["https://example.com/meta", "https://example.com/source"]),
            Some(text("survey"))
        );
        assert_eq!(member(&instance, &["meta"], &["https://example.com/meta"]), Some(TargetValue::Structure));
    }

    /// A member the geometry does not hold, a member of a described member,
    /// and a described member named by its expanded name reach nothing:
    /// clause 4.9 makes such an element non-existent.
    #[test]
    fn an_unheld_member_or_a_member_of_a_described_member_reaches_nothing() {
        let instance = kiosk();
        for names in [
            &["geometries"][..],
            &["city"],
            &["type", "length"],
            &["coordinates", "first"],
            &["bbox", "first"],
        ] {
            assert!(written(&instance, names).is_none(), "{names:?}");
        }
        assert!(member(&instance, &["missing"], &["https://example.com/missing"]).is_none());
        assert!(member(&instance, &["meta", "missing"], &["https://example.com/meta", "https://example.com/missing"]).is_none());
        assert!(member(&instance, &["box"], &[&format!("{GEOJSON}bbox")]).is_none());
    }
}
