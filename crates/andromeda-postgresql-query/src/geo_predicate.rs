//! SQL rendering of a geo query (clause 4.10).
//!
//! One reading of the seven `georel` relations, for every relation the broker
//! filters on. What varies between callers is where the subject geometry is
//! read from and what a row without one means; the relations themselves do not
//! vary, and before this module they were written three times.
//!
//! The default GeoProperty (`location`) is stored in dedicated PostGIS columns,
//! so it is read directly. Every other GeoProperty lives inside the JSONB
//! payload, and its GeoJSON is decoded column-side — the documented exception to
//! binding geometries as EWKB.

use crate::{
    error::RenderError,
    jsonb::{attribute_value_jsonpath, escape_jsonpath},
    postgis::{
        st_contains,
        st_disjoint,
        st_dwithin,
        st_equals,
        st_intersects,
        st_overlaps,
        st_within,
        validated_geom_from_ewkb,
        validated_geom_from_geojson_sql,
    },
};
use andromeda_ngsi_ld_geometry::geo_rel::GeoRel;
use andromeda_ngsi_ld_query::{error::QueryError, geo::query::GeoQuery};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{EXPIRES_AT, LOCATION, VALUE};
use andromeda_postgresql_encoding::ewkb::encode;
use sea_query::{Condition, Expr as SeaExpr, SimpleExpr};

/// Where a geo predicate reads the subject geometry from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometrySource<'a> {
    /// The dedicated PostGIS columns beside an entity payload column.
    ///
    /// A custom GeoProperty is decoded out of the payload; the default
    /// `location` reads `location` / `location_geo` and is additionally guarded
    /// against expiry, which only the payload records.
    EntityPayload {
        /// The payload column, optionally table-qualified as `alias.payload`.
        payload_column: &'a str,
    },
    /// A plain PostGIS `geometry` column, as a Context Source Registration
    /// carries. Registrations have no payload and no GeoProperty of their own,
    /// so `geoproperty` does not apply.
    Column {
        /// Unqualified column name.
        name: &'a str,
    },
}

/// What a row that carries no geometry at all means for the filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullGeometryPolicy {
    /// A missing geometry matches.
    ///
    /// Clause 5.10.2: a registration that declares no location may still serve
    /// the area a discovery query asks about, so it is not excluded by one.
    Match,
    /// A missing geometry does not match, which is what an entity query means.
    Exclude,
}

/// Render a geo query.
///
/// The reference geometry is encoded here rather than by each caller: it is
/// already parsed and validated on [`GeoQuery`], so every target binds the same
/// bytes for the same query by construction.
pub fn condition(geo: &GeoQuery, source: GeometrySource<'_>, null_policy: NullGeometryPolicy) -> Result<Condition, RenderError> {
    let subject = subject_geometry(geo, source);
    let reference = encode(&geo.geometry)?;
    let reference_geom = validated_geom_from_ewkb(SeaExpr::val(reference.to_vec()).into());

    let spatial = match &geo.georel {
        GeoRel::Within => st_within(subject.planar(), reference_geom),
        GeoRel::Contains => st_contains(subject.planar(), reference_geom),
        GeoRel::Intersects => st_intersects(subject.planar(), reference_geom),
        GeoRel::Disjoint => st_disjoint(subject.planar(), reference_geom),
        GeoRel::Equals => st_equals(subject.planar(), reference_geom),
        GeoRel::Overlaps => st_overlaps(subject.planar(), reference_geom),
        // `near` measures on the spheroid, so both operands become `geography`.
        GeoRel::Near { max_distance, min_distance } => {
            let reference_geography = SeaExpr::cust_with_exprs("ST_MakeValid(ST_GeomFromEWKB($1))::geography", [SeaExpr::val(reference.to_vec()).into()]);
            near_expression(subject.geographic(), reference_geography, *max_distance, *min_distance)?
        }
    };

    let mut condition = match null_policy {
        NullGeometryPolicy::Match => Condition::any().add(SeaExpr::cust(format!("{} IS NULL", subject.planar_sql))).add(spatial),
        NullGeometryPolicy::Exclude => Condition::all().add(spatial),
    };

    if let Some(guard) = subject.expiry_guard {
        condition = Condition::all().add(condition).add(SeaExpr::cust(guard));
    }

    Ok(condition)
}

