//! A stored geometry as a geo-query reads it, and the one box test every
//! relation narrows it by first (clause 4.10).
//!
//! Each stored geometry has two readings. A relation judges the geometry
//! itself: the repaired form of an invalid written geometry, else the written
//! one. A box test reads the box its spatial index keys: the geometry's
//! great-circle reach where it has one, else that same geometry. `near`
//! measures along great-circle edges, and a long edge bows past its vertices,
//! so a box test against the planar box of the vertices alone can discard a
//! geometry the distance keeps. The reach contains the planar box, so the
//! planar relations lose nothing to it either.
//!
//! Every box test is rendered here, against a [`GeometryBox`], which only a
//! [`StoredGeometry`] or a stream extent grown from reaches yields: a bare
//! geometry column never reaches a box test. A workspace architecture check
//! refuses the box operators anywhere else.

use crate::error::RenderError;
use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, great_circle::great_circle_bounds, near_envelope::NearEnvelope};
use andromeda_postgresql_encoding::ewkb::encode;
use andromeda_postgresql_seaquery::{
    cast::cast,
    pg_type::PostgresType,
    postgis::{bounding_boxes_overlap, judged_geometry, reach_or_geometry, st_make_envelope, unindexed_bounding_boxes_overlap, validated_geom_from_ewkb},
};
use sea_query::{Expr as SeaExpr, ExprTrait, SimpleExpr};

/// One stored geometry: what a relation judges, and the box its index keys.
#[derive(Debug, Clone)]
pub struct StoredGeometry {
    /// The geometry a relation judges.
    judged: SimpleExpr,
    /// `COALESCE(reach, …)`, spelled as the spatial index keys it.
    indexed: SimpleExpr,
}

impl StoredGeometry {
    /// A geometry stored as written beside its repaired form and its reach:
    /// it judges `COALESCE(repaired, written)` and keys
    /// `COALESCE(reach, repaired, written)`.
    #[must_use]
    pub fn written(reach: SimpleExpr, repaired: SimpleExpr, written: SimpleExpr) -> StoredGeometry {
        StoredGeometry {
            // clone: the judged geometry and the indexed box read the same two columns
            judged: judged_geometry(repaired.clone(), written.clone()),
            indexed: reach_or_geometry(reach, [repaired, written]),
        }
    }

    /// A geometry stored already valid beside its reach: it judges the
    /// geometry and keys `COALESCE(reach, geometry)`.
    #[must_use]
    pub fn valid(reach: SimpleExpr, geometry: SimpleExpr) -> StoredGeometry {
        StoredGeometry {
            // clone: the geometry is both judged and the fallback of the box
            judged: geometry.clone(),
            indexed: reach_or_geometry(reach, [geometry]),
        }
    }

    /// The geometry a planar relation judges.
    #[must_use]
    pub fn planar(&self) -> SimpleExpr {
        // clone: each relation operand owns its expression
        self.judged.clone()
    }

    /// The same geometry on the spheroid, which `near` measures on.
    #[must_use]
    pub fn geographic(&self) -> SimpleExpr {
        // clone: each relation operand owns its expression
        cast(self.judged.clone(), PostgresType::Geography)
    }

    /// The box its spatial index keys.
    #[must_use]
    pub fn indexed_box(&self) -> GeometryBox {
        // clone: the box test owns its operand
        GeometryBox(self.indexed.clone())
    }
}

/// A box a geo-query may test: one that covers its geometries as `near`
/// reads them.
#[derive(Debug, Clone)]
pub struct GeometryBox(SimpleExpr);

impl GeometryBox {
    /// The extent of a temporal attribute stream, which the write path grows
    /// from each recorded geometry's reach, else from the geometry.
    #[must_use]
    pub const fn stream_extent(extent: SimpleExpr) -> GeometryBox {
        GeometryBox(extent)
    }
}

/// Whether a box test may be served by the box's spatial index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxIndexing {
    /// The test reads the box as its `GiST` index keys it, so the planner may
    /// drive the read from the index.
    Indexed,
    /// The test compares boxes no geometry index stores. For a probe that
    /// reads one Evolution's rows through its stream index: an indexable test
    /// there lets the planner AND a geometry `GiST` into every probe, while
    /// this one only spares the exact relation of the rows it discards.
    Unindexed,
}

