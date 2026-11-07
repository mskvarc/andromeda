//! The two disjoint parts a geoquery splits registration discovery into
//! (clause 5.10.2.4).
//!
//! A registration matches a geoquery when the geometry the query names
//! possibly contains a location the query would match, and also when the
//! registration declares no such geometry, since it may still serve the area.
//! As one disjunction the two reach no index together: the spatial `GiST`
//! indexes hold only declared geometries, and a row without one is found only
//! by reading every row. Discovery therefore reads the registrations without
//! the geometry and those whose geometry meets the reduced relation as two
//! parts, each through its own partial index, and merges them. The parts are
//! disjoint, so a registration is never counted twice.

use andromeda_ngsi_ld_query::geo::{query::GeoQuery, registration_area::RegistrationGeometry};
use andromeda_postgresql_query::{error::RenderError, geo_predicate::registration_area, stored_geometry::StoredGeometry};
use andromeda_postgresql_schema::csr::Csr;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait};

/// The registrations a geoquery keeps, as two disjoint conditions over the
/// registration's own columns.
pub struct GeometryParts {
    /// The registrations that declare no geometry of the kind the query names.
    pub absent: Condition,
    /// The registrations whose geometry meets the relation the query reduces
    /// to, behind the box test that reaches the spatial index.
    pub present: Condition,
}

/// The two parts `geo` splits discovery into, or `None` when its reduction
/// constrains no geometry and every registration is kept.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when the reference
/// geometry has no EWKB spelling the store can bind.
pub fn geometry_parts(geo: &GeoQuery) -> Result<Option<GeometryParts>, RenderError> {
    let (written, repaired, reach) = registration_geometry_columns(geo);
    // Clause 4.7.2 stores an invalid geometry as written; the relation judges
    // its repaired form, and the box test reads the box the index keys.
    let stored = StoredGeometry::written(SeaExpr::col(reach), SeaExpr::col(repaired), SeaExpr::col(written));
    let Some(area) = registration_area(geo, &stored)? else {
        return Ok(None);
    };
    Ok(Some(GeometryParts {
        absent: Condition::all().add(SeaExpr::col(written).is_null()),
        // The partial indexes are restricted by the written column.
        present: Condition::all().add(SeaExpr::col(written).is_not_null()).add(area),
    }))
}

/// The columns holding the registration geometry a geoquery is matched
/// against (Table 5.2.9-1): the geometry as written, its repaired form and
/// its great-circle reach.
fn registration_geometry_columns(geo: &GeoQuery) -> (Csr, Csr, Csr) {
    match RegistrationGeometry::of(geo) {
        RegistrationGeometry::Location => (Csr::Location, Csr::LocationValid, Csr::LocationReach),
        RegistrationGeometry::ObservationSpace => (Csr::ObservationSpace, Csr::ObservationSpaceValid, Csr::ObservationSpaceReach),
        RegistrationGeometry::OperationSpace => (Csr::OperationSpace, Csr::OperationSpaceValid, Csr::OperationSpaceReach),
    }
}

#[cfg(test)]
mod tests {
    use crate::csr::geometry_parts::{GeometryParts, geometry_parts};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_query::geo::query::GeoQuery;
    use andromeda_postgresql_schema::csr::Csr;
    use sea_query::{Condition, PostgresQueryBuilder, Query};
    use std::str::FromStr;

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn query(georel: GeoRel, geoproperty: &str, coordinates: &serde_json::Value) -> GeoQuery {
        GeoQuery {
            georel,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": coordinates })).expect("point parses"),
            geoproperty: geoproperty.parse().expect("valid attribute name"),
        }
    }