/// The SQL a geo predicate reads its subject geometry through.
struct SubjectGeometry {
    /// Planar `geometry`, for the topological relations.
    planar_sql: String,
    /// Spheroidal `geography`, for `near`.
    geographic_sql: String,
    /// Expiry guard to `AND` onto the predicate, where one applies.
    expiry_guard: Option<String>,
}

impl SubjectGeometry {
    /// The planar operand as a sea-query expression.
    fn planar(&self) -> SimpleExpr {
        SeaExpr::cust(&self.planar_sql)
    }

    /// The geographic operand as a sea-query expression.
    fn geographic(&self) -> SimpleExpr {
        SeaExpr::cust(&self.geographic_sql)
    }
}

/// Work out the two operand renderings and the expiry guard for a source.
fn subject_geometry(geo: &GeoQuery, source: GeometrySource<'_>) -> SubjectGeometry {
    match source {
        GeometrySource::Column { name } => SubjectGeometry {
            planar_sql: format!("\"{name}\""),
            geographic_sql: format!("\"{name}\"::geography"),
            expiry_guard: None,
        },
        GeometrySource::EntityPayload { payload_column } => {
            if geo.geoproperty_iri == LOCATION.as_str() {
                SubjectGeometry {
                    planar_sql: aliased_column(payload_column, "location"),
                    geographic_sql: aliased_column(payload_column, "location_geo"),
                    expiry_guard: Some(location_expiry_guard(&quote_col_ref(payload_column))),
                }
            } else {
                // JSONB column-side extraction: `ST_GeomFromGeoJSON` on the
                // stored GeoJSON object is the documented exception to
                // WKB-only binds.
                let planar_sql = custom_geoproperty_sql(&quote_col_ref(payload_column), geo.geoproperty_iri.as_str());
                SubjectGeometry {
                    geographic_sql: format!("{planar_sql}::geography"),
                    planar_sql,
                    // Only the default GeoProperty has an expiry the broker
                    // tracks; a custom one is an ordinary attribute.
                    expiry_guard: None,
                }
            }
        }
    }
}

/// Decode a custom GeoProperty's GeoJSON out of the payload.
fn custom_geoproperty_sql(quoted_payload_col: &str, geoproperty_iri: &str) -> String {
    let jsonpath = attribute_value_jsonpath(geoproperty_iri, &[], &[], VALUE.as_str());
    validated_geom_from_geojson_sql(&format!(
        "jsonb_path_query_first({}, '{}')::text",
        quoted_payload_col,
        escape_jsonpath(&jsonpath)
    ))
}

/// Combine the `maxDistance` and `minDistance` halves of a `near` relation.
///
/// Clause 4.10 requires at least one of the two.
fn near_expression(geo_col: SimpleExpr, ref_geom: SimpleExpr, max_distance: Option<f64>, min_distance: Option<f64>) -> Result<SimpleExpr, RenderError> {
    let mut parts = Vec::new();
    if let Some(max) = max_distance {
        parts.push(st_dwithin(geo_col.clone(), ref_geom.clone(), max)); // clone: both halves read the same two operands
    }
    if let Some(min) = min_distance {
        // The minimum distance is request-supplied, so it binds as a parameter
        // rather than being interpolated into the statement text.
        parts.push(SeaExpr::cust_with_exprs(
            "ST_Distance($1, $2) >= $3",
            [geo_col, ref_geom, SeaExpr::val(min).into()],
        ));
    }
    // `usize` is unbounded, so a catch-all is the only way to close this match;
    // clause 4.10 admits at most the two halves, so it is reached with two.
    match parts.len() {
        0 => Err(RenderError::Query(QueryError::NearWithoutDistance)),
        1 => Ok(parts.remove(0)),
        _ => {
            let first = parts.remove(0);
            let second = parts.remove(0);
            Ok(SeaExpr::cust_with_exprs("($1 AND $2)", [first, second]))
        }
    }
}

