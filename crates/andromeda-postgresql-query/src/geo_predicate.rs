//! SQL rendering of a geo query (clause 4.10).
//!
//! One reading of the seven `georel` relations, for every relation the broker
//! filters on. What varies between callers is where the subject geometry is
//! read from and what a row without one means; the relations themselves do not
//! vary. [`relation_expression`] and [`near_envelope_prefilter`] are that one
//! reading, with [`near_envelope_unindexed_prefilter`] the same envelope for a
//! per-Evolution probe and [`bounding_box_prefilter`] the box every relation
//! admitting one reduces to; [`condition`] applies it to an entity's `GeoProperty` instances,
//! and [`registration_area`] to the geometry a registration declares, with the
//! relation [`AreaCondition`] reduces the geoquery to, since that geometry bounds
//! where the source's Entities are rather than where one of them is (clause
//! 5.10.2.4).
//!
//! Clause 4.10 lets `geoproperty` name any `GeoProperty` of the entity, defaulting
//! to `location`, and clause 5.7.2.4 counts an entity as matching when any
//! instance of it meets the restriction. Every `GeoProperty` (`observationSpace`
//! and `operationSpace` as much as a custom one) is read from
//! `current_attributes`, which holds one `value_geometry` row per attribute
//! instance keyed by the `attribute_names` dictionary, so the
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
    geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery, registration_area::AreaCondition},
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
        unindexed_bounding_boxes_overlap,
        validated_geom_from_ewkb,
    },
};
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, SimpleExpr};

/// Render a geo query over the entity `rows` the enclosing statement filters.
///
/// A `GeoProperty` is reached through its `current_attributes` instance rows,
/// correlated back to the identity row's surrogate key. The default `location`
/// also reads the hoisted default instance on the search row, narrowed by what
/// `default_location` records an in-memory index established about it. Every
/// instance is excluded once its own clause 4.22 `expiresAt` has passed, and an
/// Entity that conveys no instance of the target `GeoProperty` is non-matching
/// (clause 4.10).
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
pub fn condition(
    geo: &GeoQuery,
    rows: &EntityRowRefs,
    default_location: &DefaultLocationNarrowing,
    attribute_names: &ResolvedNames<AttributeName>,
) -> Result<Condition, RenderError> {
    if geo.geoproperty.as_str() == LOCATION.as_str() {
        return any_location_instance(geo, rows, default_location, attribute_names.get(&geo.geoproperty));
    }
    let Some(attribute_name_key) = attribute_names.get(&geo.geoproperty) else {
        // Clause 4.10: an Entity that does not convey the target GeoProperty is
        // non-matching. An IRI with no `attribute_names` row is conveyed by no
        // Entity at all, so the predicate is constant.
        return Ok(Condition::all().add(false_literal()));
    };
    let planar = instance_geometry_column(INSTANCE_ALIAS);
    // clone: the instance scan also reads the planar column for its
    // bounding-box prefilter.
    let spatial = relation_expression(&geo.georel, planar.clone(), cast(planar.clone(), PostgresType::Geography), &geo.geometry)?;
    // Clause 5.7.2.4: it is enough that one instance of the GeoProperty meets
    // the restriction, which is exactly an existential over the instance rows.
    let mut probe = geometry_instances_of(rows, INSTANCE_ALIAS, attribute_name_key);
    probe.expr(existence_marker());
    if let Some(prefilter) = near_envelope_prefilter(geo, &planar) {
        probe.and_where(prefilter);
    }
    probe.and_where(spatial);
    Ok(Condition::all().add(SeaExpr::exists(probe)))
}

