//! The one reading of each clause 4.10 `georel`: which geometry plays which
//! role, the model the relation is decided in, and the weaker relation a Context
//! Source Registration's geometry is matched by. The in-memory evaluator, the
//! SQL rendering and the registration matchers take these decisions from
//! [`RelationReading`] and make none of their own.
//!
//! Operands. Every relation compares the subject, the target geometry clause
//! 4.10 reads off the Entity (or off the registration), with the reference
//! geometry the query supplies. `within` and `contains` are the one asymmetric
//! pair: `within` has the reference contain the subject, `contains` the subject
//! contain the reference. The other relations read both operands alike.
//!
//! Model and altitude. The six topological relations are OGC 06-103r4
//! predicates over the horizontal projection of both geometries: its clause
//! 6.1.2.5 has spatial operations work in the "map geometry" of the data,
//! reflecting neither z nor m, and its clause 6.1.15.1 defines the relational
//! operators by projection onto the horizontal reference plane. `near` measures
//! the minimum distance between the two geometries on the WGS 84 spheroid, the
//! datum RFC 7946 places every position in, between the same horizontal
//! positions. Altitude therefore enters no verdict, however many ordinates the
//! geometries carry.
//!
//! Absence and instances. Clause 4.10 makes an Entity that does not convey the
//! target `GeoProperty` non-matching, so no relation holds for it, `disjoint`
//! and `near` with only a `minDistance` included: neither is decided as the
//! complement of another relation over the Entity. Clause 5.7.2.4 makes one
//! instance meeting the relation enough when the `GeoProperty` has several,
//! and clause 5.8.6 matches a Subscription's `geoQ` as clause 4.10 mandates, so
//! every execution decides a relation per instance and asks whether one holds.
//!
//! Registrations. A registration's `location`, `observationSpace` and
//! `operationSpace` (Table 5.2.9-1) bound where the source's Entities are, not
//! where one Entity is. Clauses 5.10.2.4 and 5.11.2.4 therefore let a geoquery
//! match "if the location directly matches or if the location possibly
//! contains locations that would match the geoquery". An Entity geometry `E`
//! lying inside the registration geometry `R` can meet each relation only if
//! `R` meets a weaker one:
//!
//! - `within`, `intersects` and `overlaps` need some part of `E`, hence of `R`,
//!   to meet the reference: `R` intersects it;
//! - `contains` and `equals` need the reference inside `E`, hence inside `R`:
//!   `R` contains it;
//! - `disjoint` needs some part of `E`, hence of `R`, outside the reference:
//!   `R` is not within it;
//! - `near` with a `maxDistance` needs some point of `R` that close: the
//!   shortest distance from `R` is within it. A `minDistance` alone bounds
//!   nothing, since `R` may reach arbitrarily far from its nearest point.
//!
//! Each weaker relation holds wherever the relation holds for `R` itself, so a
//! direct match is always kept.

use crate::{geo_rel::GeoRel, geometry::Geometry, planar::PlanarGeometry, positive_distance::PositiveDistance, relation::matches};

/// One of the two geometries a relation compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    /// The target geometry, read off the Entity or the registration.
    Subject,
    /// The reference geometry the query supplies.
    Reference,
}

/// A topological relation of OGC 06-103r4, decided over the horizontal
/// projection of both geometries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologicalTest {
    /// `container` contains the other operand: the reference for `within`,
    /// the subject for `contains`.
    Contains {
        /// The operand that contains the other.
        container: Operand,
    },
    /// The two geometries share a point.
    Intersects,
    /// The two geometries share no point.
    Disjoint,
    /// The two geometries are topologically equal.
    Equals,
    /// The two geometries share some but not all of their interiors and are
    /// of one dimension, which OGC 06-103r4 defines `Overlaps` between only.
    Overlaps,
}

/// What a `georel` tests, and through it the model the test is decided in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpatialTest {
    /// A topological relation, decided on the plane.
    Topological(TopologicalTest),
    /// The minimum distance between the two geometries on the WGS 84 spheroid
    /// lies in a band, zero when they share a point. Clause 4.10 requires at
    /// least one bound.
    Distance {
        /// The distance the geometries come within, inclusive.
        max_distance: Option<PositiveDistance>,
        /// The distance the geometries stay at or beyond.
        min_distance: Option<PositiveDistance>,
    },
}

/// What a registration geometry meets against the reference for an Entity
/// inside it to possibly match a geoquery.
#[derive(Debug, Clone, PartialEq)]
pub enum AreaCondition {
    /// The registration geometry stands in this relation to the reference.
    Relation(GeoRel),
    /// The registration geometry is not within the reference.
    NotWithin,
    /// Any registration geometry may hold a match.
    Unconstrained,
}

/// How one `georel` is decided: the test an Entity's geometry meets, and the
/// condition a registration's geometry meets.
#[derive(Debug, Clone, PartialEq)]
pub struct RelationReading {
    /// What the subject and the reference are tested for.
    pub test: SpatialTest,
    /// What a registration geometry is matched by.
    pub registration: AreaCondition,
}

