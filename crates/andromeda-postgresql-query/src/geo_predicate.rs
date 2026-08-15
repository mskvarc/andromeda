//! SQL rendering of a geo query (clause 4.10).
//!
//! One reading of the seven `georel` relations, for every relation the broker
//! filters on. What varies between callers is where the subject geometry is
//! read from and what a row without one means; the relations themselves do not
//! vary, and before this module they were written three times.
//!
//! Clause 4.10 lets `geoproperty` name any GeoProperty of the entity, defaulting
//! to `location`. The default one has dedicated PostGIS columns on `entities` and
//! is read directly. Every other GeoProperty — `observationSpace` and
//! `operationSpace` as much as a custom one — is read from the
//! `current_attribute_values` projection, which holds one `value_geo` row per attribute
//! instance keyed by the `attribute_names` dictionary. Reading the instance rows
//! rather than a single column is also what gives clause 4.10 its multi-instance
//! rule: where a GeoProperty has several `datasetId` instances, one of them
//! meeting the restriction is enough.

use crate::{
    attribute_instance_correlation::{CorrelatedEntityRow, existence_marker, geometry_instances_of, instance_geometry_column},
    error::RenderError,
    q_predicate::attribute_names::ResolvedAttributeNames,
};
use andromeda_ngsi_ld_geometry::geo_rel::GeoRel;
use andromeda_ngsi_ld_query::{error::QueryError, geo::query::GeoQuery};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{EXPIRES_AT, LOCATION};
use andromeda_postgresql_encoding::ewkb::encode;
use andromeda_postgresql_seaquery::{
    cast::{cast, cast_compound},
    clock::now,
    jsonb::jsonb_array_elements,
    literal::{false_literal, int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    postgis::{st_contains, st_disjoint, st_distance, st_dwithin, st_equals, st_intersects, st_overlaps, st_within, validated_geom_from_ewkb},
};
use sea_query::{Alias, Cond, Condition, Expr as SeaExpr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// What a geo predicate is filtering, which fixes both where it reads the
/// subject geometry and what a row without one means.
///
/// The two questions are one choice rather than two, because no caller mixes
/// them: an entity query excludes a row that conveys no geometry (clause 4.10),
/// while a registration discovery keeps one (clause 5.10.2). Carrying them
/// separately made "an entity whose missing GeoProperty matches" expressible,
/// and nothing could produce it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometrySource<'a> {
    /// A row of the `entities` table, addressed through the outer row a
    /// correlated subquery binds to.
    ///
    /// The default `location` reads the `location` / `location_geo` columns and
    /// is additionally guarded against expiry, which only the payload records.
    /// Any other GeoProperty is reached through its `current_attribute_values`
    /// instance rows, correlated back to this row's surrogate key.
    ///
    /// Clause 4.10: an Entity that conveys no instance of the target
    /// GeoProperty is non-matching.
    Entity {
        /// The `entities` row the predicate filters, and that any correlated
        /// instance subquery binds back to.
        row: CorrelatedEntityRow<'a>,
    },
    /// A plain PostGIS `geometry` column, as a Context Source Registration
    /// carries. Registrations have no payload and no GeoProperty of their own,
    /// so `geoproperty` does not apply.
    ///
    /// Clause 5.10.2: a registration that declares no location may still serve
    /// the area a discovery query asks about, so a null column matches.
    Registration {
        /// Unqualified column name.
        column: &'a str,
    },
}

/// Render a geo query.
///
/// The reference geometry is encoded here rather than by each caller: it is
/// already parsed and validated on [`GeoQuery`], so every target binds the same
/// bytes for the same query by construction.
pub fn condition(geo: &GeoQuery, source: GeometrySource<'_>, attribute_names: &ResolvedAttributeNames) -> Result<Condition, RenderError> {
    let Some(subject) = subject_geometry(geo, source, attribute_names) else {
        // Clause 4.10: an Entity that does not convey the target GeoProperty is
        // non-matching. An IRI with no `attribute_names` row is conveyed by no
        // Entity at all, so the predicate is constant. A registration reaches a
        // named column that always exists, so it never lands here.
        return Ok(Condition::all().add(false_literal()));
    };
    let reference = encode(&geo.geometry)?;
    // Built fresh per relation so the EWKB it binds is not shared as an owned
    // value between the two operands a `near` relation carries.
    let reference_geom = || validated_geom_from_ewkb(SeaExpr::val(reference.to_vec()));
    let SubjectGeometry { planar, geographic, envelope } = subject;

    let spatial = match &geo.georel {
        GeoRel::Within => st_within(planar.clone(), reference_geom()), // clone: a registration also reads the planar column for its null guard
        GeoRel::Contains => st_contains(planar.clone(), reference_geom()),
        GeoRel::Intersects => st_intersects(planar.clone(), reference_geom()),
        GeoRel::Disjoint => st_disjoint(planar.clone(), reference_geom()),
        GeoRel::Equals => st_equals(planar.clone(), reference_geom()),
        GeoRel::Overlaps => st_overlaps(planar.clone(), reference_geom()),
        // `near` measures on the spheroid, so both operands become `geography`.
        GeoRel::Near { max_distance, min_distance } => {
            let reference_geography = cast(reference_geom(), PostgresType::Geography);
            near_expression(geographic, reference_geography, *max_distance, *min_distance)?
        }
    };

    Ok(match envelope {
        // Clause 5.10.2: a registration that declares no location is not
        // excluded by a geo filter, so the null column is an alternative to the
        // relation rather than a failure of it.
        Envelope::RegistrationColumn => Condition::any().add(SeaExpr::expr(planar).is_null()).add(spatial),
        Envelope::EntityColumn { expiry_guard } => Condition::all().add(spatial).add(expiry_guard),
        // Clause 4.10: it is enough that one instance of the GeoProperty meets
        // the restriction, which is exactly an existential over the instance
        // rows.
        Envelope::Instances { correlation } => {
            let mut probe = *correlation;
            probe.expr(existence_marker()).and_where(spatial);
            Condition::all().add(SeaExpr::exists(probe))
        }
    })
}

/// The two operand expressions a geo predicate reads its subject geometry
/// through, and how the relation reaches the rows carrying it.
struct SubjectGeometry {
    /// Planar `geometry`, for the topological relations.
    planar: SimpleExpr,
    /// Spheroidal `geography`, for `near`.
    geographic: SimpleExpr,
    /// How the rendered relation reaches the rows it is evaluated against.
    envelope: Envelope,
}

/// How a rendered relation reaches the rows carrying the subject geometry, and
/// what a row without one means.
enum Envelope {
    /// A registration's own geometry column, where a null column matches
    /// (clause 5.10.2).
    RegistrationColumn,
    /// The entity's dedicated `location` columns, where a null column does not
    /// match (clause 4.10) and an expired `location` is excluded outright.
    EntityColumn {
        /// Expiry guard to `AND` onto the predicate.
        expiry_guard: SimpleExpr,
    },
    /// The geometry lives in the attribute's instance rows, so the relation is
    /// evaluated inside a correlated `EXISTS`. An entity with no such row
    /// satisfies no existential, which is the clause 4.10 exclusion.
    Instances {
        /// The correlated scan selecting those instance rows, left open so the
        /// relation and the `SELECT 1` marker can be added before it is wrapped
        /// in `EXISTS`. Boxed: a `SelectStatement` dwarfs the other variants.
        correlation: Box<SelectStatement>,
    },
}

/// Work out the two operand expressions and the envelope for a source.
///
/// `None` means the target GeoProperty is one no Entity can convey, because the
/// broker has never recorded an attribute under that IRI.
fn subject_geometry(geo: &GeoQuery, source: GeometrySource<'_>, attribute_names: &ResolvedAttributeNames) -> Option<SubjectGeometry> {
    match source {
        // A registration carries no attributes of its own, so `geoproperty`
        // does not apply and the named column is the whole of the subject.
        GeometrySource::Registration { column } => Some(SubjectGeometry {
            planar: SeaExpr::col(Alias::new(column)),
            geographic: cast(SeaExpr::col(Alias::new(column)), PostgresType::Geography),
            envelope: Envelope::RegistrationColumn,
        }),
        GeometrySource::Entity { row } if geo.geoproperty.as_str() == LOCATION.as_str() => Some(SubjectGeometry {
            planar: row.column(Alias::new("location")),
            geographic: row.column(Alias::new("location_geo")),
            envelope: Envelope::EntityColumn {
                expiry_guard: location_expiry_guard(row.column(Alias::new("payload"))),
            },
        }),
        GeometrySource::Entity { row } => {
            let attribute_id = attribute_names.get(&geo.geoproperty)?;
            Some(SubjectGeometry {
                planar: instance_geometry_column(INSTANCE_ALIAS),
                geographic: cast(instance_geometry_column(INSTANCE_ALIAS), PostgresType::Geography),
                envelope: Envelope::Instances {
                    correlation: Box::new(geometry_instances_of(row, INSTANCE_ALIAS, attribute_id)),
                },
            })
        }
    }
}

/// Alias the correlated instance subquery reads `current_attribute_values`
/// under.
///
/// Distinct enough not to collide with an alias the enclosing statement chose.
const INSTANCE_ALIAS: &str = "geo_instance";

/// Combine the `maxDistance` and `minDistance` halves of a `near` relation.
///
/// Clause 4.10 requires at least one of the two.
fn near_expression(geo_col: SimpleExpr, ref_geom: SimpleExpr, max_distance: Option<f64>, min_distance: Option<f64>) -> Result<SimpleExpr, RenderError> {
    let mut parts = Vec::new();
    if let Some(max) = max_distance {
        parts.push(st_dwithin(geo_col.clone(), ref_geom.clone(), max)); // clone: both halves read the same two operands
    }
    if let Some(min) = min_distance {
        // The minimum distance is request-supplied, so it binds as a parameter.
        parts.push(SeaExpr::expr(st_distance(geo_col, ref_geom)).gte(min));
    }
    // `usize` is unbounded, so a catch-all is the only way to close this match;
    // clause 4.10 admits at most the two halves, so it is reached with two.
    match parts.len() {
        0 => Err(RenderError::Query(QueryError::NearWithoutDistance)),
        1 => Ok(parts.remove(0)),
        _ => {
            let first = parts.remove(0);
            let second = parts.remove(0);
            Ok(first.and(second))
        }
    }
}

/// Exclude entities whose `location` attribute has expired.
///
/// The dedicated PostGIS columns carry no expiry, so the guard reads the
/// payload instance list: each element of the expanded `location` attribute is
/// admitted when it carries no `expiresAt`, or one still in the future
/// (clause 5.6.6).
fn location_expiry_guard(payload: SimpleExpr) -> SimpleExpr {
    let container = payload.json_get(int_literal(0)).json_get(text_literal(LOCATION.as_str()));
    // The `text[]` path reaching an instance's `expiresAt` value: element,
    // first entry, `@value`.
    let expires_path = format!("{{{},0,@value}}", EXPIRES_AT.as_str());
    // The instance element is read twice — once for its presence, once for its
    // value — so each read builds its own extraction.
    let expires_at = || SimpleExpr::from(SeaExpr::col((Alias::new("inst"), Alias::new("value")))).json_path_text(text_literal(&expires_path));

    let mut probe = Query::select();
    probe
        .expr(existence_marker())
        .from_function(jsonb_array_elements(container), Alias::new("inst"))
        .cond_where(
            Cond::any()
                .add(SeaExpr::expr(expires_at()).is_null())
                .add(SeaExpr::expr(cast_compound(expires_at(), PostgresType::Timestamptz)).gt(now())),
        );
    SeaExpr::exists(probe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute_name_id::AttributeNameId;
    use andromeda_ngsi_ld_geometry::{dimension::GeometryDimension, geometry::Geometry};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_postgresql_schema::current::Entities;
    use sea_query::{PostgresQueryBuilder, Query, Value, Values};

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn point_query(georel: GeoRel, geoproperty: &str) -> GeoQuery {
        GeoQuery {
            georel,
            geometry: Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] })).expect("point parses"),
            geoproperty: name(geoproperty),
        }
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .to_string(PostgresQueryBuilder)
    }

    /// The same query with the reference point carrying an altitude.
    fn point_query_3d(georel: GeoRel, geoproperty: &str) -> GeoQuery {
        GeoQuery {
            georel,
            geometry: Geometry::from_geojson_value(&serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0, 3.0] }))
                .expect("point with altitude parses"),
            geoproperty: name(geoproperty),
        }
    }

    fn entity_source() -> GeometrySource<'static> {
        GeometrySource::Entity {
            row: CorrelatedEntityRow::unaliased(),
        }
    }

    /// The registration source every discovery query renders against.
    fn registration_source() -> GeometrySource<'static> {
        GeometrySource::Registration { column: "location" }
    }

    /// A custom GeoProperty IRI and the dictionary key it is taken to resolve
    /// to, for the tests that render its instance subquery.
    const CUSTOM_GEOPROPERTY: &str = "https://example.com/customGeo";
    const CUSTOM_GEOPROPERTY_ID: i32 = 42;

    /// A dictionary in which the default `location` is deliberately absent: it
    /// is read from its own column and must never need resolving.
    fn resolved_custom() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([(name(CUSTOM_GEOPROPERTY), AttributeNameId::from(CUSTOM_GEOPROPERTY_ID))])
    }

    /// A dictionary that knows nothing.
    fn resolved_none() -> ResolvedAttributeNames {
        ResolvedAttributeNames::default()
    }

    /// Render with the reference geometry left as a placeholder, so two
    /// statements can be compared on their shape rather than on the bytes they
    /// bind.
    fn render_parameterised(condition: Condition) -> (String, Values) {
        Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder)
    }

    /// The byte buffers a statement binds, in order.
    fn bound_bytes(values: &Values) -> Vec<Vec<u8>> {
        values
            .0
            .iter()
            .filter_map(|value| match value {
                Value::Bytes(Some(bytes)) => Some(bytes.clone()), // clone: the assertion outlives the borrow of the statement
                _non_geometry @ (Value::Bool(_)
                | Value::TinyInt(_)
                | Value::SmallInt(_)
                | Value::Int(_)
                | Value::BigInt(_)
                | Value::TinyUnsigned(_)
                | Value::SmallUnsigned(_)
                | Value::Unsigned(_)
                | Value::BigUnsigned(_)
                | Value::Float(_)
                | Value::Double(_)
                | Value::String(_)
                | Value::Enum(_)
                | Value::Char(_)
                | Value::Bytes(_)
                | Value::Json(_)
                | Value::ChronoDate(_)
                | Value::ChronoTime(_)
                | Value::ChronoDateTime(_)
                | Value::ChronoDateTimeUtc(_)
                | Value::ChronoDateTimeLocal(_)
                | Value::ChronoDateTimeWithTimeZone(_)
                | Value::Uuid(_)
                | Value::Array(..)) => None,
            })
            .collect()
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
        let error = condition(&geo, entity_source(), &resolved_custom()).expect_err("must reject");
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
        let sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        assert!(sql.contains("ST_DWithin"), "{sql}");
        assert!(sql.contains("ST_Distance"), "{sql}");
    }

    #[test]
    fn a_payload_alias_reaches_both_the_geometry_column_and_the_expiry_guard() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let source = GeometrySource::Entity {
            row: CorrelatedEntityRow::aliased("e"),
        };
        let sql = render(condition(&geo, source, &resolved_custom()).unwrap());
        assert!(sql.contains("\"e\".\"location\""), "{sql}");
        assert!(sql.contains("\"e\".\"payload\""), "expiry guard reads the aliased payload: {sql}");
    }

    /// Clause 4.10: `geoproperty` names any GeoProperty of the entity. Every
    /// one other than the default is stored as a `current_attribute_values` row
    /// rather than a column of `entities`, so the relation is evaluated over
    /// those instance rows, correlated back to the entity being filtered.
    #[test]
    fn a_custom_geoproperty_is_read_from_its_attribute_instance_rows() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        assert!(sql.contains("EXISTS(SELECT 1 FROM \"current_attribute_values\""), "{sql}");
        assert!(sql.contains("\"geo_instance\".\"value_geo\""), "{sql}");
        assert!(
            sql.contains(&format!("\"attribute_name_id\" = {CUSTOM_GEOPROPERTY_ID}")),
            "the dictionary key selects the attribute: {sql}"
        );
        assert!(
            sql.contains("\"geo_instance\".\"entity_key\" = \"entities\".\"entity_key\""),
            "correlated to the filtered row: {sql}"
        );
        assert!(sql.contains("ST_Within"), "{sql}");
        assert!(sql.contains("ST_GeomFromEWKB"), "reference side still binds EWKB: {sql}");
        assert!(
            !sql.contains("ST_GeomFromGeoJSON"),
            "the geometry is stored decoded, not as GeoJSON text: {sql}"
        );
        assert!(!sql.contains("expiresAt"), "the expiry guard is specific to the default location: {sql}");
    }

    /// Clause 4.10: where a GeoProperty has several `datasetId` instances, one
    /// of them meeting the restriction is enough. The predicate is therefore an
    /// existential over the instance rows, with no restriction on `dataset_id`
    /// that could pin it to the default instance — an instance that fails the
    /// relation cannot veto one that passes it.
    #[test]
    fn a_multi_instance_geoproperty_matches_when_any_one_instance_does() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        assert!(
            sql.contains("EXISTS(SELECT 1 FROM \"current_attribute_values\""),
            "an existential, not an aggregate: {sql}"
        );
        assert!(!sql.contains("dataset_id"), "no instance is preferred over another: {sql}");
        assert!(!sql.contains("NOT EXISTS"), "a non-matching instance does not veto a matching one: {sql}");
    }

    /// The two spec-defined GeoProperties of clause 4.7 are not special-cased:
    /// they take the same instance-row route every other named GeoProperty does,
    /// so none of them can be the one that silently reads the wrong column.
    #[test]
    fn a_spec_defined_geoproperty_takes_the_same_route_as_a_custom_one() {
        for iri in ["https://uri.etsi.org/ngsi-ld/observationSpace", "https://uri.etsi.org/ngsi-ld/operationSpace"] {
            let names = ResolvedAttributeNames::from_pairs([(name(iri), AttributeNameId::from(7_i32))]);
            let geo = point_query(GeoRel::Within, iri);
            let sql = render(condition(&geo, entity_source(), &names).unwrap());
            assert!(sql.contains("EXISTS(SELECT 1 FROM \"current_attribute_values\""), "{iri}: {sql}");
            assert!(sql.contains("\"attribute_name_id\" = 7"), "{iri}: {sql}");
        }
    }

    /// The default GeoProperty keeps its dedicated columns, so the common case
    /// still reads an indexed column rather than a correlated subquery.
    #[test]
    fn the_default_geoproperty_reads_its_dedicated_columns_without_a_subquery() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let sql = render(condition(&geo, entity_source(), &resolved_none()).unwrap());
        assert!(sql.contains("\"location\""), "{sql}");
        assert!(
            !sql.contains("current_attribute_values"),
            "the default GeoProperty needs no instance lookup: {sql}"
        );
    }

    /// Clause 4.10: an Entity that does not convey the target GeoProperty is
    /// non-matching. An IRI the dictionary has never seen is conveyed by no
    /// Entity, so the filter is constant `FALSE` rather than an error.
    #[test]
    fn an_unknown_geoproperty_matches_nothing_without_failing() {
        let geo = point_query(GeoRel::Within, "https://example.com/neverRecorded");
        let sql = render(condition(&geo, entity_source(), &resolved_none()).expect("renders"));
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("ST_Within"), "no relation is evaluated at all: {sql}");
    }

    /// `near` reaches the instance geometry as `geography`, exactly as it
    /// reaches the default GeoProperty's dedicated geography column.
    #[test]
    fn a_near_relation_casts_the_instance_geometry_to_geography() {
        let geo = point_query(
            GeoRel::Near {
                max_distance: Some(500.0),
                min_distance: None,
            },
            CUSTOM_GEOPROPERTY,
        );
        let sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        assert!(sql.contains("\"geo_instance\".\"value_geo\"::geography"), "{sql}");
        assert!(sql.contains("ST_DWithin"), "{sql}");
    }

    /// The instance subquery correlates to whichever alias the enclosing
    /// statement joined `entities` under, so the temporal path reaches the same
    /// rows the entity path does.
    #[test]
    fn an_instance_subquery_correlates_through_the_table_alias() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let source = GeometrySource::Entity {
            row: CorrelatedEntityRow::aliased("e"),
        };
        let sql = render(condition(&geo, source, &resolved_custom()).unwrap());
        assert!(sql.contains("\"geo_instance\".\"entity_key\" = \"e\".\"entity_key\""), "{sql}");
    }

    // -- what a missing geometry means, per source (clauses 4.10, 5.10.2) -----

    #[test]
    fn a_registration_without_a_location_matches_a_geo_filter() {
        // Clause 5.10.2: a registration that declares no location may still
        // serve the area, so it is not excluded by a geo filter.
        let geo = point_query(GeoRel::Within, LOCATION);
        let sql = render(condition(&geo, registration_source(), &resolved_custom()).unwrap());
        assert!(sql.contains("\"location\" IS NULL"), "{sql}");
        assert!(sql.contains("ST_Within"), "{sql}");
        assert!(sql.contains(" OR "), "the two are alternatives: {sql}");
    }

    #[test]
    fn an_entity_without_a_location_does_not_match_a_geo_filter() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        // The expiry guard has an `IS NULL` of its own, so the assertion names
        // the geometry column rather than the operator.
        assert!(!sql.contains("\"location\" IS NULL"), "an entity query admits no null geometry: {sql}");
    }

    /// A registration reaches a column that always exists, so it never renders
    /// the constant-`FALSE` an unresolvable GeoProperty gives an entity query:
    /// the two null readings belong to the two sources and cannot be mixed.
    #[test]
    fn a_registration_never_renders_the_entity_exclusion() {
        let geo = point_query(GeoRel::Within, "https://example.com/neverRecorded");
        let sql = render(condition(&geo, registration_source(), &resolved_none()).expect("renders"));
        assert!(!sql.contains("FALSE"), "{sql}");
        assert!(sql.contains("ST_Within"), "{sql}");
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
            let entity_sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
            let registration_sql = render(condition(&geo, registration_source(), &resolved_custom()).unwrap());
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
        let entity_sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        let temporal_source = GeometrySource::Entity {
            row: CorrelatedEntityRow::aliased("e"),
        };
        let temporal_sql = render(condition(&geo, temporal_source, &resolved_custom()).unwrap());
        assert!(entity_sql.contains("ST_DWithin"), "{entity_sql}");
        assert!(temporal_sql.contains("ST_DWithin"), "{temporal_sql}");
        // The only difference is the table qualification of the geometry column.
        assert_eq!(
            entity_sql
                .replace("\"entities\".\"location_geo\"", "\"e\".\"location_geo\"")
                .replace("\"entities\".\"payload\"", "\"e\".\"payload\""),
            temporal_sql
        );
    }

    // -- a third ordinate does not reach the predicate (clause 4.10) ----------

    /// Clause 4.10 defines every `georel` on the horizontal plane, so a
    /// reference geometry that carries an altitude must render the same
    /// statement as the same geometry without one: `ST_Within` and friends and
    /// the `geography` casts behind `near` all ignore Z. Only the bound bytes
    /// may differ.
    #[test]
    fn a_three_dimensional_reference_renders_the_same_sql_as_a_two_dimensional_one() {
        let relations = [
            GeoRel::Within,
            GeoRel::Contains,
            GeoRel::Intersects,
            GeoRel::Disjoint,
            GeoRel::Equals,
            GeoRel::Overlaps,
            GeoRel::Near {
                max_distance: Some(500.0),
                min_distance: Some(10.0),
            },
        ];
        for georel in relations {
            for source in [entity_source(), registration_source()] {
                let flat = point_query(georel.clone(), LOCATION); // clone: the relation is reused for the elevated query
                let elevated = point_query_3d(georel.clone(), LOCATION); // clone: and again on the next source
                let (flat_sql, flat_values) = render_parameterised(condition(&flat, source, &resolved_custom()).expect("flat renders"));
                let (elevated_sql, elevated_values) = render_parameterised(condition(&elevated, source, &resolved_custom()).expect("elevated renders"));

                assert_eq!(
                    elevated_sql, flat_sql,
                    "{georel:?} on {source:?} must not vary with the reference dimensionality"
                );
                assert_ne!(
                    bound_bytes(&elevated_values),
                    bound_bytes(&flat_values),
                    "{georel:?} on {source:?} must still bind the altitude it was given"
                );
            }
        }
    }

    /// The altitude survives as far as the bind: what the statement sends is
    /// XYZ EWKB, even though the predicate around it is the two-dimensional
    /// one clause 4.10 mandates.
    #[test]
    fn a_three_dimensional_reference_binds_as_three_dimensional_ewkb() {
        let geo = point_query_3d(GeoRel::Within, LOCATION);
        let ewkb = encode(&geo.geometry).expect("elevated point encodes");
        let decoded = Geometry::try_from(&ewkb).expect("the bound bytes decode");
        assert_eq!(decoded.dimension(), GeometryDimension::ThreeD);

        let (_, values) = render_parameterised(condition(&geo, entity_source(), &resolved_custom()).expect("renders"));
        assert!(
            bound_bytes(&values).contains(&ewkb.to_vec()),
            "the statement binds exactly the encoded reference"
        );
    }
}
