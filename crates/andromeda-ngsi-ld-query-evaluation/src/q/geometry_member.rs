//! The members of a `GeoProperty`'s geometry a trailing path reads (clause
//! 4.9): the `type` and the `coordinates` of the `GeoJSON` geometry object it
//! is (IETF RFC 7946 section 3.1).
//!
//! The geometry is held as positions, so each member is built from them: the
//! type's token as a String, the coordinates as nested arrays of numbers, a
//! position an array of its longitude, its latitude and, where it has one, its
//! altitude. A String and an array have no named member, so a path of two
//! members or more reaches nothing.

use crate::q::target_value::{ResolvedTarget, TargetValue};
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geometry_member::GeometryMember, position::Position};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::filter::reading::family_reading::{MultiValueReading, TargetOrigin};
use std::borrow::Cow;

/// The member of `form` the path `written` names, as the client wrote it.
pub(crate) fn geometry_member<'a>(form: &'a GeometryForm, written: &[AttributeName]) -> Option<ResolvedTarget<'a>> {
    let [name] = written else {
        return None;
    };
    let value = match name.as_str().parse::<GeometryMember>().ok()? {
        GeometryMember::Type => TargetValue::Text(Cow::Owned(form.geometry_type().to_string())),
        GeometryMember::Coordinates => coordinates(form)?,
        GeometryMember::Bbox | GeometryMember::Geometries => return None,
    };
    Some(ResolvedTarget {
        value,
        origin: TargetOrigin::AttributeValue,
        reading: MultiValueReading::WholeArray,
    })
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

/// The array of a position's coordinates, none when one is no finite number,
/// which no position admitted as CRS84 holds.
fn position(point: &Position) -> Option<TargetValue<'static>> {
    let coordinate = |value: f64| JsonNumber::try_from(value).ok().map(|number| TargetValue::Number(Cow::Owned(number)));
    nested(
        [Some(point.longitude()), Some(point.latitude()), point.altitude()]
            .into_iter()
            .flatten()
            .map(coordinate),
    )
}

#[cfg(test)]
mod tests {
    use crate::q::{
        geometry_member::geometry_member,
        target_value::{ResolvedTarget, TargetValue},
    };
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, position::Position};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_number::JsonNumber;
    use pretty_assertions::assert_eq;
    use std::borrow::Cow;

    fn path(names: &[&str]) -> Vec<AttributeName> {
        names.iter().map(|name| name.parse().expect("a member name")).collect()
    }

    fn point() -> GeometryForm {
        GeometryForm::Point(Position::try_new(13.4, 52.5, None).expect("a CRS84 position"))
    }

    fn number(value: f64) -> TargetValue<'static> {
        TargetValue::Number(Cow::Owned(JsonNumber::try_from(value).expect("a finite number")))
    }

    /// Clause 4.9 evaluates `location[type]` against the geometry object,
    /// whose `type` member is its type's token.
    #[test]
    fn the_type_member_is_the_token_of_the_geometry_type() {
        let form = point();
        assert_eq!(
            geometry_member(&form, &path(&["type"])).map(|target| target.value),
            Some(TargetValue::Text(Cow::Borrowed("Point")))
        );
    }

    /// RFC 7946 section 3.1.2: a Point's `coordinates` is one position.
    #[test]
    fn the_coordinates_of_a_point_are_its_one_position() {
        let form = point();
        assert_eq!(
            geometry_member(&form, &path(&["coordinates"])).map(|target: ResolvedTarget<'_>| target.value),
            Some(TargetValue::Array(vec![number(13.4), number(52.5)]))
        );
    }

    /// A member the geometry does not hold, and a member of a member, reach
    /// nothing: clause 4.9 makes such an element non-existent.
    #[test]
    fn an_unheld_member_or_a_member_of_a_member_reaches_nothing() {
        let form = point();
        for names in [&["bbox"][..], &["geometries"], &["city"], &["type", "length"], &["coordinates", "first"]] {
            assert!(geometry_member(&form, &path(names)).is_none(), "{names:?}");
        }
    }
}
