//! In-memory evaluation of a clause 4.10 spatial relation.
//!
//! One evaluator, used by every in-memory matcher in the broker. Before this
//! module existed the same seven match arms were written three times — in the
//! entity spatial index, in the subscription spatial index and in subscription
//! matching — and one of the copies carried a comment asserting it "matches the
//! semantics of" another. That assertion is now the compiler's.
//!
//! The SQL renderings are a separate reading of the same clause, because
//! PostGIS evaluates the predicates rather than this crate. The pairs they
//! render to are named in the doc comments below so the two stay comparable.

use crate::{distance::haversine_distance_m, geo_rel::GeoRel, geometry::Geometry};
use geo::algorithm::{Contains, Intersects};

/// Whether `subject` stands in the relation `georel` to `reference`.
///
/// `subject` is the geometry read off the entity or registration; `reference`
/// is the one the query supplied. The asymmetry matters: `within` asks whether
/// the subject is inside the reference, `contains` the opposite.
pub fn matches(georel: &GeoRel, subject: &Geometry, reference: &Geometry) -> bool {
    matches_geo(georel, subject.as_ref(), reference.as_ref())
}

/// [`matches()`] over the underlying `geo` geometries.
///
/// Spatial indexes hold `geo::Geometry` directly, so requiring the newtype at
/// that boundary would cost a wrap per candidate.
pub fn matches_geo(georel: &GeoRel, subject: &geo::Geometry<f64>, reference: &geo::Geometry<f64>) -> bool {
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
        // `ST_Overlaps(subject, reference)`
        GeoRel::Overlaps => subject.intersects(reference),
        // `ST_DWithin` / `ST_Distance` over `geography`; here a Haversine
        // distance between centroids, which is what the `geography` type
        // approximates for non-point geometries.
        GeoRel::Near { max_distance, min_distance } => {
            let distance = haversine_distance_m(subject, reference);
            max_distance.is_none_or(|max| distance <= max) && min_distance.is_none_or(|min| distance >= min)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{Coord, Geometry as GeoGeometry, LineString, Point, Polygon};

    fn point(x: f64, y: f64) -> geo::Geometry<f64> {
        GeoGeometry::Point(Point::new(x, y))
    }

    fn square(cx: f64, cy: f64, half: f64) -> geo::Geometry<f64> {
        GeoGeometry::Polygon(Polygon::new(
            LineString(vec![
                Coord { x: cx - half, y: cy - half },
                Coord { x: cx + half, y: cy - half },
                Coord { x: cx + half, y: cy + half },
                Coord { x: cx - half, y: cy + half },
                Coord { x: cx - half, y: cy - half },
            ]),
            Vec::new(),
        ))
    }

    /// One row of the conformance table: a relation, the two geometries it is
    /// evaluated over, and the verdict clause 4.10 requires.
    struct Case<'a> {
        /// What the row asserts, used as the assertion message.
        description: &'a str,
        /// The relation under test.
        georel: GeoRel,
        /// The geometry read off the entity.
        subject: &'a geo::Geometry<f64>,
        /// The geometry the query supplied.
        reference: &'a geo::Geometry<f64>,
        /// The verdict the evaluator must reach.
        expected: bool,
    }

    /// One table over every clause 4.10 relation, driven through the single
    /// evaluator. The fixtures are the ones the three deleted copies each
    /// tested separately.
    #[test]
    fn every_georel_variant_evaluates_through_one_in_memory_matcher() {
        let inside = point(0.0, 0.0);
        let outside = point(50.0, 50.0);
        let area = square(0.0, 0.0, 1.0);
        let same_area = square(0.0, 0.0, 1.0);
        let far_area = square(50.0, 50.0, 1.0);

        let cases = vec![
            Case {
                description: "a point inside the reference area is within it",
                georel: GeoRel::Within,
                subject: &inside,
                reference: &area,
                expected: true,
            },
            Case {
                description: "a point outside the reference area is not within it",
                georel: GeoRel::Within,
                subject: &outside,
                reference: &area,
                expected: false,
            },
            Case {
                description: "an area containing the reference point contains it",
                georel: GeoRel::Contains,
                subject: &area,
                reference: &inside,
                expected: true,
            },
            Case {
                description: "an area not covering the reference point does not contain it",
                georel: GeoRel::Contains,
                subject: &area,
                reference: &outside,
                expected: false,
            },
            Case {
                description: "overlapping areas intersect",
                georel: GeoRel::Intersects,
                subject: &area,
                reference: &same_area,
                expected: true,
            },
            Case {
                description: "separated areas do not intersect",
                georel: GeoRel::Intersects,
                subject: &area,
                reference: &far_area,
                expected: false,
            },
            Case {
                description: "separated areas are disjoint",
                georel: GeoRel::Disjoint,
                subject: &area,
                reference: &far_area,
                expected: true,
            },
            Case {
                description: "overlapping areas are not disjoint",
                georel: GeoRel::Disjoint,
                subject: &area,
                reference: &same_area,
                expected: false,
            },
            Case {
                description: "identical areas are equal",
                georel: GeoRel::Equals,
                subject: &area,
                reference: &same_area,
                expected: true,
            },
            Case {
                description: "different areas are not equal",
                georel: GeoRel::Equals,
                subject: &area,
                reference: &far_area,
                expected: false,
            },
            Case {
                description: "overlapping areas overlap",
                georel: GeoRel::Overlaps,
                subject: &area,
                reference: &same_area,
                expected: true,
            },
            Case {
                description: "separated areas do not overlap",
                georel: GeoRel::Overlaps,
                subject: &area,
                reference: &far_area,
                expected: false,
            },
            Case {
                // The two fixtures are ~7 300 km apart on the great circle.
                description: "a point inside the max distance is near",
                georel: GeoRel::Near {
                    max_distance: Some(10_000_000.0),
                    min_distance: None,
                },
                subject: &inside,
                reference: &outside,
                expected: true,
            },
            Case {
                description: "a point beyond the max distance is not near",
                georel: GeoRel::Near {
                    max_distance: Some(1_000.0),
                    min_distance: None,
                },
                subject: &inside,
                reference: &outside,
                expected: false,
            },
            Case {
                description: "a point beyond the min distance is near",
                georel: GeoRel::Near {
                    max_distance: None,
                    min_distance: Some(1_000.0),
                },
                subject: &inside,
                reference: &outside,
                expected: true,
            },
            Case {
                description: "a point inside the min distance is not near",
                georel: GeoRel::Near {
                    max_distance: None,
                    min_distance: Some(10_000_000.0),
                },
                subject: &inside,
                reference: &outside,
                expected: false,
            },
            Case {
                description: "a point satisfying neither bound is not near",
                georel: GeoRel::Near {
                    max_distance: Some(1_000.0),
                    min_distance: Some(100.0),
                },
                subject: &inside,
                reference: &outside,
                expected: false,
            },
            Case {
                description: "an unbounded near matches anything",
                georel: GeoRel::Near {
                    max_distance: None,
                    min_distance: None,
                },
                subject: &inside,
                reference: &outside,
                expected: true,
            },
        ];

        for case in cases {
            pretty_assertions::assert_eq!(matches_geo(&case.georel, case.subject, case.reference), case.expected, "{}", case.description);
        }
    }

    #[test]
    fn the_newtype_entry_point_agrees_with_the_geo_one() {
        let subject = Geometry::from(point(0.0, 0.0));
        let reference = Geometry::from(square(0.0, 0.0, 1.0));
        assert!(matches(&GeoRel::Within, &subject, &reference));
        assert!(matches_geo(&GeoRel::Within, subject.as_ref(), reference.as_ref()));
    }
}
