//! In-memory evaluation of a clause 4.10 spatial relation.
//!
//! One evaluator, used by every in-memory matcher in the broker. Before this
//! module existed the same seven match arms were written three times — in the
//! entity spatial index, in the subscription spatial index and in subscription
//! matching — and one of the copies carried a comment asserting it "matches the
//! semantics of" another. That assertion is now the compiler's.
//!
//! Every predicate is evaluated over the horizontal projection of both
//! geometries. ETSI GS CIM 009 v1.9.1 clause 4.10 defines `georel` by reference
//! to OGC 06-103r4, whose clause 6.1.2.5 states that spatial operations work in
//! the "map geometry" of the data and reflect neither z nor m, and whose clause
//! 6.1.15.1 defines the relational operators by projection onto the
//! two-dimensional horizontal spatial reference plane. `near` inherits the same
//! reading through `Buffer`, defined in clause 6.1.2.4. Altitude therefore never
//! enters a verdict, however many ordinates the geometries carry.
//!
//! The SQL renderings are a separate reading of the same clause, because
//! `PostGIS` evaluates the predicates rather than this crate. The pairs they
//! render to are named in the doc comments below so the two stay comparable.

use crate::{distance::haversine_distance_m, geo_rel::GeoRel, geometry::Geometry};
use geo::algorithm::{Contains, Intersects};

/// Whether `subject` stands in the relation `georel` to `reference`.
///
/// `subject` is the geometry read off the entity or registration; `reference`
/// is the one the query supplied. The asymmetry matters: `within` asks whether
/// the subject is inside the reference, `contains` the opposite.
#[must_use]
pub fn matches(georel: &GeoRel, subject: &Geometry, reference: &Geometry) -> bool {
    matches_planar(georel, subject.planar(), reference.planar())
}

/// [`matches()`] over the horizontal projections the geometries carry.
fn matches_planar(georel: &GeoRel, subject: &geo::Geometry<f64>, reference: &geo::Geometry<f64>) -> bool {
    match georel {
        // `ST_Within(subject, reference)`
        GeoRel::Within => reference.contains(subject),
        // `ST_Contains(subject, reference)`
        GeoRel::Contains => subject.contains(reference),
        // `ST_Intersects(subject, reference)`
        GeoRel::Intersects => subject.intersects(reference),
        // `ST_Disjoint(subject, reference)`
        GeoRel::Disjoint => !subject.intersects(reference),
        // `ST_Equals(subject, reference)`; mutual containment is `geo`'s
        // equivalent, as it has no topological equality predicate.
        GeoRel::Equals => subject.contains(reference) && reference.contains(subject),
        // `ST_Overlaps(subject, reference)`: intersecting, of the same
        // dimension, and neither one containing the other. This used to be
        // plain `intersects`, under which an entity lying wholly inside the
        // query polygon matched here and not in the PostGIS rendering of the
        // same clause.
        GeoRel::Overlaps => {
            topological_dimension(subject) == topological_dimension(reference)
                && subject.intersects(reference)
                && !subject.contains(reference)
                && !reference.contains(subject)
        }
        // `ST_DWithin` / `ST_Distance` over `geography`; here a Haversine
        // distance between centroids, which is what the `geography` type
        // approximates for non-point geometries.
        GeoRel::Near { max_distance, min_distance } => match haversine_distance_m(subject, reference) {
            Some(distance) => max_distance.is_none_or(|max| distance <= max.metres()) && min_distance.is_none_or(|min| distance >= min.metres()),
            // A geometry with no centroid is nowhere, so it is near nothing.
            None => false,
        },
    }
}