/// Exclude entities whose `location` attribute has expired.
///
/// The dedicated PostGIS columns carry no expiry, so the guard reads the
/// payload instance list.
fn location_expiry_guard(quoted_payload_col: &str) -> String {
    let location_iri = LOCATION.as_str();
    let expires_at_iri = EXPIRES_AT.as_str();
    format!(
        "EXISTS (\
            SELECT 1 FROM jsonb_array_elements({}->0->'{}') AS inst(elem) \
            WHERE (inst.elem #>> '{{{},0,@value}}') IS NULL \
               OR (inst.elem #>> '{{{},0,@value}}')::timestamptz > NOW()\
        )",
        quoted_payload_col, location_iri, expires_at_iri, expires_at_iri
    )
}

/// Name a sibling column of a payload reference, preserving its table alias.
///
/// `e.payload` and `location` become `"e"."location"`; a bare `payload` becomes
/// `"location"`.
fn aliased_column(payload_col: &str, column: &str) -> String {
    match payload_col.strip_suffix(".payload") {
        Some(prefix) => format!("\"{}\".\"{}\"", prefix, column),
        None => format!("\"{}\"", column),
    }
}

/// Properly quotes a table.column reference for PostgreSQL.
///
/// Input like `e.payload` becomes `"e"."payload"`.
/// Input without a dot like `payload` becomes `"payload"`.
fn quote_col_ref(col_ref: &str) -> String {
    match col_ref.split_once('.') {
        Some((table, col)) => format!("\"{}\".\"{}\"", table, col),
        None => format!("\"{}\"", col_ref),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Entities;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use iri_rs::IriBuf;
    use sea_query::{PostgresQueryBuilder, Query};

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn point_query(georel: GeoRel, geoproperty_iri: &str) -> GeoQuery {
        GeoQuery {
            georel,
            geometry: Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] })).expect("point parses"),
            geoproperty_iri: iri(geoproperty_iri),
        }
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    fn entity_source() -> GeometrySource<'static> {
        GeometrySource::EntityPayload { payload_column: "payload" }
    }

    #[test]
    fn a_table_qualified_reference_is_quoted_in_both_halves() {
        assert_eq!(quote_col_ref("e.payload"), "\"e\".\"payload\"");
        assert_eq!(quote_col_ref("payload"), "\"payload\"");
    }

    #[test]
    fn a_sibling_column_keeps_the_payload_alias() {
        assert_eq!(aliased_column("e.payload", "location_geo"), "\"e\".\"location_geo\"");
        assert_eq!(aliased_column("payload", "location_geo"), "\"location_geo\"");
    }

    #[test]
    fn near_without_either_distance_is_rejected() {
        let geo = point_query(
            GeoRel::Near {
                max_distance: None,
                min_distance: None,
            },
            LOCATION,
        );
        let error = condition(&geo, entity_source(), NullGeometryPolicy::Exclude).expect_err("must reject");
        assert!(matches!(error, RenderError::Query(QueryError::NearWithoutDistance)), "got {error:?}");
    }

    #[test]
    fn near_with_both_distances_renders_a_conjunction() {
        let geo = point_query(
            GeoRel::Near {
                max_distance: Some(1000.0),
                min_distance: Some(10.0),
            },
            LOCATION,
        );
        let sql = render(condition(&geo, entity_source(), NullGeometryPolicy::Exclude).unwrap());
        assert!(sql.contains("ST_DWithin"), "{sql}");
        assert!(sql.contains("ST_Distance"), "{sql}");
    }

    #[test]
    fn a_payload_alias_reaches_both_the_geometry_column_and_the_expiry_guard() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let source = GeometrySource::EntityPayload { payload_column: "e.payload" };
        let sql = render(condition(&geo, source, NullGeometryPolicy::Exclude).unwrap());
        assert!(sql.contains("\"e\".\"location\""), "{sql}");
        assert!(sql.contains("\"e\".\"payload\""), "expiry guard reads the aliased payload: {sql}");
    }

    #[test]
    fn a_custom_geoproperty_decodes_geojson_column_side() {
        let geo = point_query(GeoRel::Within, "https://example.com/customGeo");
        let sql = render(condition(&geo, entity_source(), NullGeometryPolicy::Exclude).unwrap());
        assert!(sql.contains("ST_GeomFromGeoJSON"), "{sql}");
        assert!(sql.contains("ST_GeomFromEWKB"), "reference side still binds EWKB: {sql}");
        assert!(!sql.contains("expiresAt"), "the expiry guard is specific to the default location: {sql}");
    }

    // -- null-geometry policy (clause 5.10.2) --------------------------------

    #[test]
    fn a_registration_without_a_location_matches_a_geo_filter() {
        // Clause 5.10.2: a registration that declares no location may still
        // serve the area, so it is not excluded by a geo filter.
        let geo = point_query(GeoRel::Within, LOCATION);
        let source = GeometrySource::Column { name: "location" };
        let sql = render(condition(&geo, source, NullGeometryPolicy::Match).unwrap());
        assert!(sql.contains("\"location\" IS NULL"), "{sql}");
        assert!(sql.contains("ST_Within"), "{sql}");
        assert!(sql.contains(" OR "), "the two are alternatives: {sql}");
    }

    #[test]
    fn an_entity_without_a_location_does_not_match_a_geo_filter() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let sql = render(condition(&geo, entity_source(), NullGeometryPolicy::Exclude).unwrap());
        // The expiry guard has an `IS NULL` of its own, so the assertion names
        // the geometry column rather than the operator.
        assert!(!sql.contains("\"location\" IS NULL"), "an entity query admits no null geometry: {sql}");
    }

    // -- one reading of clause 4.10 ------------------------------------------

    /// Every relation renders through the same seven arms whatever the source,
    /// so the PostGIS function each one picks cannot drift between callers.
    #[test]
    fn every_georel_renders_the_same_postgis_function_for_every_source() {
        let cases = [
            (GeoRel::Within, "ST_Within"),
            (GeoRel::Contains, "ST_Contains"),
            (GeoRel::Intersects, "ST_Intersects"),
            (GeoRel::Disjoint, "ST_Disjoint"),
            (GeoRel::Equals, "ST_Equals"),
            (GeoRel::Overlaps, "ST_Overlaps"),
            (
                GeoRel::Near {
                    max_distance: Some(500.0),
                    min_distance: None,
                },
                "ST_DWithin",
            ),
        ];
        for (georel, function) in cases {
            let geo = point_query(georel.clone(), LOCATION); // clone: the relation is asserted on again below
            let entity_sql = render(condition(&geo, entity_source(), NullGeometryPolicy::Exclude).unwrap());
            let registration_sql = render(condition(&geo, GeometrySource::Column { name: "location" }, NullGeometryPolicy::Match).unwrap());
            assert!(entity_sql.contains(function), "{georel:?} on an entity: {entity_sql}");
            assert!(registration_sql.contains(function), "{georel:?} on a registration: {registration_sql}");
        }
    }

    /// The entity and temporal targets bind the identical reference geometry;
    /// only the relation they are attached to differs.
    #[test]
    fn entity_and_temporal_render_the_same_geo_predicate() {
        let geo = point_query(
            GeoRel::Near {
                max_distance: Some(500.0),
                min_distance: None,
            },
            LOCATION,
        );
        let entity_sql = render(condition(&geo, entity_source(), NullGeometryPolicy::Exclude).unwrap());
        let temporal_source = GeometrySource::EntityPayload { payload_column: "e.payload" };
        let temporal_sql = render(condition(&geo, temporal_source, NullGeometryPolicy::Exclude).unwrap());
        assert!(entity_sql.contains("ST_DWithin"), "{entity_sql}");
        assert!(temporal_sql.contains("ST_DWithin"), "{temporal_sql}");
        // The only difference is the table qualification of the geometry column.
        assert_eq!(
            entity_sql
                .replace("\"location_geo\"", "\"e\".\"location_geo\"")
                .replace("\"payload\"", "\"e\".\"payload\""),
            temporal_sql
        );
    }
}
