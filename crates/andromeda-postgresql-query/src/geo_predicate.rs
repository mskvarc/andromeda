//! SQL rendering of a geo query (clause 4.10).
//!
//! One reading of the seven `georel` relations, for every relation the broker
//! filters on. What varies between callers is where the subject geometry is
//! read from and what a row without one means; the relations themselves do not
//! vary.
//!
//! Clause 4.10 lets `geoproperty` name any `GeoProperty` of the entity, defaulting
//! to `location`, and clause 5.7.2.4 counts an entity as matching when any
//! instance of it meets the restriction. Every `GeoProperty` — `observationSpace`
//! and `operationSpace` as much as a custom one — is read from the
//! `current_attribute_values` projection, which holds one `value_geometry` row
//! per attribute instance keyed by the `attribute_names` dictionary, so the
//! multi-instance rule is an existential over those rows. The default
//! `location` additionally has its default instance hoisted into dedicated
//! `PostGIS` columns on `entity_search`; [`crate::location_instances`] reads
//! both.

use crate::{
    attribute_instance_correlation::{existence_marker, geometry_instances_of, instance_geometry_column},
    dictionary::resolved_names::ResolvedNames,
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    location_instances::any_location_instance,
};
use andromeda_ngsi_ld_geometry::{
    geo_rel::GeoRel,
    geometry::Geometry,
    great_circle::great_circle_bounds,
    near_envelope::NearEnvelope,
    positive_distance::PositiveDistance,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    error::QueryError,
    geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_encoding::ewkb::encode;
use andromeda_postgresql_seaquery::{
    cast::cast,
    literal::false_literal,
    pg_type::PostgresType,
    postgis::{
        bounding_boxes_overlap,
        st_contains,
        st_disjoint,
        st_distance,
        st_dwithin,
        st_equals,
        st_intersects,
        st_make_envelope,
        st_overlaps,
        st_within,
        validated_geom_from_ewkb,
    },
};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, SelectStatement, SimpleExpr};

/// What a geo predicate is filtering, which fixes both where it reads the
/// subject geometry and what a row without one means.
///
/// The two questions are one choice rather than two, because no caller mixes
/// them: an entity query excludes a row that conveys no geometry (clause 4.10),
/// while a registration discovery keeps one (clause 5.10.2). Keeping them in
/// one type makes "an entity whose missing `GeoProperty` matches"
/// unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GeometrySource<'a> {
    /// One entity's current-state rows, addressed through the names the
    /// enclosing statement knows them by.
    ///
    /// A `GeoProperty` is reached through its `current_attribute_values`
    /// instance rows, correlated back to the identity row's surrogate key. The
    /// default `location` also reads the hoisted default instance on the search
    /// row, narrowed by what an in-memory index established about it. Every
    /// instance is excluded once its own clause 4.22 `expiresAt` has passed.
    ///
    /// Clause 4.10: an Entity that conveys no instance of the target
    /// `GeoProperty` is non-matching.
    Entity {
        /// The entity rows the predicate filters, and that any correlated
        /// instance subquery binds back to.
        rows: &'a EntityRowRefs,
        /// What an in-memory spatial index established about the default
        /// `location` instances; ignored for any other `GeoProperty`.
        default_location: &'a DefaultLocationNarrowing,
    },
    /// A plain `PostGIS` `geometry` column, as a Context Source Registration
    /// carries. Registrations have no payload and no `GeoProperty` of their own,
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
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when the query's
/// reference geometry has no EWKB spelling the store can bind, and
/// [`RenderError::Query`] carrying [`QueryError::NearWithoutDistance`] when
/// `georel=near` names neither `maxDistance` nor `minDistance`, which clause
/// 4.10 requires at least one of.
pub fn condition(geo: &GeoQuery, source: GeometrySource<'_>, attribute_names: &ResolvedNames<AttributeName>) -> Result<Condition, RenderError> {
    if let GeometrySource::Entity { rows, default_location } = source
        && geo.geoproperty.as_str() == LOCATION.as_str()
    {
        return any_location_instance(geo, rows, default_location, attribute_names.get(&geo.geoproperty));
    }
    let Some(subject) = subject_geometry(geo, source, attribute_names) else {
        // Clause 4.10: an Entity that does not convey the target GeoProperty is
        // non-matching. An IRI with no `attribute_names` row is conveyed by no
        // Entity at all, so the predicate is constant. A registration reaches a
        // named column that always exists, so it never lands here.
        return Ok(Condition::all().add(false_literal()));
    };
    let SubjectGeometry { planar, geographic, envelope } = subject;
    // clone: a registration also reads the planar column for its null guard, and
    // an instance scan for its bounding-box prefilter.
    let spatial = relation_expression(&geo.georel, planar.clone(), geographic, &geo.geometry)?;

    Ok(match envelope {
        // Clause 5.10.2: a registration that declares no location is not
        // excluded by a geo filter, so the null column is an alternative to the
        // relation rather than a failure of it.
        Envelope::RegistrationColumn => Condition::any().add(SeaExpr::expr(planar).is_null()).add(spatial),
        // Clause 5.7.2.4: it is enough that one instance of the GeoProperty
        // meets the restriction, which is exactly an existential over the
        // instance rows.
        Envelope::Instances { correlation } => {
            let mut probe = *correlation;
            probe.expr(existence_marker());
            if let Some(prefilter) = near_envelope_prefilter(geo, &planar) {
                probe.and_where(prefilter);
            }
            probe.and_where(spatial);
            Condition::all().add(SeaExpr::exists(probe))
        }
    })
}

