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
//! `near` alone measures on the earth rather than on the plane: the minimum
//! distance between the two geometries on the WGS 84 spheroid, as
//! [`crate::proximity`] sets out.
//!
//! The SQL renderings are a separate reading of the same clause, because
//! `PostGIS` evaluates the predicates rather than this crate. The pairs they
//! render to are named in the doc comments below so the two stay comparable.

use crate::{
    geo_rel::GeoRel,
    geometry::Geometry,
    proximity::{Boundary, Proximity},
};
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
        // `ST_DWithin(subject, reference, max)` and
        // `ST_Distance(subject, reference) >= min` over `geography`: the minimum
        // distance between the two geometries on the WGS 84 spheroid (OGC
        // 06-103r4 clause 6.1.2.4 `Distance`), zero when they touch. A geometry
        // with no coordinates is nowhere, so it is near nothing.
        GeoRel::Near { max_distance, min_distance } => Proximity::between(subject, reference).is_some_and(|proximity| {
            max_distance.is_none_or(|max| proximity.comes_within(max, Boundary::Inclusive))
                && min_distance.is_none_or(|min| !proximity.comes_within(min, Boundary::Exclusive))
        }),
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
    use crate::{
        form::{GeometryForm, PolygonRings},
        geo_rel::GeoRel,
        geometry::Geometry,
        line_string_coordinates::LineStringCoordinates,
        linear_ring::LinearRing,
        position::Position,
        positive_distance::PositiveDistance,
        relation::matches,
    };
    use std::str::FromStr;

    fn point(longitude: f64, latitude: f64) -> Geometry {
        Geometry::new(GeometryForm::Point(
            Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes"),
        ))
    }

    fn square(centre_longitude: f64, centre_latitude: f64, half: f64) -> Geometry {
        Geometry::new(GeometryForm::Polygon(PolygonRings::new(vec![box_ring(
            centre_longitude - half,
            centre_latitude - half,
            centre_longitude + half,
            centre_latitude + half,
        )])))
    }

    /// The closed ring around a longitude/latitude box.
    fn box_ring(west: f64, south: f64, east: f64, north: f64) -> LinearRing {
        LinearRing::try_new(
            [(west, south), (east, south), (east, north), (west, north), (west, south)]
                .into_iter()
                .map(|(longitude, latitude)| Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes"))
                .collect(),
        )
        .expect("a test box is a closed ring")
    }

    /// A polygon of one or more rings, the first being the exterior.
    fn polygon(rings: Vec<LinearRing>) -> Geometry {
        Geometry::new(GeometryForm::Polygon(PolygonRings::new(rings)))
    }

    /// A line through the given positions.
    fn line(positions: &[(f64, f64)]) -> Geometry {
        Geometry::new(GeometryForm::LineString(
            LineStringCoordinates::try_new(
                positions
                    .iter()
                    .map(|&(longitude, latitude)| Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes"))
                    .collect(),
            )
            .expect("a test line has two positions or more"),
        ))
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
            GeometryForm::LineString(line_string) => GeometryForm::LineString(lift_line_string(line_string, altitude)),
            GeometryForm::MultiLineString(line_strings) => {
                GeometryForm::MultiLineString(line_strings.iter().map(|line_string| lift_line_string(line_string, altitude)).collect())
            }
            GeometryForm::Polygon(rings) => GeometryForm::Polygon(lift_rings(rings, altitude)),
            GeometryForm::MultiPolygon(polygons) => GeometryForm::MultiPolygon(polygons.iter().map(|polygon| lift_rings(polygon, altitude)).collect()),
        }
    }

    /// Lift one position to the given altitude.
    fn lift_position(position: &Position, altitude: f64) -> Position {
        Position::try_new(position.longitude(), position.latitude(), Some(altitude)).expect("a test position lies on the CRS84 axes")
    }

    /// Lift a run of positions to the given altitude.
    fn lift_positions(positions: &[Position], altitude: f64) -> Vec<Position> {
        positions.iter().map(|position| lift_position(position, altitude)).collect()
    }

    /// Lift a line to the given altitude.
    fn lift_line_string(line_string: &LineStringCoordinates, altitude: f64) -> LineStringCoordinates {
        LineStringCoordinates::try_new(lift_positions(line_string.positions(), altitude)).expect("lifting keeps the position count")
    }

    /// Lift the rings of a polygon to the given altitude.
    fn lift_rings(rings: &PolygonRings, altitude: f64) -> PolygonRings {
        PolygonRings::new(
            rings
                .rings()
                .iter()
                .map(|ring| LinearRing::try_new(lift_positions(ring.positions(), altitude)).expect("lifting every vertex alike keeps the ring closed"))
                .collect(),
        )
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
        .into_iter()
        .chain(minimum_distance_cases(fixtures))
        .collect()
    }

    /// A `near` relation with the given bounds, in metres.
    fn near(max_distance: Option<&str>, min_distance: Option<&str>) -> GeoRel {
        GeoRel::Near {
            max_distance: max_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
            min_distance: min_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
        }
    }

    /// The rows asserting that `near` measures the minimum distance between
    /// the geometries on the WGS 84 spheroid (OGC 06-103r4 clause 6.1.2.4),
    /// never the distance between their centroids.
    fn minimum_distance_cases(fixtures: &Fixtures) -> Vec<Case<'_>> {
        area_distance_cases(fixtures).into_iter().chain(shape_and_earth_cases(fixtures)).collect()
    }

    /// The rows measuring from areas: a polygon's nearest wall, a point inside
    /// one, a point inside its hole.
    fn area_distance_cases(fixtures: &Fixtures) -> Vec<Case<'_>> {
        vec![
            Case {
                description: "a polygon whose nearest wall is inside maxDistance is near even when its centroid is not",
                georel: near(Some("150"), None),
                subject: &fixtures.east_square,
                reference: &fixtures.bremen,
                expected: true,
            },
            Case {
                description: "a polygon whose nearest wall is inside minDistance is not near",
                georel: near(None, Some("150")),
                subject: &fixtures.east_square,
                reference: &fixtures.bremen,
                expected: false,
            },
            Case {
                description: "a polygon whose nearest wall is beyond maxDistance is not near",
                georel: near(Some("90"), None),
                subject: &fixtures.east_square,
                reference: &fixtures.bremen,
                expected: false,
            },
            Case {
                description: "a point inside a polygon is at distance zero for maxDistance",
                georel: near(Some("500"), None),
                subject: &fixtures.bremen_area,
                reference: &fixtures.bremen,
                expected: true,
            },
            Case {
                description: "a point inside a polygon is never beyond a minDistance",
                georel: near(None, Some("500")),
                subject: &fixtures.bremen_area,
                reference: &fixtures.bremen,
                expected: false,
            },
            Case {
                description: "a point in a hole is farther than a maxDistance short of the hole boundary",
                georel: near(Some("50"), None),
                subject: &fixtures.holed,
                reference: &fixtures.bremen,
                expected: false,
            },
            Case {
                description: "a point in a hole is near a maxDistance past the hole boundary",
                georel: near(Some("150"), None),
                subject: &fixtures.holed,
                reference: &fixtures.bremen,
                expected: true,
            },
            Case {
                description: "a point in a hole is beyond a minDistance short of the hole boundary",
                georel: near(None, Some("50")),
                subject: &fixtures.holed,
                reference: &fixtures.bremen,
                expected: true,
            },
        ]
    }

    /// The rows measuring lines, polygon references, the spheroid, the
    /// antimeridian and empty geometries.
    fn shape_and_earth_cases(fixtures: &Fixtures) -> Vec<Case<'_>> {
        vec![
            Case {
                description: "a line is near through the interior of its nearest edge",
                georel: near(Some("150"), None),
                subject: &fixtures.east_line,
                reference: &fixtures.bremen,
                expected: true,
            },
            Case {
                description: "a polygon reference is measured from its nearest edge",
                georel: near(Some("100"), None),
                subject: &fixtures.east_of_square,
                reference: &fixtures.bremen_square,
                expected: true,
            },
            Case {
                description: "near is measured on the spheroid, where the point is inside maxDistance",
                georel: near(Some("5014"), None),
                subject: &fixtures.north_of_origin,
                reference: &fixtures.inside,
                expected: true,
            },
            Case {
                description: "near is measured on the spheroid, where the point is inside minDistance",
                georel: near(None, Some("5014")),
                subject: &fixtures.north_of_origin,
                reference: &fixtures.inside,
                expected: false,
            },
            Case {
                description: "a polygon cut at the antimeridian is near across it",
                georel: near(Some("300"), None),
                subject: &fixtures.cut_at_antimeridian,
                reference: &fixtures.west_of_antimeridian,
                expected: true,
            },
            Case {
                description: "an empty geometry is within no maxDistance",
                georel: near(Some("1000000"), None),
                subject: &fixtures.empty,
                reference: &fixtures.bremen,
                expected: false,
            },
            Case {
                description: "an empty geometry is beyond no minDistance",
                georel: near(None, Some("1")),
                subject: &fixtures.empty,
                reference: &fixtures.bremen,
                expected: false,
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
        /// The reference point the minimum-distance fixtures are placed
        /// around, in Bremen.
        bremen: Geometry,
        /// A square whose west wall is 100.06 m east of `bremen` and whose
        /// centroid is 170.10 m east of it.
        east_square: Geometry,
        /// An area containing `bremen`, whose centroid is 995.24 m from it.
        bremen_area: Geometry,
        /// An area with a hole around `bremen`, whose nearest hole wall is
        /// 100.06 m from it.
        holed: Geometry,
        /// A line whose nearest vertex is 316.70 m from `bremen` and whose
        /// nearest edge passes 100.06 m from it.
        east_line: Geometry,
        /// A square centred on `bremen`.
        bremen_square: Geometry,
        /// A point 90.07 m east of `bremen_square` and 140.07 m from its centre.
        east_of_square: Geometry,
        /// A point 5 000.17 m north of `inside` on the spheroid, 5 028.24 m on
        /// the sphere.
        north_of_origin: Geometry,
        /// A polygon just east of the antimeridian, 212.97 m from
        /// `west_of_antimeridian` and 692.16 m from it by its centroid.
        cut_at_antimeridian: Geometry,
        /// A point just west of the antimeridian.
        west_of_antimeridian: Geometry,
        /// A polygon with no rings.
        empty: Geometry,
    }

    impl Fixtures {
        /// The two-dimensional fixtures.
        fn flat() -> Fixtures {
            let (x, y) = (8.8072, 53.0758);
            Fixtures {
                inside: point(0.0, 0.0),
                outside: point(50.0, 50.0),
                area: square(0.0, 0.0, 1.0),
                same_area: square(0.0, 0.0, 1.0),
                shifted_area: square(1.0, 0.0, 1.0),
                inner_area: square(0.0, 0.0, 0.25),
                far_area: square(50.0, 50.0, 1.0),
                bremen: point(x, y),
                east_square: polygon(vec![box_ring(8.808_693, 53.075_171, 8.810_783, 53.076_429)]),
                bremen_area: polygon(vec![box_ring(8.8069, 53.0753, 8.8372, 53.0763)]),
                holed: polygon(vec![
                    box_ring(x - 0.015, y - 0.009, x + 0.015, y + 0.009),
                    box_ring(x - 0.001_493, y - 0.000_898, x + 0.001_493, y + 0.000_898),
                ]),
                east_line: line(&[(8.808_693, 53.0731), (8.808_693, 53.0785), (8.817_693, 53.0785)]),
                bremen_square: polygon(vec![box_ring(8.806_454, 53.075_351, 8.807_946, 53.076_249)]),
                east_of_square: point(8.809_290, y),
                north_of_origin: point(0.0, 0.045_22),
                cut_at_antimeridian: Geometry::new(GeometryForm::MultiPolygon(vec![PolygonRings::new(vec![box_ring(
                    -179.999, -17.0005, -179.990, -16.9995,
                )])])),
                west_of_antimeridian: point(179.999, -17.0),
                empty: Geometry::new(GeometryForm::Polygon(PolygonRings::new(Vec::new()))),
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
                bremen: lift(&flat.bremen, 80.0),
                east_square: lift(&flat.east_square, 500.0),
                bremen_area: lift(&flat.bremen_area, 90.0),
                holed: lift(&flat.holed, 100.0),
                east_line: lift(&flat.east_line, 110.0),
                bremen_square: lift(&flat.bremen_square, 120.0),
                east_of_square: lift(&flat.east_of_square, 130.0),
                north_of_origin: lift(&flat.north_of_origin, 140.0),
                cut_at_antimeridian: lift(&flat.cut_at_antimeridian, 150.0),
                west_of_antimeridian: lift(&flat.west_of_antimeridian, 160.0),
                empty: lift(&flat.empty, 170.0),
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