/// A box test every geometry meeting `georel` against `reference` passes, or
/// `None` when the relation admits no box.
///
/// `near` with a `maxDistance` tests the [`NearEnvelope`] around the
/// reference's great-circle bounds, as one or, across the antimeridian, two
/// rectangles. `within`, `contains`, `intersects`, `overlaps` and `equals`
/// each need the two geometries to share a point, so their boxes overlap: the
/// test is the overlap with the reference as the relation reads it.
/// `disjoint` and a `near` with only a `minDistance` match geometries
/// arbitrarily far from the reference.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when the reference
/// geometry has no EWKB spelling the store can bind.
pub fn box_test(georel: &GeoRel, reference: &Geometry, subject: &GeometryBox, indexing: BoxIndexing) -> Result<Option<SimpleExpr>, RenderError> {
    let overlap = match indexing {
        BoxIndexing::Indexed => bounding_boxes_overlap,
        BoxIndexing::Unindexed => unindexed_bounding_boxes_overlap,
    };
    match georel {
        GeoRel::Near {
            max_distance: Some(max_distance),
            ..
        } => Ok(great_circle_bounds(&reference.planar()).and_then(|bounds| {
            NearEnvelope::around(bounds, *max_distance)
                .rects()
                .iter()
                // clone: each rectangle of a split envelope tests the same box
                .map(|rect| overlap(subject.0.clone(), st_make_envelope([rect.min().x, rect.min().y], [rect.max().x, rect.max().y])))
                .reduce(ExprTrait::or)
        })),
        GeoRel::Within | GeoRel::Contains | GeoRel::Intersects | GeoRel::Overlaps | GeoRel::Equals => {
            let encoded = encode(reference.form())?;
            // clone: the test owns its operand
            Ok(Some(overlap(subject.0.clone(), validated_geom_from_ewkb(SeaExpr::val(encoded.to_vec())))))
        }
        GeoRel::Near { max_distance: None, .. } | GeoRel::Disjoint => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::stored_geometry::{BoxIndexing, GeometryBox, StoredGeometry, box_test};
    use andromeda_ngsi_ld_geometry::{
        geo_rel::GeoRel,
        geometry::Geometry,
        great_circle::great_circle_bounds,
        near_envelope::NearEnvelope,
        positive_distance::PositiveDistance,
    };
    use andromeda_postgresql_encoding::ewkb::encode;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr, Value, Values};
    use std::str::FromStr;

    fn point(longitude: f64, latitude: f64) -> Geometry {
        serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [longitude, latitude] })).expect("point parses")
    }

    fn near(metres: Option<&str>, minimum: Option<&str>) -> GeoRel {
        GeoRel::Near {
            max_distance: metres.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
            min_distance: minimum.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
        }
    }

    fn column(name: &str) -> SimpleExpr {
        Expr::col((Alias::new("g"), Alias::new(name)))
    }

    fn instance() -> StoredGeometry {
        StoredGeometry::written(column("value_geometry_reach"), column("value_geometry_valid"), column("value_geometry"))
    }

    fn render(expression: SimpleExpr) -> (String, Values) {
        Query::select().expr(expression).build(PostgresQueryBuilder)
    }

    fn doubles(values: &Values) -> Vec<f64> {
        values
            .0
            .iter()
            .filter_map(|value| if let Value::Double(Some(double)) = value { Some(*double) } else { None })
            .collect()
    }

    #[test]
    fn a_written_geometry_judges_its_repair_and_keys_its_reach_first() {
        let stored = instance();
        assert_eq!(
            render(stored.planar()).0,
            r#"SELECT COALESCE("g"."value_geometry_valid", "g"."value_geometry")"#
        );
        assert_eq!(
            render(stored.geographic()).0,
            r#"SELECT COALESCE("g"."value_geometry_valid", "g"."value_geometry")::geography"#
        );
        let (sql, _) = render(
            box_test(&GeoRel::Within, &point(1.0, 2.0), &stored.indexed_box(), BoxIndexing::Indexed)
                .expect("renders")
                .expect("a box"),
        );
        assert_eq!(
            sql,
            r#"SELECT COALESCE("g"."value_geometry_reach", "g"."value_geometry_valid", "g"."value_geometry") && ST_MakeValid(ST_GeomFromEWKB($1))"#
        );
    }

    #[test]
    fn a_valid_geometry_judges_itself_and_keys_its_reach_first() {
        let stored = StoredGeometry::valid(column("location_reach"), column("location"));
        assert_eq!(render(stored.planar()).0, r#"SELECT "g"."location""#);
        let (sql, _) = render(
            box_test(&near(Some("1000"), None), &point(0.0, 63.43), &stored.indexed_box(), BoxIndexing::Indexed)
                .expect("renders")
                .expect("a box"),
        );
        assert!(
            sql.starts_with(r#"SELECT COALESCE("g"."location_reach", "g"."location") && ST_MakeEnvelope("#),
            "{sql}"
        );
    }

    #[test]
    fn every_area_relation_admits_the_reference_box() {
        for georel in [GeoRel::Within, GeoRel::Contains, GeoRel::Intersects, GeoRel::Overlaps, GeoRel::Equals] {
            let reference = point(1.0, 2.0);
            let (_, values) = render(
                box_test(&georel, &reference, &instance().indexed_box(), BoxIndexing::Indexed)
                    .expect("renders")
                    .expect("an area box"),
            );
            let bytes: Vec<Vec<u8>> = values
                .0
                .iter()
                .filter_map(|value| if let Value::Bytes(Some(bytes)) = value { Some(bytes.clone()) } else { None })
                .collect();
            assert_eq!(bytes, [encode(reference.form()).expect("encodes").to_vec()], "{georel:?}");
        }
    }

    #[test]
    fn disjoint_and_a_minimum_distance_alone_admit_no_box() {
        for georel in [GeoRel::Disjoint, near(None, Some("10"))] {
            assert!(
                box_test(&georel, &point(1.0, 2.0), &instance().indexed_box(), BoxIndexing::Indexed)
                    .expect("renders")
                    .is_none(),
                "{georel:?}"
            );
        }
    }

    #[test]
    fn a_near_box_is_the_near_envelope_of_the_reference() {
        let reference = point(4.5, 52.5);
        let (_, values) = render(
            box_test(&near(Some("2000"), None), &reference, &instance().indexed_box(), BoxIndexing::Indexed)
                .expect("renders")
                .expect("a box"),
        );
        let NearEnvelope::Single(rect) = NearEnvelope::around(
            great_circle_bounds(&reference.planar()).expect("a point has bounds"),
            PositiveDistance::from_str("2000").expect("positive distance"),
        ) else {
            panic!("a reference far from the antimeridian has one rectangle");
        };
        assert_eq!(doubles(&values), [rect.min().x, rect.min().y, rect.max().x, rect.max().y]);
    }

    #[test]
    fn a_near_box_across_the_antimeridian_tests_both_halves() {
        let (sql, values) = render(
            box_test(
                &near(Some("300"), None),
                &point(179.999, -17.0),
                &instance().indexed_box(),
                BoxIndexing::Indexed,
            )
            .expect("renders")
            .expect("a box"),
        );
        assert_eq!(sql.matches(r#""g"."value_geometry") && ST_MakeEnvelope("#).count(), 2, "{sql}");
        assert!(sql.contains(" OR "), "{sql}");
        let doubles = doubles(&values);
        assert!(doubles.contains(&180.0) && doubles.contains(&-180.0), "{doubles:?}");
    }

    /// `near` is measured on the spheroid, where 5 000 m due north of the
    /// equator is latitude 0.045 218 4, beyond the 0.044 966 a box grown on
    /// the sphere would reach.
    #[test]
    fn a_near_box_covers_the_spheroidal_reach_of_the_reference() {
        let (_, values) = render(
            box_test(&near(Some("5000"), None), &point(0.0, 0.0), &instance().indexed_box(), BoxIndexing::Indexed)
                .expect("renders")
                .expect("a box"),
        );
        let north = doubles(&values).into_iter().fold(f64::NEG_INFINITY, f64::max);
        assert!(north >= 0.045_218, "the box reaches latitude {north}");
    }

    #[test]
    fn an_unindexed_box_compares_box2d_values() {
        let (sql, _) = render(
            box_test(&near(Some("2000"), None), &point(4.5, 52.5), &instance().indexed_box(), BoxIndexing::Unindexed)
                .expect("renders")
                .expect("a box"),
        );
        assert!(
            sql.starts_with(r#"SELECT COALESCE("g"."value_geometry_reach", "g"."value_geometry_valid", "g"."value_geometry")::box2d && ST_MakeEnvelope("#),
            "{sql}"
        );
        assert!(sql.ends_with(")::box2d"), "{sql}");
    }

    #[test]
    fn a_stream_extent_is_tested_bare() {
        let (sql, _) = render(
            box_test(
                &GeoRel::Intersects,
                &point(1.0, 2.0),
                &GeometryBox::stream_extent(column("value_extent")),
                BoxIndexing::Indexed,
            )
            .expect("renders")
            .expect("a box"),
        );
        assert_eq!(sql, r#"SELECT "g"."value_extent" && ST_MakeValid(ST_GeomFromEWKB($1))"#);
    }
}