/// A bounding-box test a geometry `GiST` index answers, admitting every geometry
/// a `near` relation with a `maxDistance` could match, or `None` for any other
/// relation.
///
/// `near` measures on the spheroid over a `geometry` column cast to
/// `geography`, which reaches no index. The [`NearEnvelope`] around the
/// reference geometry's great-circle bounds covers every point within
/// `maxDistance` of it on the spheroid, so the test discards no geometry a
/// vertex of which the relation could match, and the exact distance is still
/// evaluated on what it keeps. Where the envelope wraps across the
/// antimeridian it is tested as its two halves. A `near` with only a
/// `minDistance` matches geometries arbitrarily far from the reference and
/// admits no box.
///
/// The test compares the stored geometry's planar box, which bounds its
/// vertices only. A stored edge long enough for its great circle to bow past
/// its vertices towards the reference can therefore still be discarded.
pub(crate) fn near_envelope_prefilter(geo: &GeoQuery, geometry: &SimpleExpr) -> Option<SimpleExpr> {
    let GeoRel::Near {
        max_distance: Some(max_distance),
        ..
    } = &geo.georel
    else {
        return None;
    };
    let envelope = NearEnvelope::around(great_circle_bounds(&geo.geometry.planar())?, *max_distance);
    envelope
        .rects()
        .iter()
        // clone: each rectangle of a split envelope tests the same column
        .map(|rect| bounding_boxes_overlap(geometry.clone(), st_make_envelope([rect.min().x, rect.min().y], [rect.max().x, rect.max().y])))
        .reduce(ExprTrait::or)
}