/// The topological dimension of a geometry, or `None` for a collection, whose
/// members may differ.
///
/// This is the point/line/area distinction of OGC 06-103r4, unrelated to
/// whether the geometry's positions carry altitude. `ST_Overlaps` is defined
/// only between geometries of equal dimension: a point and a polygon intersect
/// but never overlap.
const fn topological_dimension(geometry: &geo::Geometry<f64>) -> Option<u8> {
    match geometry {
        geo::Geometry::Point(_) | geo::Geometry::MultiPoint(_) => Some(0),
        geo::Geometry::Line(_) | geo::Geometry::LineString(_) | geo::Geometry::MultiLineString(_) => Some(1),
        geo::Geometry::Polygon(_) | geo::Geometry::MultiPolygon(_) | geo::Geometry::Rect(_) | geo::Geometry::Triangle(_) => Some(2),
        geo::Geometry::GeometryCollection(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        form::{GeometryForm, PolygonRings},
        position::Position,
        positive_distance::PositiveDistance,
    };
    use std::str::FromStr;

    fn point(longitude: f64, latitude: f64) -> Geometry {
        Geometry::new(GeometryForm::Point(Position::new(longitude, latitude, None)))
    }

    fn square(centre_longitude: f64, centre_latitude: f64, half: f64) -> Geometry {
        Geometry::new(GeometryForm::Polygon(PolygonRings::new(vec![vec![
            Position::new(centre_longitude - half, centre_latitude - half, None),
            Position::new(centre_longitude + half, centre_latitude - half, None),
            Position::new(centre_longitude + half, centre_latitude + half, None),
            Position::new(centre_longitude - half, centre_latitude + half, None),
            Position::new(centre_longitude - half, centre_latitude - half, None),
        ]])))
    }

    /// Lift every position of a geometry to the given altitude, leaving its
    /// horizontal footprint untouched.
    fn lift(geometry: &Geometry, altitude: f64) -> Geometry {
        Geometry::new(lift_form(geometry.form(), altitude))
    }

    /// Lift every position of a form.
    fn lift_form(form: &GeometryForm, altitude: f64) -> GeometryForm {
        match form {
            GeometryForm::Point(position) => GeometryForm::Point(lift_position(position, altitude)),
            GeometryForm::MultiPoint(positions) => GeometryForm::MultiPoint(lift_positions(positions, altitude)),
            GeometryForm::LineString(positions) => GeometryForm::LineString(lift_positions(positions, altitude)),
            GeometryForm::MultiLineString(line_strings) => {
                GeometryForm::MultiLineString(line_strings.iter().map(|line_string| lift_positions(line_string, altitude)).collect())
            }
            GeometryForm::Polygon(rings) => GeometryForm::Polygon(lift_rings(rings, altitude)),
            GeometryForm::MultiPolygon(polygons) => GeometryForm::MultiPolygon(polygons.iter().map(|polygon| lift_rings(polygon, altitude)).collect()),
        }
    }

    /// Lift one position to the given altitude.
    fn lift_position(position: &Position, altitude: f64) -> Position {
        Position::new(position.longitude(), position.latitude(), Some(altitude))
    }

    /// Lift a run of positions to the given altitude.
    fn lift_positions(positions: &[Position], altitude: f64) -> Vec<Position> {
        positions.iter().map(|position| lift_position(position, altitude)).collect()
    }

    /// Lift the rings of a polygon to the given altitude.
    fn lift_rings(rings: &PolygonRings, altitude: f64) -> PolygonRings {
        PolygonRings::new(rings.rings().iter().map(|ring| lift_positions(ring, altitude)).collect())
    }

    /// One row of the conformance table: a relation, the two geometries it is
    /// evaluated over, and the verdict clause 4.10 requires.
    struct Case<'a> {
        /// What the row asserts, used as the assertion message.
        description: &'a str,
        /// The relation under test.
        georel: GeoRel,
        /// The geometry read off the entity.
        subject: &'a Geometry,
        /// The geometry the query supplied.
        reference: &'a Geometry,
        /// The verdict the evaluator must reach.
        expected: bool,
    }

    /// The conformance table over every clause 4.10 relation. The fixtures are
    /// the ones the three deleted copies each tested separately.
    fn conformance_table(fixtures: &Fixtures) -> Vec<Case<'_>> {
        containment_cases(fixtures)
            .into_iter()
            .chain(intersection_cases(fixtures))
            .chain(equality_cases(fixtures))
            .chain(overlap_cases(fixtures))
            .chain(proximity_cases(fixtures))
            .collect()
    }

    /// The rows asserting `within` and `contains`, the two directions of the
    /// same containment question.
    fn containment_cases(fixtures: &Fixtures) -> Vec<Case<'_>> {
        vec![
            Case {
                description: "a point inside the reference area is within it",
                georel: GeoRel::Within,
                subject: &fixtures.inside,
                reference: &fixtures.area,
                expected: true,
            },
            Case {
                description: "a point outside the reference area is not within it",
                georel: GeoRel::Within,
                subject: &fixtures.outside,
                reference: &fixtures.area,
                expected: false,
            },
            Case {
                description: "an area containing the reference point contains it",
                georel: GeoRel::Contains,
                subject: &fixtures.area,
                reference: &fixtures.inside,
                expected: true,
            },
            Case {
                description: "an area not covering the reference point does not contain it",
                georel: GeoRel::Contains,
                subject: &fixtures.area,
                reference: &fixtures.outside,
                expected: false,
            },
        ]
    }

    /// The rows asserting `intersects` and its negation `disjoint`.
    fn intersection_cases(fixtures: &Fixtures) -> Vec<Case<'_>> {
        vec![
            Case {
                description: "overlapping areas intersect",
                georel: GeoRel::Intersects,
                subject: &fixtures.area,
                reference: &fixtures.same_area,
                expected: true,
            },
            Case {
                description: "separated areas do not intersect",
                georel: GeoRel::Intersects,
                subject: &fixtures.area,
                reference: &fixtures.far_area,
                expected: false,
            },
            Case {
                description: "separated areas are disjoint",
                georel: GeoRel::Disjoint,
                subject: &fixtures.area,
                reference: &fixtures.far_area,
                expected: true,
            },
            Case {
                description: "overlapping areas are not disjoint",
                georel: GeoRel::Disjoint,
                subject: &fixtures.area,
                reference: &fixtures.same_area,
                expected: false,
            },
        ]
    }

    /// The rows asserting `equals`.
    fn equality_cases(fixtures: &Fixtures) -> Vec<Case<'_>> {
        vec![
            Case {
                description: "identical areas are equal",
                georel: GeoRel::Equals,
                subject: &fixtures.area,
                reference: &fixtures.same_area,
                expected: true,
            },
            Case {
                description: "different areas are not equal",
                georel: GeoRel::Equals,
                subject: &fixtures.area,
                reference: &fixtures.far_area,
                expected: false,
            },
        ]
    }

    /// The rows asserting `overlaps`, including the containment and dimension
    /// exclusions `ST_Overlaps` carries.
    fn overlap_cases(fixtures: &Fixtures) -> Vec<Case<'_>> {
        vec![
            Case {
                description: "partially overlapping areas overlap",
                georel: GeoRel::Overlaps,
                subject: &fixtures.area,
                reference: &fixtures.shifted_area,
                expected: true,
            },
            Case {
                description: "separated areas do not overlap",
                georel: GeoRel::Overlaps,
                subject: &fixtures.area,
                reference: &fixtures.far_area,
                expected: false,
            },
            Case {
                // `ST_Overlaps` excludes containment: an area wholly inside
                // another intersects it but does not overlap it.
                description: "an area contained by the reference does not overlap it",
                georel: GeoRel::Overlaps,
                subject: &fixtures.inner_area,
                reference: &fixtures.area,
                expected: false,
            },
            Case {
                description: "identical areas do not overlap, they are equal",
                georel: GeoRel::Overlaps,
                subject: &fixtures.area,
                reference: &fixtures.same_area,
                expected: false,
            },
            Case {
                // Dimensions differ, so the pair can intersect but never
                // overlap.
                description: "a point inside an area does not overlap it",
                georel: GeoRel::Overlaps,
                subject: &fixtures.inside,
                reference: &fixtures.area,
                expected: false,
            },
        ]
    }

    /// The rows asserting `near`, over every combination of its two bounds.
    fn proximity_cases(fixtures: &Fixtures) -> Vec<Case<'_>> {
        vec![
            Case {
                // The two fixtures are ~7 300 km apart on the great circle.
                description: "a point inside the max distance is near",
                georel: GeoRel::Near {
                    max_distance: Some(PositiveDistance::from_str("10000000").expect("positive distance")),
                    min_distance: None,
                },
                subject: &fixtures.inside,
                reference: &fixtures.outside,
                expected: true,
            },
            Case {
                description: "a point beyond the max distance is not near",
                georel: GeoRel::Near {
                    max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                    min_distance: None,
                },
                subject: &fixtures.inside,
                reference: &fixtures.outside,
                expected: false,
            },
            Case {
                description: "a point beyond the min distance is near",
                georel: GeoRel::Near {
                    max_distance: None,
                    min_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                },
                subject: &fixtures.inside,
                reference: &fixtures.outside,
                expected: true,
            },
            Case {
                description: "a point inside the min distance is not near",
                georel: GeoRel::Near {
                    max_distance: None,
                    min_distance: Some(PositiveDistance::from_str("10000000").expect("positive distance")),
                },
                subject: &fixtures.inside,
                reference: &fixtures.outside,
                expected: false,
            },
            Case {
                description: "a point satisfying neither bound is not near",
                georel: GeoRel::Near {
                    max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                    min_distance: Some(PositiveDistance::from_str("100").expect("positive distance")),
                },
                subject: &fixtures.inside,
                reference: &fixtures.outside,
                expected: false,
            },
            Case {
                description: "an unbounded near matches anything",
                georel: GeoRel::Near {
                    max_distance: None,
                    min_distance: None,
                },
                subject: &fixtures.inside,
                reference: &fixtures.outside,
                expected: true,
            },
        ]
    }

    /// The geometries the conformance table is written against.
    struct Fixtures {
        /// A point inside `area`.
        inside: Geometry,
        /// A point far outside `area`.
        outside: Geometry,
        /// The reference area.
        area: Geometry,
        /// An area with the same footprint as `area`.
        same_area: Geometry,
        /// An area partially covering `area`.
        shifted_area: Geometry,
        /// An area wholly inside `area`.
        inner_area: Geometry,
        /// An area far from `area`.
        far_area: Geometry,
    }

    impl Fixtures {
        /// The two-dimensional fixtures.
        fn flat() -> Fixtures {
            Fixtures {
                inside: point(0.0, 0.0),
                outside: point(50.0, 50.0),
                area: square(0.0, 0.0, 1.0),
                same_area: square(0.0, 0.0, 1.0),
                shifted_area: square(1.0, 0.0, 1.0),
                inner_area: square(0.0, 0.0, 0.25),
                far_area: square(50.0, 50.0, 1.0),
            }
        }

        /// The same footprints, every position lifted to a distinct altitude so
        /// that no two fixtures share one.
        fn elevated(flat: &Fixtures) -> Fixtures {
            Fixtures {
                inside: lift(&flat.inside, 10.0),
                outside: lift(&flat.outside, 20.0),
                area: lift(&flat.area, 30.0),
                same_area: lift(&flat.same_area, 40.0),
                shifted_area: lift(&flat.shifted_area, 50.0),
                inner_area: lift(&flat.inner_area, 60.0),
                far_area: lift(&flat.far_area, 70.0),
            }
        }
    }

    #[test]
    fn every_georel_variant_evaluates_through_one_in_memory_matcher() {
        let fixtures = Fixtures::flat();
        for case in conformance_table(&fixtures) {
            pretty_assertions::assert_eq!(matches(&case.georel, case.subject, case.reference), case.expected, "{}", case.description);
        }
    }

    #[test]
    fn every_georel_verdict_ignores_altitude() {
        // OGC 06-103r4 evaluates the clause 4.10 predicates on the horizontal
        // plane, so lifting every fixture to a different altitude — which would
        // separate them entirely in three dimensions — must change no verdict.
        let flat = Fixtures::flat();
        let elevated = Fixtures::elevated(&flat);
        for case in conformance_table(&elevated) {
            pretty_assertions::assert_eq!(
                matches(&case.georel, case.subject, case.reference),
                case.expected,
                "{} (elevated)",
                case.description
            );
        }
    }

    #[test]
    fn the_projection_of_an_elevated_geometry_matches_its_flat_footprint() {
        let flat = Fixtures::flat();
        let elevated = Fixtures::elevated(&flat);
        pretty_assertions::assert_eq!(elevated.area.planar(), flat.area.planar());
    }
}