/// The relation a registration's `geometry` has to meet for an Entity inside it
/// to possibly match `geo` (clause 5.10.2.4), or `None` when the reduction
/// constrains no geometry. A null geometry meets none of them.
///
/// # Errors
///
/// The failures of [`relation_expression`].
pub fn registration_area(geo: &GeoQuery, geometry: &SimpleExpr) -> Result<Option<SimpleExpr>, RenderError> {
    // clone: the relation reads the column both planar and cast to geography
    let relation = |georel: &GeoRel| relation_expression(georel, geometry.clone(), cast(geometry.clone(), PostgresType::Geography), &geo.geometry);
    match AreaCondition::of(geo) {
        AreaCondition::Relation(georel) => relation(&georel).map(Some),
        AreaCondition::NotWithin => relation(&GeoRel::Within).map(|within| Some(within.not())),
        AreaCondition::Unconstrained => Ok(None),
    }
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
pub fn near_envelope_prefilter(geo: &GeoQuery, geometry: &SimpleExpr) -> Option<SimpleExpr> {
    near_envelope_test(geo, geometry, bounding_boxes_overlap)
}

/// A bounding-box test a geometry `GiST` index answers that every geometry
/// meeting the relation of `geo` passes, or `None` when the relation admits
/// no box.
///
/// `near` with a `maxDistance` tests the [`near_envelope_prefilter`]
/// envelope. `within`, `contains`, `intersects`, `overlaps` and `equals` each
/// need the two geometries to share a point, so their boxes overlap: the test
/// is the overlap with the reference geometry as the relation reads it.
/// `disjoint` and a `near` with only a `minDistance` match geometries
/// arbitrarily far from the reference.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when the reference
/// geometry has no EWKB spelling the store can bind.
pub fn bounding_box_prefilter(geo: &GeoQuery, geometry: SimpleExpr) -> Result<Option<SimpleExpr>, RenderError> {
    match &geo.georel {
        GeoRel::Near { .. } => Ok(near_envelope_prefilter(geo, &geometry)),
        GeoRel::Within | GeoRel::Contains | GeoRel::Intersects | GeoRel::Overlaps | GeoRel::Equals => {
            let reference = encode(geo.geometry.form())?;
            Ok(Some(bounding_boxes_overlap(
                geometry,
                validated_geom_from_ewkb(SeaExpr::val(reference.to_vec())),
            )))
        }
        GeoRel::Disjoint => Ok(None),
    }
}

/// The box test of [`near_envelope_prefilter`] over boxes no geometry index
/// stores, or `None` for any relation that admits no box.
///
/// For a probe that reads one Evolution's rows through its stream index: an
/// indexable test there lets the planner AND a geometry `GiST` into every
/// probe, while this one only spares the spheroidal distance of the rows it
/// discards.
pub fn near_envelope_unindexed_prefilter(geo: &GeoQuery, geometry: &SimpleExpr) -> Option<SimpleExpr> {
    near_envelope_test(geo, geometry, unindexed_bounding_boxes_overlap)
}

/// `overlap` of `geometry` with each rectangle of the [`NearEnvelope`] a
/// `near` relation with a `maxDistance` admits, joined by `OR`.
fn near_envelope_test(geo: &GeoQuery, geometry: &SimpleExpr, overlap: fn(SimpleExpr, SimpleExpr) -> SimpleExpr) -> Option<SimpleExpr> {
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
        .map(|rect| overlap(geometry.clone(), st_make_envelope([rect.min().x, rect.min().y], [rect.max().x, rect.max().y])))
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
/// geometry from rows this module does not model (the `attribute_history`
/// instances a clause 5.7.4.4 temporal geo query filters) applies the
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

/// Alias the correlated instance subquery reads `current_attributes`
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
#[expect(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        attribute_instance_correlation::instance_geometry_column,
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_row_refs::EntityRowRefs,
        error::RenderError,
        geo_predicate::{INSTANCE_ALIAS, bounding_box_prefilter, condition, near_envelope_prefilter, near_envelope_unindexed_prefilter},
    };
    use andromeda_ngsi_ld_geometry::{
        dimension::GeometryDimension,
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
    use andromeda_postgresql_encoding::ewkb::encode;
    use andromeda_postgresql_schema::current::Entities;
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query, SimpleExpr, Value, Values};
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
        let error = condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect_err("must reject");
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
        let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert!(sql.contains("ST_DWithin"), "{sql}");
        assert!(sql.contains("ST_Distance"), "{sql}");
    }

    #[test]
    fn a_search_alias_reaches_the_semi_join_and_each_instance_carries_its_own_expiry_guard() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let sql = render(condition(&geo, &ALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert!(sql.contains("\"e\".\"entity_key\" IN (SELECT \"geo_default\".\"entity_key\""), "{sql}");
        assert!(
            sql.contains("\"geo_default\".\"location_expires_at\" IS NULL OR \"geo_default\".\"location_expires_at\" > NOW()"),
            "the hoisted default instance is guarded by its own expiry: {sql}"
        );
        assert!(!sql.contains("\"s\".\"location_expires_at\""), "no entity-level expiry is mixed in: {sql}");
    }

    /// Clause 4.10: `geoproperty` names any `GeoProperty` of the entity. Every
    /// one other than the default is stored as a `current_attributes` row
    /// rather than a column of `entities`, so the relation is evaluated over
    /// those instance rows, correlated back to the entity being filtered.
    #[test]
    fn a_custom_geoproperty_is_read_from_its_attribute_instance_rows() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert!(sql.contains("EXISTS(SELECT 1 FROM \"current_attributes\""), "{sql}");
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
    /// that could pin it to the default instance: an instance that fails the
    /// relation cannot veto one that passes it.
    #[test]
    fn a_multi_instance_geoproperty_matches_when_any_one_instance_does() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert!(
            sql.contains("EXISTS(SELECT 1 FROM \"current_attributes\""),
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
            let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &names).unwrap());
            assert!(sql.contains("EXISTS(SELECT 1 FROM \"current_attributes\""), "{iri}: {sql}");
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
        let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &names).unwrap());
        assert!(sql.contains("ST_Within(\"geo_default\".\"location\""), "{sql}");
        assert!(sql.contains("\"geo_location_instance\".\"attribute_name_key\" = 3"), "{sql}");
        assert!(sql.contains("\"geo_location_instance\".\"dataset_id\" IS NOT NULL"), "{sql}");
    }

    /// A `location` the dictionary has never recorded has no instance rows, so
    /// the hoisted default instance decides alone.
    #[test]
    fn an_unrecorded_default_geoproperty_reads_its_hoisted_instance_alone() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_none()).unwrap());
        assert!(sql.contains("ST_Within(\"geo_default\".\"location\""), "{sql}");
        assert!(!sql.contains("current_attributes"), "{sql}");
    }

    /// Clause 4.10: an Entity that does not convey the target `GeoProperty` is
    /// non-matching. An IRI the dictionary has never seen is conveyed by no
    /// Entity, so the filter is constant `FALSE` rather than an error.
    #[test]
    fn an_unknown_geoproperty_matches_nothing_without_failing() {
        let geo = point_query(GeoRel::Within, "https://example.com/neverRecorded");
        let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_none()).expect("renders"));
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
        let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert!(
            sql.contains("COALESCE(\"geo_instance\".\"value_geometry_valid\", \"geo_instance\".\"value_geometry\")::geography"),
            "{sql}"
        );
        assert!(sql.contains("ST_DWithin"), "{sql}");
        assert!(
            sql.contains("COALESCE(\"geo_instance\".\"value_geometry_valid\", \"geo_instance\".\"value_geometry\") && ST_MakeEnvelope("),
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
        let (sql, values) = render_parameterised(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        assert_eq!(
            sql.matches("COALESCE(\"geo_instance\".\"value_geometry_valid\", \"geo_instance\".\"value_geometry\") && ST_MakeEnvelope(")
                .count(),
            2,
            "{sql}"
        );
        assert!(
            sql.contains(")) OR (COALESCE(\"geo_instance\".\"value_geometry_valid\", \"geo_instance\".\"value_geometry\") && ST_MakeEnvelope("),
            "{sql}"
        );
        let doubles = bound_doubles(&values);
        assert!(doubles.contains(&180.0) && doubles.contains(&-180.0), "{doubles:?}");
    }

    /// `near` is measured on the spheroid, where 5 000 m due north of the
    /// equator is latitude 0.045 218 4, beyond the 0.044 966 a box grown on the
    /// sphere would reach.
    #[test]
    fn a_near_prefilter_covers_the_spheroidal_reach_of_the_reference() {
        let geo = near_query(0.0, 0.0, Some("5000"), None);
        let (_, values) = render_parameterised(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        // The distance itself binds as a double too; the box's ordinates are
        // the ones that are latitudes.
        let north = bound_doubles(&values)
            .into_iter()
            .filter(|double| double.abs() <= 90.0)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(north >= 0.045_218, "the box reaches latitude {north}");
    }

    /// The instance geometry a box test is rendered over, as a named
    /// `GeoProperty`'s predicate judges it.
    fn instance_geometry() -> SimpleExpr {
        instance_geometry_column(INSTANCE_ALIAS)
    }

    /// A box test alone, with its bound values.
    fn render_box_test(test: SimpleExpr) -> (String, Values) {
        Query::select().expr(test).build(PostgresQueryBuilder)
    }

    #[test]
    fn the_unindexed_box_is_the_near_envelope() {
        let geo = near_query(4.5, 52.5, Some("2000"), None);
        let (sql, values) = render_box_test(near_envelope_unindexed_prefilter(&geo, &instance_geometry()).expect("a near box"));
        assert_eq!(
            sql.matches("COALESCE(\"geo_instance\".\"value_geometry_valid\", \"geo_instance\".\"value_geometry\")::box2d && ST_MakeEnvelope(")
                .count(),
            1,
            "{sql}"
        );
        assert!(sql.ends_with(")::box2d"), "{sql}");
        let reference = great_circle_bounds(&geo.geometry.planar()).expect("a point has bounds");
        let NearEnvelope::Single(rect) = NearEnvelope::around(reference, PositiveDistance::from_str("2000").expect("positive distance")) else {
            panic!("a reference far from the antimeridian has one rectangle");
        };
        assert_eq!(bound_doubles(&values), [rect.min().x, rect.min().y, rect.max().x, rect.max().y]);

        let across = near_query(179.99, 52.5, Some("2000"), None);
        let (sql, _) = render_box_test(near_envelope_unindexed_prefilter(&across, &instance_geometry()).expect("a near box"));
        assert_eq!(
            sql.matches("COALESCE(\"geo_instance\".\"value_geometry_valid\", \"geo_instance\".\"value_geometry\")::box2d && ST_MakeEnvelope(")
                .count(),
            2,
            "{sql}"
        );
        assert!(
            sql.contains(")::box2d) OR (COALESCE(\"geo_instance\".\"value_geometry_valid\", \"geo_instance\".\"value_geometry\")::box2d && ST_MakeEnvelope("),
            "{sql}"
        );
    }

    /// The Entity geo-query and registration discovery read the geometry
    /// through its `GiST` index, so their box test stays the indexable `&&`.
    #[test]
    fn the_entity_and_registration_prefilters_stay_indexable() {
        let geo = near_query(4.5, 52.5, Some("2000"), None);
        let (sql, _) = render_box_test(near_envelope_prefilter(&geo, &instance_geometry()).expect("a near box"));
        assert!(
            sql.contains("COALESCE(\"geo_instance\".\"value_geometry_valid\", \"geo_instance\".\"value_geometry\") && ST_MakeEnvelope("),
            "{sql}"
        );
        assert!(!sql.contains("::box2d"), "{sql}");
        let entity = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        assert!(
            entity.contains("COALESCE(\"geo_instance\".\"value_geometry_valid\", \"geo_instance\".\"value_geometry\") && ST_MakeEnvelope("),
            "{entity}"
        );
        assert!(!entity.contains("::box2d"), "{entity}");
    }

    /// Each of the five area relations needs the two geometries to share a
    /// point, so each admits the overlap of the boxes, with the reference made
    /// valid as the relation reads it.
    #[test]
    fn an_area_relation_admits_the_reference_bounding_box() {
        for georel in [GeoRel::Within, GeoRel::Contains, GeoRel::Intersects, GeoRel::Overlaps, GeoRel::Equals] {
            let geo = point_query(georel, CUSTOM_GEOPROPERTY);
            let (sql, values) = render_box_test(bounding_box_prefilter(&geo, instance_geometry()).expect("renders").expect("an area box"));
            assert_eq!(
                sql, r#"SELECT COALESCE("geo_instance"."value_geometry_valid", "geo_instance"."value_geometry") && ST_MakeValid(ST_GeomFromEWKB($1))"#,
                "{:?}",
                geo.georel
            );
            assert_eq!(bound_bytes(&values), [encode(geo.geometry.form()).expect("encodes").to_vec()]);
        }
    }

    #[test]
    fn a_near_box_is_the_near_envelope_and_disjoint_admits_none() {
        let geo = near_query(4.5, 52.5, Some("2000"), None);
        let near = render_box_test(bounding_box_prefilter(&geo, instance_geometry()).expect("renders").expect("a near box"));
        assert_eq!(near, render_box_test(near_envelope_prefilter(&geo, &instance_geometry()).expect("a near box")));
        let far = near_query(4.5, 52.5, None, Some("10"));
        assert!(bounding_box_prefilter(&far, instance_geometry()).expect("renders").is_none());
        let disjoint = point_query(GeoRel::Disjoint, CUSTOM_GEOPROPERTY);
        assert!(bounding_box_prefilter(&disjoint, instance_geometry()).expect("renders").is_none());
    }

    #[test]
    fn a_min_distance_only_near_renders_no_prefilter() {
        let geo = near_query(1.0, 2.0, None, Some("1000"));
        let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        assert!(!sql.contains("&&"), "{sql}");
        assert!(sql.contains("ST_Distance"), "{sql}");
    }

    /// The instance subquery correlates to whichever alias the enclosing
    /// statement joined `entities` under, so the temporal path reaches the same
    /// rows the entity path does.
    #[test]
    fn an_instance_subquery_correlates_through_the_table_alias() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let sql = render(condition(&geo, &ALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert!(sql.contains("\"geo_instance\".\"entity_key\" = \"e\".\"entity_key\""), "{sql}");
    }

    #[test]
    fn an_entity_without_a_location_does_not_match_a_geo_filter() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        // The expiry guard has an `IS NULL` of its own, so the assertion names
        // the geometry column rather than the operator.
        assert!(!sql.contains("\"location\" IS NULL"), "an entity query admits no null geometry: {sql}");
    }

    #[test]
    fn every_georel_renders_its_postgis_function_on_an_entity() {
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
                    min_distance: Some(PositiveDistance::from_str("100").expect("positive distance")),
                },
                "ST_DWithin",
            ),
        ];
        for (georel, function) in cases {
            let geo = point_query(georel, LOCATION);
            let sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
            assert!(sql.contains(function), "{:?} on an entity: {sql}", geo.georel);
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
        let entity_sql = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        let temporal_sql = render(condition(&geo, &ALIASED, &UNNARROWED, &resolved_custom()).unwrap());
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
            let flat = point_query(georel.clone(), LOCATION); // clone: the relation is reused for the elevated query
            let elevated = point_query_3d(georel, LOCATION);
            let (flat_sql, flat_values) = render_parameterised(condition(&flat, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("flat renders"));
            let (elevated_sql, elevated_values) =
                render_parameterised(condition(&elevated, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("elevated renders"));

            assert_eq!(elevated_sql, flat_sql, "{:?} must not vary with the reference dimensionality", flat.georel);
            assert_ne!(
                bound_bytes(&elevated_values),
                bound_bytes(&flat_values),
                "{:?} must still bind the altitude it was given",
                flat.georel
            );
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

        let (_, values) = render_parameterised(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        assert!(
            bound_bytes(&values).contains(&ewkb.to_vec()),
            "the statement binds exactly the encoded reference"
        );
    }
}