/// The topological or distance relation of a geo query as a boolean SQL
/// expression over a subject geometry (clause 4.10).
///
/// `planar` is the subject read as a `PostGIS` `geometry`, which the six
/// topological relations evaluate; `geographic` is the same subject cast to
/// `geography`, which the spheroidal `near` relation measures on. Exactly one of
/// the two is read, whichever the relation names. The relation semantics live
/// here rather than inside [`condition`] so a caller that reads its subject
/// geometry from rows this module does not model -- the `attribute_history`
/// instances a clause 5.7.4.4 temporal geo query filters -- applies the
/// identical relation rather than a second, drifting copy of it.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when the reference
/// geometry has no EWKB spelling the store can bind, and [`RenderError::Query`]
/// carrying [`QueryError::NearWithoutDistance`] when `georel=near` names neither
/// `maxDistance` nor `minDistance`.
pub fn relation_expression(georel: &GeoRel, planar: SimpleExpr, geographic: SimpleExpr, geometry: &Geometry) -> Result<SimpleExpr, RenderError> {
    let reference = encode(geometry.form())?;
    // Built fresh per relation so the EWKB it binds is not shared as an owned
    // value between the two operands a `near` relation carries.
    let reference_geom = || validated_geom_from_ewkb(SeaExpr::val(reference.to_vec()));
    let spatial = match georel {
        GeoRel::Within => st_within(planar, reference_geom()),
        GeoRel::Contains => st_contains(planar, reference_geom()),
        GeoRel::Intersects => st_intersects(planar, reference_geom()),
        GeoRel::Disjoint => st_disjoint(planar, reference_geom()),
        GeoRel::Equals => st_equals(planar, reference_geom()),
        GeoRel::Overlaps => st_overlaps(planar, reference_geom()),
        // `near` measures on the spheroid, so both operands become `geography`.
        GeoRel::Near { max_distance, min_distance } => {
            let reference_geography = cast(reference_geom(), PostgresType::Geography);
            let max_metres = max_distance.map(PositiveDistance::metres);
            let min_metres = min_distance.map(PositiveDistance::metres);
            near_expression(geographic, reference_geography, max_metres, min_metres)?
        }
    };
    Ok(spatial)
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
/// `None` means the target `GeoProperty` is one no Entity can convey, because the
/// broker has never recorded an attribute under that IRI.
fn subject_geometry(geo: &GeoQuery, source: GeometrySource<'_>, attribute_names: &ResolvedNames<AttributeName>) -> Option<SubjectGeometry> {
    match source {
        // A registration carries no attributes of its own, so `geoproperty`
        // does not apply and the named column is the whole of the subject.
        GeometrySource::Registration { column } => Some(SubjectGeometry {
            planar: SeaExpr::col(Alias::new(column)),
            geographic: cast(SeaExpr::col(Alias::new(column)), PostgresType::Geography),
            envelope: Envelope::RegistrationColumn,
        }),
        GeometrySource::Entity { rows, .. } => {
            let attribute_name_key = attribute_names.get(&geo.geoproperty)?;
            Some(SubjectGeometry {
                planar: instance_geometry_column(INSTANCE_ALIAS),
                geographic: cast(instance_geometry_column(INSTANCE_ALIAS), PostgresType::Geography),
                envelope: Envelope::Instances {
                    correlation: Box::new(geometry_instances_of(rows, INSTANCE_ALIAS, attribute_name_key)),
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

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_row_refs::EntityRowRefs,
        error::RenderError,
        geo_predicate::{GeometrySource, condition},
    };
    use andromeda_ngsi_ld_geometry::{dimension::GeometryDimension, geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::{
        error::QueryError,
        geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery},
    };
    use andromeda_postgresql_encoding::ewkb::encode;
    use andromeda_postgresql_schema::current::Entities;
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query, Value, Values};
    use std::{str::FromStr, sync::LazyLock};

    /// The unaliased entity rows the entity path filters.
    static UNALIASED: LazyLock<EntityRowRefs> = LazyLock::new(|| EntityRowRefs::unaliased(None));

    /// The rows as the temporal path aliases them: `e` for the identity,
    /// `s` for the search row.
    static ALIASED: LazyLock<EntityRowRefs> = LazyLock::new(|| EntityRowRefs::aliased(None, Alias::new("e"), Alias::new("s")));

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn point_query(georel: GeoRel, geoproperty: &str) -> GeoQuery {
        GeoQuery {
            georel,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] })).expect("point parses"),
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
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0, 3.0] }))
                .expect("point with altitude parses"),
            geoproperty: name(geoproperty),
        }
    }

    /// No in-memory index answered for the default `location` instances.
    static UNNARROWED: DefaultLocationNarrowing = DefaultLocationNarrowing::Unnarrowed;

    fn entity_source() -> GeometrySource<'static> {
        GeometrySource::Entity {
            rows: &UNALIASED,
            default_location: &UNNARROWED,
        }
    }

    /// The entity source as the temporal path aliases it.
    fn aliased_source() -> GeometrySource<'static> {
        GeometrySource::Entity {
            rows: &ALIASED,
            default_location: &UNNARROWED,
        }
    }

    /// The registration source every discovery query renders against.
    fn registration_source() -> GeometrySource<'static> {
        GeometrySource::Registration { column: "location" }
    }

    /// A custom `GeoProperty` IRI and the dictionary key it is taken to resolve
    /// to, for the tests that render its instance subquery.
    const CUSTOM_GEOPROPERTY: &str = "https://example.com/customGeo";
    const CUSTOM_GEOPROPERTY_ID: i32 = 42;

    /// A dictionary in which the default `location` is deliberately absent: it
    /// is read from its own column and must never need resolving.
    fn resolved_custom() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs([(name(CUSTOM_GEOPROPERTY), AttributeNameKey::from(CUSTOM_GEOPROPERTY_ID))])
    }

    /// A dictionary that knows nothing.
    fn resolved_none() -> ResolvedNames<AttributeName> {
        ResolvedNames::default()
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
                | Value::BigDecimal(_)
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
                max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                min_distance: Some(PositiveDistance::from_str("10").expect("positive distance")),
            },
            LOCATION,
        );
        let sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        assert!(sql.contains("ST_DWithin"), "{sql}");
        assert!(sql.contains("ST_Distance"), "{sql}");
    }

    #[test]
    fn a_search_alias_reaches_the_semi_join_and_each_instance_carries_its_own_expiry_guard() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let sql = render(condition(&geo, aliased_source(), &resolved_custom()).unwrap());
        assert!(sql.contains("\"e\".\"entity_key\" IN (SELECT \"geo_default\".\"entity_key\""), "{sql}");
        assert!(
            sql.contains("\"geo_default\".\"location_expires_at\" IS NULL OR \"geo_default\".\"location_expires_at\" > NOW()"),
            "the hoisted default instance is guarded by its own expiry: {sql}"
        );
        assert!(!sql.contains("\"s\".\"location_expires_at\""), "no entity-level expiry is mixed in: {sql}");
        assert!(!sql.contains("payload"), "the guard no longer walks the payload: {sql}");
    }

    /// Clause 4.10: `geoproperty` names any `GeoProperty` of the entity. Every
    /// one other than the default is stored as a `current_attribute_values` row
    /// rather than a column of `entities`, so the relation is evaluated over
    /// those instance rows, correlated back to the entity being filtered.
    #[test]
    fn a_custom_geoproperty_is_read_from_its_attribute_instance_rows() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        assert!(sql.contains("EXISTS(SELECT 1 FROM \"current_attribute_values\""), "{sql}");
        assert!(sql.contains("\"geo_instance\".\"value_geometry\""), "{sql}");
        assert!(
            sql.contains(&format!("\"attribute_name_key\" = {CUSTOM_GEOPROPERTY_ID}")),
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
        assert!(
            sql.contains("\"geo_instance\".\"expires_at\" IS NULL OR \"geo_instance\".\"expires_at\" > NOW()"),
            "each instance is guarded by its own clause 4.22 expiry: {sql}"
        );
    }

    /// Clause 4.10: where a `GeoProperty` has several `datasetId` instances, one
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

    /// The two spec-defined `GeoProperties` of clause 4.7 are not special-cased:
    /// they take the same instance-row route every other named `GeoProperty` does,
    /// so none of them can be the one that silently reads the wrong column.
    #[test]
    fn a_spec_defined_geoproperty_takes_the_same_route_as_a_custom_one() {
        for iri in ["https://uri.etsi.org/ngsi-ld/observationSpace", "https://uri.etsi.org/ngsi-ld/operationSpace"] {
            let names = ResolvedNames::from_pairs([(name(iri), AttributeNameKey::from(7_i32))]);
            let geo = point_query(GeoRel::Within, iri);
            let sql = render(condition(&geo, entity_source(), &names).unwrap());
            assert!(sql.contains("EXISTS(SELECT 1 FROM \"current_attribute_values\""), "{iri}: {sql}");
            assert!(sql.contains("\"attribute_name_key\" = 7"), "{iri}: {sql}");
        }
    }

    /// Clause 5.7.2.4: any instance of the default `GeoProperty` may match, so
    /// once the dictionary knows `location` the relation reads both the hoisted
    /// default instance and every `datasetId` instance row.
    #[test]
    fn the_default_geoproperty_reads_its_hoisted_instance_and_every_dataset_instance() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let names = ResolvedNames::from_pairs([(name(LOCATION), AttributeNameKey::from(3_i32))]);
        let sql = render(condition(&geo, entity_source(), &names).unwrap());
        assert!(sql.contains("ST_Within(\"geo_default\".\"location\""), "{sql}");
        assert!(sql.contains("\"geo_location_instance\".\"attribute_name_key\" = 3"), "{sql}");
        assert!(sql.contains("\"geo_location_instance\".\"dataset_id\" IS NOT NULL"), "{sql}");
    }

    /// A `location` the dictionary has never recorded has no instance rows, so
    /// the hoisted default instance decides alone.
    #[test]
    fn an_unrecorded_default_geoproperty_reads_its_hoisted_instance_alone() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let sql = render(condition(&geo, entity_source(), &resolved_none()).unwrap());
        assert!(sql.contains("ST_Within(\"geo_default\".\"location\""), "{sql}");
        assert!(!sql.contains("current_attribute_values"), "{sql}");
    }

    /// Clause 4.10: an Entity that does not convey the target `GeoProperty` is
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
    /// reaches the default `GeoProperty`'s dedicated geography column.
    #[test]
    fn a_near_relation_casts_the_instance_geometry_to_geography() {
        let geo = point_query(
            GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("500").expect("positive distance")),
                min_distance: None,
            },
            CUSTOM_GEOPROPERTY,
        );
        let sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        assert!(sql.contains("\"geo_instance\".\"value_geometry\"::geography"), "{sql}");
        assert!(sql.contains("ST_DWithin"), "{sql}");
        assert!(
            sql.contains("\"geo_instance\".\"value_geometry\" && ST_MakeEnvelope("),
            "the cast column reaches no index, so a box the geometry GiST answers prefilters it: {sql}"
        );
    }

    /// A `near` query on a custom `GeoProperty` at the given reference point.
    fn near_query(longitude: f64, latitude: f64, max_distance: Option<&str>, min_distance: Option<&str>) -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Near {
                max_distance: max_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
                min_distance: min_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
            },
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [longitude, latitude] })).expect("point parses"),
            geoproperty: name(CUSTOM_GEOPROPERTY),
        }
    }

    /// The doubles a statement binds, in order.
    fn bound_doubles(values: &Values) -> Vec<f64> {
        values
            .0
            .iter()
            .filter_map(|value| if let Value::Double(Some(double)) = value { Some(*double) } else { None })
            .collect()
    }

    /// A reference by the antimeridian reaches both its sides, so the box
    /// test is the disjunction of the envelope's two halves rather than one box
    /// that stops at the seam.
    #[test]
    fn a_near_prefilter_across_the_antimeridian_tests_both_halves() {
        let geo = near_query(179.999, -17.0, Some("300"), None);
        let (sql, values) = render_parameterised(condition(&geo, entity_source(), &resolved_custom()).expect("renders"));
        assert_eq!(sql.matches("\"geo_instance\".\"value_geometry\" && ST_MakeEnvelope(").count(), 2, "{sql}");
        assert!(sql.contains(")) OR (\"geo_instance\".\"value_geometry\" && ST_MakeEnvelope("), "{sql}");
        let doubles = bound_doubles(&values);
        assert!(doubles.contains(&180.0) && doubles.contains(&-180.0), "{doubles:?}");
    }

    /// `near` is measured on the spheroid, where 5 000 m due north of the
    /// equator is latitude 0.045 218 4, beyond the 0.044 966 a box grown on the
    /// sphere would reach.
    #[test]
    fn a_near_prefilter_covers_the_spheroidal_reach_of_the_reference() {
        let geo = near_query(0.0, 0.0, Some("5000"), None);
        let (_, values) = render_parameterised(condition(&geo, entity_source(), &resolved_custom()).expect("renders"));
        // The distance itself binds as a double too; the box's ordinates are
        // the ones that are latitudes.
        let north = bound_doubles(&values)
            .into_iter()
            .filter(|double| double.abs() <= 90.0)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(north >= 0.045_218, "the box reaches latitude {north}");
    }

    #[test]
    fn a_min_distance_only_near_renders_no_prefilter() {
        let geo = near_query(1.0, 2.0, None, Some("1000"));
        let sql = render(condition(&geo, entity_source(), &resolved_custom()).expect("renders"));
        assert!(!sql.contains("&&"), "{sql}");
        assert!(sql.contains("ST_Distance"), "{sql}");
    }

    /// The instance subquery correlates to whichever alias the enclosing
    /// statement joined `entities` under, so the temporal path reaches the same
    /// rows the entity path does.
    #[test]
    fn an_instance_subquery_correlates_through_the_table_alias() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let sql = render(condition(&geo, aliased_source(), &resolved_custom()).unwrap());
        assert!(sql.contains("\"geo_instance\".\"entity_key\" = \"e\".\"entity_key\""), "{sql}");
    }

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
    /// the constant-`FALSE` an unresolvable `GeoProperty` gives an entity query:
    /// the two null readings belong to the two sources and cannot be mixed.
    #[test]
    fn a_registration_never_renders_the_entity_exclusion() {
        let geo = point_query(GeoRel::Within, "https://example.com/neverRecorded");
        let sql = render(condition(&geo, registration_source(), &resolved_none()).expect("renders"));
        assert!(!sql.contains("FALSE"), "{sql}");
        assert!(sql.contains("ST_Within"), "{sql}");
    }

    /// Every relation renders through the same seven arms whatever the source,
    /// so the `PostGIS` function each one picks cannot drift between callers.
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
                    max_distance: Some(PositiveDistance::from_str("500").expect("positive distance")),
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
                max_distance: Some(PositiveDistance::from_str("500").expect("positive distance")),
                min_distance: None,
            },
            LOCATION,
        );
        let entity_sql = render(condition(&geo, entity_source(), &resolved_custom()).unwrap());
        let temporal_sql = render(condition(&geo, aliased_source(), &resolved_custom()).unwrap());
        assert!(entity_sql.contains("ST_DWithin"), "{entity_sql}");
        assert!(temporal_sql.contains("ST_DWithin"), "{temporal_sql}");
        // The only difference is the qualification of the outer rows' columns.
        assert_eq!(
            entity_sql.replace("\"entity_search\".", "\"s\".").replace("\"entities\".", "\"e\"."),
            temporal_sql
        );
    }

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
                max_distance: Some(PositiveDistance::from_str("500").expect("positive distance")),
                min_distance: Some(PositiveDistance::from_str("10").expect("positive distance")),
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
        let ewkb = encode(geo.geometry.form()).expect("elevated point encodes");
        let decoded = Geometry::try_from(&ewkb).expect("the bound bytes decode");
        assert_eq!(decoded.dimension(), GeometryDimension::ThreeD);

        let (_, values) = render_parameterised(condition(&geo, entity_source(), &resolved_custom()).expect("renders"));
        assert!(
            bound_bytes(&values).contains(&ewkb.to_vec()),
            "the statement binds exactly the encoded reference"
        );
    }
}