impl RelationReading {
    /// The reading of `georel`.
    #[must_use]
    pub const fn of(georel: &GeoRel) -> RelationReading {
        let (test, registration) = match georel {
            GeoRel::Within => (
                SpatialTest::Topological(TopologicalTest::Contains { container: Operand::Reference }),
                AreaCondition::Relation(GeoRel::Intersects),
            ),
            GeoRel::Contains => (
                SpatialTest::Topological(TopologicalTest::Contains { container: Operand::Subject }),
                AreaCondition::Relation(GeoRel::Contains),
            ),
            GeoRel::Intersects => (
                SpatialTest::Topological(TopologicalTest::Intersects),
                AreaCondition::Relation(GeoRel::Intersects),
            ),
            GeoRel::Disjoint => (SpatialTest::Topological(TopologicalTest::Disjoint), AreaCondition::NotWithin),
            GeoRel::Equals => (SpatialTest::Topological(TopologicalTest::Equals), AreaCondition::Relation(GeoRel::Contains)),
            GeoRel::Overlaps => (SpatialTest::Topological(TopologicalTest::Overlaps), AreaCondition::Relation(GeoRel::Intersects)),
            GeoRel::Near { max_distance, min_distance } => (
                SpatialTest::Distance {
                    max_distance: *max_distance,
                    min_distance: *min_distance,
                },
                match max_distance {
                    Some(max_distance) => AreaCondition::Relation(GeoRel::Near {
                        max_distance: Some(*max_distance),
                        min_distance: None,
                    }),
                    None => AreaCondition::Unconstrained,
                },
            ),
        };
        RelationReading { test, registration }
    }
}

impl AreaCondition {
    /// Whether `area` meets this condition against `reference`.
    #[must_use]
    pub fn admits(&self, area: &Geometry, reference: &PlanarGeometry) -> bool {
        match self {
            AreaCondition::Relation(georel) => matches(georel, &area.planar(), reference),
            AreaCondition::NotWithin => !matches(&GeoRel::Within, &area.planar(), reference),
            AreaCondition::Unconstrained => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        geo_rel::GeoRel,
        relation_reading::{AreaCondition, Operand, RelationReading, SpatialTest, TopologicalTest},
    };
    use pretty_assertions::assert_eq;

    fn reading(georel: &str) -> RelationReading {
        RelationReading::of(&georel.parse::<GeoRel>().expect("a valid georel"))
    }

    /// Clause 4.10 has the target geometry "within" the reference for
    /// `within` and "contain" the reference for `contains`.
    #[test]
    fn within_and_contains_name_opposite_containers() {
        assert_eq!(
            reading("within").test,
            SpatialTest::Topological(TopologicalTest::Contains { container: Operand::Reference })
        );
        assert_eq!(
            reading("contains").test,
            SpatialTest::Topological(TopologicalTest::Contains { container: Operand::Subject })
        );
    }

    #[test]
    fn the_symmetric_relations_read_their_own_topological_test() {
        for (georel, test) in [
            ("intersects", TopologicalTest::Intersects),
            ("disjoint", TopologicalTest::Disjoint),
            ("equals", TopologicalTest::Equals),
            ("overlaps", TopologicalTest::Overlaps),
        ] {
            assert_eq!(reading(georel).test, SpatialTest::Topological(test), "{georel}");
        }
    }

    /// Only `near` measures a distance, the one test decided on the spheroid.
    #[test]
    fn near_keeps_both_bounds_of_its_distance_band() {
        let near = "near;maxDistance==2000;minDistance==100".parse::<GeoRel>().expect("a valid georel");
        let GeoRel::Near { max_distance, min_distance } = near else {
            panic!("near parses to the near relation");
        };
        assert_eq!(RelationReading::of(&near).test, SpatialTest::Distance { max_distance, min_distance });
    }

    #[test]
    fn each_relation_names_the_weaker_relation_a_registration_meets() {
        for (georel, registration) in [
            ("within", AreaCondition::Relation(GeoRel::Intersects)),
            ("intersects", AreaCondition::Relation(GeoRel::Intersects)),
            ("overlaps", AreaCondition::Relation(GeoRel::Intersects)),
            ("contains", AreaCondition::Relation(GeoRel::Contains)),
            ("equals", AreaCondition::Relation(GeoRel::Contains)),
            ("disjoint", AreaCondition::NotWithin),
            ("near;minDistance==100", AreaCondition::Unconstrained),
        ] {
            assert_eq!(reading(georel).registration, registration, "{georel}");
        }
    }

    /// A registration area within the maximum distance at its nearest point
    /// may hold a match whatever the minimum distance asks.
    #[test]
    fn a_near_registration_keeps_only_the_maximum_distance() {
        let near = "near;maxDistance==2000;minDistance==100".parse::<GeoRel>().expect("a valid georel");
        let GeoRel::Near { max_distance, .. } = near else {
            panic!("near parses to the near relation");
        };
        assert_eq!(
            RelationReading::of(&near).registration,
            AreaCondition::Relation(GeoRel::Near {
                max_distance,
                min_distance: None
            })
        );
    }
}