    fn near(max_distance: Option<&str>, min_distance: Option<&str>) -> GeoRel {
        GeoRel::Near {
            max_distance: max_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
            min_distance: min_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
        }
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .column(Csr::Id)
            .from(Csr::Table)
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    fn parts(geo: &GeoQuery) -> (String, String) {
        let GeometryParts { absent, present } = geometry_parts(geo).expect("renders").expect("constrains the geometry");
        (render(absent), render(present))
    }

    /// Clause 5.10.2.4: a registration that declares no location may still
    /// serve the area, so it forms a part of its own.
    #[test]
    fn a_registration_without_the_geometry_forms_its_own_part() {
        let (absent, present) = parts(&query(GeoRel::Within, LOCATION, &serde_json::json!([1.0, 2.0])));
        assert!(absent.ends_with(r#"WHERE "location" IS NULL"#), "{absent}");
        assert!(present.contains(r#""location" IS NOT NULL AND "#), "{present}");
        assert!(
            present.contains(r#"))) AND _ST_Intersects(COALESCE("location_valid", "location")"#),
            "{present}"
        );
        assert!(!present.contains(" OR "), "the parts are separate statements, not alternatives: {present}");
    }

    #[test]
    fn each_space_is_split_on_its_own_column() {
        for (iri, column) in [
            ("https://uri.etsi.org/ngsi-ld/observationSpace", "observation_space"),
            ("https://uri.etsi.org/ngsi-ld/operationSpace", "operation_space"),
            ("https://example.com/customGeo", "location"),
        ] {
            let (absent, present) = parts(&query(GeoRel::Intersects, iri, &serde_json::json!([1.0, 2.0])));
            assert!(absent.ends_with(&format!(r#"WHERE "{column}" IS NULL"#)), "{iri}: {absent}");
            assert!(present.contains(&format!(r#""{column}" IS NOT NULL"#)), "{iri}: {present}");
            assert!(present.contains(&format!(r#"COALESCE("{column}_valid", "{column}")"#)), "{iri}: {present}");
        }
    }

    /// The geometry a registration declares bounds where its Entities are, so
    /// it is tested with the weakest relation it has to meet to possibly
    /// contain a location the geoquery matches.
    #[test]
    fn every_georel_is_tested_through_its_reduction() {
        let cases = [
            (GeoRel::Within, " _ST_Intersects("),
            (GeoRel::Contains, " _ST_Contains("),
            (GeoRel::Intersects, " _ST_Intersects("),
            (GeoRel::Disjoint, "NOT _ST_Within("),
            (GeoRel::Equals, " _ST_Contains("),
            (GeoRel::Overlaps, " _ST_Intersects("),
            (near(Some("500"), Some("100")), " _ST_DWithin("),
        ];
        for (georel, function) in cases {
            let geo = query(georel, LOCATION, &serde_json::json!([1.0, 2.0]));
            let (_, present) = parts(&geo);
            assert!(present.contains(function), "{:?}: {present}", geo.georel);
        }
    }

    /// A registration's area reaches arbitrarily far from its nearest point, so
    /// a minimum distance excludes none, and only a maximum distance bounds it.
    #[test]
    fn a_near_relation_is_tested_against_its_maximum_distance_only() {
        let (_, bounded) = parts(&query(near(Some("500"), Some("100")), LOCATION, &serde_json::json!([13.4, 52.5])));
        assert!(!bounded.contains("ST_Distance"), "{bounded}");
        assert!(
            geometry_parts(&query(near(None, Some("100")), LOCATION, &serde_json::json!([13.4, 52.5])))
                .expect("renders")
                .is_none(),
            "a minimum distance alone keeps every registration"
        );
    }

    /// `near` measures the column cast to `geography`, which reaches no index,
    /// so the box the geometry `GiST` index keys, great-circle reach first,
    /// prefilters it.
    #[test]
    fn a_near_relation_is_prefiltered_by_the_reach_box_the_spatial_index_keys() {
        let (_, present) = parts(&query(near(Some("500"), None), LOCATION, &serde_json::json!([13.4, 52.5])));
        assert!(
            present.contains(r#"COALESCE("location_reach", "location_valid", "location") && ST_MakeEnvelope("#),
            "{present}"
        );
        assert!(present.contains(r#"COALESCE("location_valid", "location")::geography"#), "{present}");
    }

    /// The reduced relation reads the judged geometry, which the index does
    /// not key, so its box is tested explicitly; `disjoint` reduces to a
    /// negation, which admits none.
    #[test]
    fn a_topological_relation_is_prefiltered_by_its_reduced_relations_box() {
        let (_, within) = parts(&query(GeoRel::Within, LOCATION, &serde_json::json!([13.4, 52.5])));
        assert!(
            within.contains(r#"COALESCE("location_reach", "location_valid", "location") && ST_MakeValid(ST_GeomFromEWKB("#),
            "{within}"
        );
        let (_, disjoint) = parts(&query(GeoRel::Disjoint, LOCATION, &serde_json::json!([13.4, 52.5])));
        assert!(!disjoint.contains("&&"), "{disjoint}");
    }

    /// Clause 4.10 defines every `georel` on the horizontal plane, so an
    /// altitude on the reference leaves the statement unchanged.
    #[test]
    fn a_three_dimensional_reference_renders_the_same_parts_as_a_two_dimensional_one() {
        for georel in [GeoRel::Within, GeoRel::Contains, GeoRel::Disjoint, near(Some("500"), Some("10"))] {
            let flat = geometry_parts(&query(georel.clone(), LOCATION, &serde_json::json!([1.0, 2.0])))
                .expect("renders")
                .expect("constrains the geometry");
            let elevated = geometry_parts(&query(georel, LOCATION, &serde_json::json!([1.0, 2.0, 3.0])))
                .expect("renders")
                .expect("constrains the geometry");
            let shape = |condition: Condition| {
                Query::select()
                    .column(Csr::Id)
                    .from(Csr::Table)
                    .cond_where(condition)
                    .build(PostgresQueryBuilder)
                    .0
            };
            pretty_assertions::assert_eq!(shape(elevated.present), shape(flat.present));
        }
    }
}
