//! SQL rendering of a geo query (clause 4.10).
//!
//! One reading of the seven `georel` relations, for every relation the broker
//! filters on. What varies between callers is where the subject geometry is
//! read from and what a row without one means; the relations themselves do not
//! vary. [`restriction`] is that one reading: the box test of
//! [`crate::stored_geometry`], then the exact relation of
//! [`relation_expression`], which renders the test [`RelationReading`]
//! states for the relation. [`condition`] applies it to an entity's
//! `GeoProperty` instances, and [`registration_area`] to the geometry a
//! registration declares, with the weaker relation the same reading states for
//! it, since that geometry bounds where the source's Entities are rather than
//! where one of them is (clause 5.10.2.4).
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
//!
//! A geo-query is read three ways over the same instances. [`condition`] keeps
//! the entities the enclosing statement filters. [`matches`](fn@matches) reads the entities
//! that hold a matching instance with no outer row, through the spatial
//! indexes, for a page that reads them as candidates before it reads the
//! entities themselves. [`probe`] tests the instances of one entity, looked up
//! by its key, for a page that visits the entities in order and keeps those
//! that match. The probe's box test reads no spatial index: an indexable box
//! test lets the planner AND a geometry `GiST` into each per-entity
//! primary-key lookup.

use crate::{
    attribute_instance_correlation::{existence_marker, geometry_instances, geometry_instances_of, instance_stored_geometry},
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    location_instances::{any_location_instance, location_matches, location_probe},
    near_measure::near_expression,
    stored_geometry::{BoxIndexing, StoredGeometry, box_test},
};
use andromeda_ngsi_ld_geometry::{
    geo_rel::GeoRel,
    geometry::Geometry,
    positive_distance::PositiveDistance,
    relation_reading::{AreaCondition, Operand, RelationReading, SpatialTest, TopologicalTest},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery};
use andromeda_ngsi_ld_vocabulary::expanded::properties::LOCATION;
use andromeda_postgresql_encoding::ewkb::encode;
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::{
    cast::cast,
    literal::false_literal,
    pg_type::PostgresType,
    postgis::{exact_contains, exact_equals, exact_intersects, exact_overlaps, exact_within, st_disjoint, validated_geom_from_ewkb},
};
use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, SelectStatement, SimpleExpr};

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
/// [`RenderError::Query`] carrying [`QueryError::NearWithoutDistance`](andromeda_ngsi_ld_query::error::QueryError::NearWithoutDistance) when
/// `georel=near` names neither `maxDistance` nor `minDistance`, which clause
/// 4.10 requires at least one of.
pub fn condition(
    geo: &GeoQuery,
    rows: &EntityRowRefs,
    default_location: &DefaultLocationNarrowing,
    attribute_names: &ResolvedNames<AttributeName>,
) -> Result<Condition, RenderError> {
    match GeoTarget::of(geo, attribute_names) {
        GeoTarget::Location(location_key) => any_location_instance(geo, rows, default_location, location_key),
        GeoTarget::Instances(attribute_name_key) => any_instance_meets(geo, rows, attribute_name_key, BoxIndexing::Indexed),
        // The predicate is constant: no Entity conveys the GeoProperty.
        GeoTarget::Unrecorded => Ok(Condition::all().add(false_literal())),
    }
}

/// The `entity_key` of every entity one of whose instances of `geo`'s
/// `GeoProperty` meets the restriction, once per matching instance and in no
/// order, read through the spatial indexes, or `None` when no instance can
/// match: the `GeoProperty` is one the dictionary never recorded, or no arm of
/// the default `location` can hold a match.
///
/// `rows` names the branch and the instant the instances are read at; nothing
/// correlates to it. `default_location` narrows the default `location` as it
/// does for [`condition`].
///
/// # Errors
///
/// The failures of [`condition`].
pub fn matches(
    geo: &GeoQuery,
    rows: &EntityRowRefs,
    default_location: &DefaultLocationNarrowing,
    attribute_names: &ResolvedNames<AttributeName>,
) -> Result<Option<SelectStatement>, RenderError> {
    match GeoTarget::of(geo, attribute_names) {
        GeoTarget::Location(location_key) => location_matches(geo, rows, default_location, location_key),
        GeoTarget::Instances(attribute_name_key) => {
            let mut scan = geometry_instances(rows, INSTANCE_ALIAS, attribute_name_key);
            scan.column((Alias::new(INSTANCE_ALIAS), CurrentAttributes::EntityKey)).cond_where(restriction(
                &geo.georel,
                &geo.geometry,
                &instance_stored_geometry(INSTANCE_ALIAS),
                BoxIndexing::Indexed,
            )?);
            Ok(Some(scan))
        }
        GeoTarget::Unrecorded => Ok(None),
    }
}

/// Whether the entity `rows` names holds an instance of `geo`'s
/// `GeoProperty` that meets the restriction, each instance looked up by the
/// entity's key and its box tested with [`BoxIndexing::Unindexed`].
///
/// The answer for an entity is the one [`condition`] gives it, and the
/// predicate is constant `FALSE` for a `GeoProperty` the dictionary never
/// recorded.
///
/// # Errors
///
/// The failures of [`condition`].
pub fn probe(geo: &GeoQuery, rows: &EntityRowRefs, attribute_names: &ResolvedNames<AttributeName>) -> Result<Condition, RenderError> {
    match GeoTarget::of(geo, attribute_names) {
        GeoTarget::Location(location_key) => location_probe(geo, rows, location_key),
        GeoTarget::Instances(attribute_name_key) => any_instance_meets(geo, rows, attribute_name_key, BoxIndexing::Unindexed),
        GeoTarget::Unrecorded => Ok(Condition::all().add(false_literal())),
    }
}

/// Whether one of the instances of the entity `rows` names, stored under
/// `attribute_name_key`, meets `geo`, its box tested with `indexing`.
fn any_instance_meets(geo: &GeoQuery, rows: &EntityRowRefs, attribute_name_key: AttributeNameKey, indexing: BoxIndexing) -> Result<Condition, RenderError> {
    let spatial = restriction(&geo.georel, &geo.geometry, &instance_stored_geometry(INSTANCE_ALIAS), indexing)?;
    // Clause 5.7.2.4: it is enough that one instance of the GeoProperty meets
    // the restriction, which is exactly an existential over the instance rows.
    let mut probe = geometry_instances_of(rows, INSTANCE_ALIAS, attribute_name_key);
    probe.expr(existence_marker()).cond_where(spatial);
    Ok(Condition::all().add(SeaExpr::exists(probe)))
}

/// The relation a registration's geometry has to meet for an Entity inside it
/// to possibly match `geo` (clause 5.10.2.4), behind the box test it admits,
/// or `None` when the reduction constrains no geometry. A null geometry meets
/// none of them.
///
/// # Errors
///
/// The failures of [`relation_expression`].
pub fn registration_area(geo: &GeoQuery, subject: &StoredGeometry) -> Result<Option<Condition>, RenderError> {
    match RelationReading::of(&geo.georel).registration {
        AreaCondition::Relation(georel) => restriction(&georel, &geo.geometry, subject, BoxIndexing::Indexed).map(Some),
        AreaCondition::NotWithin => relation_expression(&GeoRel::Within, subject, &geo.geometry).map(|within| Some(Condition::all().add(within.not()))),
        AreaCondition::Unconstrained => Ok(None),
    }
}

/// A subject geometry meets `georel` against `reference`: the box test the
/// relation admits on the box `subject`'s index keys, then the exact relation
/// on the geometry it judges.
///
/// The box test is what reaches a spatial index, since the relations read the
/// judged geometry, not the indexed box, and `near` reads it cast to
/// `geography`, which no index keys. It is also the one condition the planner
/// estimates from the box: the exact relations carry no index support.
///
/// # Errors
///
/// The failures of [`box_test`] and [`relation_expression`].
pub fn restriction(georel: &GeoRel, reference: &Geometry, subject: &StoredGeometry, indexing: BoxIndexing) -> Result<Condition, RenderError> {
    let boxed = box_test(georel, reference, &subject.indexed_box(), indexing)?;
    Ok(Condition::all().add_option(boxed).add(relation_expression(georel, subject, reference)?))
}

/// The test a geo query's relation states ([`RelationReading`]) as a boolean
/// SQL expression over a subject geometry (clause 4.10).
///
/// A topological test evaluates the subject read as a `PostGIS` `geometry`;
/// the distance test measures the same subject cast to `geography`, on the
/// spheroid. The relation semantics live here rather than inside
/// [`condition`] so a caller that reads its subject geometry from rows this
/// module does not model (the `attribute_history` instances a clause 5.7.4.4
/// temporal geo query filters) applies the identical relation rather than a
/// second, drifting copy of it.
///
/// # Errors
///
/// Returns [`RenderError::ReferenceGeometryEncoding`] when the reference
/// geometry has no EWKB spelling the store can bind, and [`RenderError::Query`]
/// carrying [`QueryError::NearWithoutDistance`](andromeda_ngsi_ld_query::error::QueryError::NearWithoutDistance) when `georel=near` names neither
/// `maxDistance` nor `minDistance`.
pub fn relation_expression(georel: &GeoRel, subject: &StoredGeometry, geometry: &Geometry) -> Result<SimpleExpr, RenderError> {
    let reference = encode(geometry.form())?;
    // Built fresh per relation so the EWKB it binds is not shared as an owned
    // value between the two operands a `near` relation carries.
    let reference_geom = || validated_geom_from_ewkb(SeaExpr::val(reference.to_vec()));
    let spatial = match RelationReading::of(georel).test {
        SpatialTest::Topological(test) => topological_expression(test, subject.planar(), reference_geom()),
        SpatialTest::Distance { max_distance, min_distance } => near_expression(
            subject,
            || cast(reference_geom(), PostgresType::Geography),
            geometry.form(),
            max_distance.map(PositiveDistance::metres),
            min_distance.map(PositiveDistance::metres),
        )?,
    };
    Ok(spatial)
}

/// A topological test over the planar subject and reference.
fn topological_expression(test: TopologicalTest, subject: SimpleExpr, reference: SimpleExpr) -> SimpleExpr {
    match test {
        TopologicalTest::Contains { container: Operand::Reference } => exact_within(subject, reference),
        TopologicalTest::Contains { container: Operand::Subject } => exact_contains(subject, reference),
        TopologicalTest::Intersects => exact_intersects(subject, reference),
        TopologicalTest::Disjoint => st_disjoint(subject, reference),
        TopologicalTest::Equals => exact_equals(subject, reference),
        TopologicalTest::Overlaps => exact_overlaps(subject, reference),
    }
}

/// Where the instances of a geo-query's `GeoProperty` are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeoTarget {
    /// The default `location`, hoisted onto the search row, with the
    /// dictionary key of its instance rows, or `None` when the dictionary has
    /// never recorded it and no instance row exists.
    Location(Option<AttributeNameKey>),
    /// Any other `GeoProperty` the dictionary has recorded, whose instances are
    /// the `current_attributes` rows under this key.
    Instances(AttributeNameKey),
    /// A `GeoProperty` the dictionary has never recorded. Clause 4.10: an
    /// Entity that does not convey the target `GeoProperty` is non-matching,
    /// and an IRI with no `attribute_names` row is conveyed by no Entity at
    /// all.
    Unrecorded,
}

impl GeoTarget {
    /// Where the instances of `geo`'s `GeoProperty` are stored, by the keys
    /// `attribute_names` resolved.
    fn of(geo: &GeoQuery, attribute_names: &ResolvedNames<AttributeName>) -> GeoTarget {
        let key = attribute_names.get(&geo.geoproperty);
        if geo.geoproperty.as_str() == LOCATION.as_str() {
            return GeoTarget::Location(key);
        }
        key.map_or(GeoTarget::Unrecorded, GeoTarget::Instances)
    }
}

/// Alias the instance subquery and the instance matches read
/// `current_attributes` under.
///
/// Distinct enough not to collide with an alias the enclosing statement chose.
const INSTANCE_ALIAS: &str = "geo_instance";

#[cfg(test)]
mod tests {
    use crate::{
        entity_row_refs::EntityRowRefs,
        error::RenderError,
        geo_predicate::{condition, matches, probe},
    };
    use andromeda_ngsi_ld_geometry::{dimension::GeometryDimension, geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::{
        error::QueryError,
        geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery},
    };
    use andromeda_postgresql_encoding::ewkb::encode;
    use andromeda_postgresql_schema::current::Entities;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query, SelectStatement, Value, Values};
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

    fn render(condition: Condition) -> SelectStatement {
        Query::select().column(Entities::Id).from(Entities::Table).cond_where(condition).to_owned()
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

    /// The rows a page's walk probes: each visited Entity's key is read from
    /// the walk.
    static WALKED: LazyLock<EntityRowRefs> = LazyLock::new(|| EntityRowRefs::unaliased(None).keyed_by(Alias::new("walked")));

    /// The dictionary key `location`'s instance rows are taken to be stored
    /// under.
    const LOCATION_KEY: i32 = 9;

    /// A dictionary that has recorded `location`.
    fn resolved_location() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs([(name(LOCATION), AttributeNameKey::from(LOCATION_KEY))])
    }

    /// The seven relations of clause 4.10, `near` bounded on both sides.
    fn every_georel() -> [GeoRel; 7] {
        [
            GeoRel::Within,
            GeoRel::Contains,
            GeoRel::Intersects,
            GeoRel::Disjoint,
            GeoRel::Equals,
            GeoRel::Overlaps,
            GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("500").expect("positive distance")),
                min_distance: Some(PositiveDistance::from_str("100").expect("positive distance")),
            },
        ]
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
                Value::Bytes(Some(bytes)) => Some(bytes.clone()),
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
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert_statement_snapshot!("geo-predicate-near-with-both-distances-renders-a-conjunction", statement);
    }

    #[test]
    fn a_search_alias_reaches_the_semi_join_and_each_instance_carries_its_own_expiry_guard() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let statement = render(condition(&geo, &ALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert_statement_snapshot!(
            "geo-predicate-a-search-alias-reaches-the-semi-join-and-each-instance-carries-its-own-expiry-guard",
            statement
        );
    }

    /// Clause 4.10: `geoproperty` names any `GeoProperty` of the entity. Every
    /// one other than the default is stored as a `current_attributes` row
    /// rather than a column of `entities`, so the relation is evaluated over
    /// those instance rows, correlated back to the entity being filtered.
    #[test]
    fn a_custom_geoproperty_is_read_from_its_attribute_instance_rows() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert_statement_snapshot!("geo-predicate-a-custom-geoproperty-is-read-from-its-attribute-instance-rows", statement);
    }

    /// A planar relation reads the judged geometry, which the index does not
    /// key, so the box the index keys, great-circle reach first, is tested
    /// explicitly before the relation.
    #[test]
    fn an_area_relation_tests_the_indexed_reach_box_before_the_relation() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        assert_statement_snapshot!("geo-predicate-an-area-relation-tests-the-indexed-reach-box-before-the-relation", statement);
    }

    /// Clause 4.10: where a `GeoProperty` has several `datasetId` instances, one
    /// of them meeting the restriction is enough. The predicate is therefore an
    /// existential over the instance rows, with no restriction on `dataset_id`
    /// that could pin it to the default instance: an instance that fails the
    /// relation cannot veto one that passes it.
    #[test]
    fn a_multi_instance_geoproperty_matches_when_any_one_instance_does() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert_statement_snapshot!("geo-predicate-a-multi-instance-geoproperty-matches-when-any-one-instance-does", statement);
    }

    /// The two spec-defined `GeoProperties` of clause 4.7 are not special-cased:
    /// they take the same instance-row route every other named `GeoProperty` does,
    /// so none of them can be the one that silently reads the wrong column.
    #[test]
    fn a_spec_defined_geoproperty_takes_the_same_route_as_a_custom_one() {
        for (index, iri) in ["https://uri.etsi.org/ngsi-ld/observationSpace", "https://uri.etsi.org/ngsi-ld/operationSpace"]
            .into_iter()
            .enumerate()
        {
            let names = ResolvedNames::from_pairs([(name(iri), AttributeNameKey::from(7_i32))]);
            let geo = point_query(GeoRel::Within, iri);
            assert_statement_snapshot!(
                format!("geo-predicate-a-spec-defined-geoproperty-takes-the-same-route-as-a-custom-one-{}", index + 1),
                render(condition(&geo, &UNALIASED, &UNNARROWED, &names).unwrap())
            );
        }
    }

    /// Clause 5.7.2.4: any instance of the default `GeoProperty` may match, so
    /// once the dictionary knows `location` the relation reads both the hoisted
    /// default instance and every `datasetId` instance row.
    #[test]
    fn the_default_geoproperty_reads_its_hoisted_instance_and_every_dataset_instance() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let names = ResolvedNames::from_pairs([(name(LOCATION), AttributeNameKey::from(3_i32))]);
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &names).unwrap());
        assert_statement_snapshot!(
            "geo-predicate-the-default-geoproperty-reads-its-hoisted-instance-and-every-dataset-instance",
            statement
        );
    }

    /// A `location` the dictionary has never recorded has no instance rows, so
    /// the hoisted default instance decides alone.
    #[test]
    fn an_unrecorded_default_geoproperty_reads_its_hoisted_instance_alone() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_none()).unwrap());
        assert_statement_snapshot!("geo-predicate-an-unrecorded-default-geoproperty-reads-its-hoisted-instance-alone", statement);
    }

    /// Clause 4.10: an Entity that does not convey the target `GeoProperty` is
    /// non-matching. An IRI the dictionary has never seen is conveyed by no
    /// Entity, so the filter is constant `FALSE` rather than an error.
    #[test]
    fn an_unknown_geoproperty_matches_nothing_without_failing() {
        let geo = point_query(GeoRel::Within, "https://example.com/neverRecorded");
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_none()).expect("renders"));
        assert_statement_snapshot!("geo-predicate-an-unknown-geoproperty-matches-nothing-without-failing", statement);
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
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert_statement_snapshot!("geo-predicate-a-near-relation-casts-the-instance-geometry-to-geography", statement);
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

    /// A reference by the antimeridian reaches both its sides, so the box
    /// test is the disjunction of the envelope's two halves rather than one box
    /// that stops at the seam.
    #[test]
    fn a_near_prefilter_across_the_antimeridian_tests_both_halves() {
        let geo = near_query(179.999, -17.0, Some("300"), None);
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        assert_statement_snapshot!("geo-predicate-a-near-prefilter-across-the-antimeridian-tests-both-halves", statement);
    }

    /// `near` is measured on the spheroid, where 5 000 m due north of the
    /// equator is latitude 0.045 218 4, beyond the 0.044 966 a box grown on the
    /// sphere would reach.
    #[test]
    fn a_near_prefilter_covers_the_spheroidal_reach_of_the_reference() {
        let geo = near_query(0.0, 0.0, Some("5000"), None);
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        assert_statement_snapshot!("geo-predicate-a-near-prefilter-covers-the-spheroidal-reach-of-the-reference", statement);
    }

    #[test]
    fn a_min_distance_only_near_renders_no_prefilter() {
        let geo = near_query(1.0, 2.0, None, Some("1000"));
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        assert_statement_snapshot!("geo-predicate-a-min-distance-only-near-renders-no-prefilter", statement);
    }

    /// The instance subquery correlates to whichever alias the enclosing
    /// statement joined `entities` under, so the temporal path reaches the same
    /// rows the entity path does.
    #[test]
    fn an_instance_subquery_correlates_through_the_table_alias() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let statement = render(condition(&geo, &ALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert_statement_snapshot!("geo-predicate-an-instance-subquery-correlates-through-the-table-alias", statement);
    }

    #[test]
    fn an_entity_without_a_location_does_not_match_a_geo_filter() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        assert_statement_snapshot!("geo-predicate-an-entity-without-a-location-does-not-match-a-geo-filter", statement);
    }

    #[test]
    fn every_georel_renders_its_postgis_function_on_an_entity() {
        for (index, georel) in every_georel().into_iter().enumerate() {
            let geo = point_query(georel, LOCATION);
            assert_statement_snapshot!(
                format!("geo-predicate-every-georel-renders-its-postgis-function-on-an-entity-{}", index + 1),
                render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap())
            );
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
        let entity = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        let temporal = render(condition(&geo, &ALIASED, &UNNARROWED, &resolved_custom()).unwrap());
        let entity_sql = entity.to_string(PostgresQueryBuilder);
        let temporal_sql = temporal.to_string(PostgresQueryBuilder);
        assert_statement_snapshot!("geo-predicate-entity-and-temporal-render-the-same-geo-predicate-1", entity);
        assert_statement_snapshot!("geo-predicate-entity-and-temporal-render-the-same-geo-predicate-2", temporal);
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
        for georel in every_georel() {
            let flat = point_query(georel.clone(), LOCATION);
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

        let statement = render(condition(&geo, &UNALIASED, &UNNARROWED, &resolved_custom()).expect("renders"));
        assert_statement_snapshot!("geo-predicate-a-three-dimensional-reference-binds-as-three-dimensional-ewkb", statement);
    }

    /// Clause 5.7.2.4: any instance meeting the restriction is enough, so the
    /// matches of a custom `GeoProperty` are its instance rows that meet it,
    /// read with no outer row through the box `idx_ca_value_geometry` keys.
    #[test]
    fn the_matches_of_a_custom_geoproperty_are_its_instances_meeting_the_relation() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let statement = matches(&geo, &UNALIASED, &UNNARROWED, &resolved_custom())
            .expect("renders")
            .expect("a recorded GeoProperty can match");
        assert_statement_snapshot!(
            "geo-predicate-the-matches-of-a-custom-geoproperty-are-its-instances-meeting-the-relation",
            statement
        );
    }

    /// Clause 4.10: an IRI the dictionary has never seen is conveyed by no
    /// Entity, so there is nothing to read.
    #[test]
    fn an_unrecorded_geoproperty_has_no_matches() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        assert_eq!(matches(&geo, &UNALIASED, &UNNARROWED, &resolved_none()).expect("renders"), None);
    }

    /// Clause 5.7.2.4: the walked Entity matches when its hoisted default
    /// instance or one of its `datasetId` instances meets the restriction.
    /// Each is looked up by the walked key, the box compared as `box2d`.
    #[test]
    fn the_location_probe_tests_one_entitys_hoisted_instance_and_its_dataset_instances() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let statement = render(probe(&geo, &WALKED, &resolved_location()).expect("renders"));
        assert_statement_snapshot!(
            "geo-predicate-the-location-probe-tests-one-entitys-hoisted-instance-and-its-dataset-instances",
            statement
        );
    }

    /// A `location` the dictionary has never recorded has no instance rows,
    /// so the hoisted default instance decides alone.
    #[test]
    fn an_unrecorded_location_probe_tests_the_hoisted_instance_alone() {
        let geo = point_query(GeoRel::Within, LOCATION);
        let statement = render(probe(&geo, &WALKED, &resolved_none()).expect("renders"));
        assert_statement_snapshot!("geo-predicate-an-unrecorded-location-probe-tests-the-hoisted-instance-alone", statement);
    }

    #[test]
    fn a_custom_geoproperty_probe_tests_the_instances_of_one_entity() {
        let geo = point_query(GeoRel::Within, CUSTOM_GEOPROPERTY);
        let statement = render(probe(&geo, &WALKED, &resolved_custom()).expect("renders"));
        assert_statement_snapshot!("geo-predicate-a-custom-geoproperty-probe-tests-the-instances-of-one-entity", statement);
    }

    /// Clause 4.10: an Entity that does not convey the target `GeoProperty` is
    /// non-matching, and no Entity conveys one the dictionary never recorded.
    #[test]
    fn the_probe_of_an_unknown_geoproperty_matches_nothing() {
        let geo = point_query(GeoRel::Within, "https://example.com/neverRecorded");
        let statement = render(probe(&geo, &WALKED, &resolved_none()).expect("renders"));
        assert_statement_snapshot!("geo-predicate-the-probe-of-an-unknown-geoproperty-matches-nothing", statement);
    }

    /// The seven relations of clause 4.10, then `near` with a `minDistance`
    /// alone. `disjoint` and that `near` match geometries arbitrarily far from
    /// the reference, so their probes carry no box test.
    #[test]
    fn every_georel_renders_its_probe_on_one_entity() {
        let unbounded_near = GeoRel::Near {
            max_distance: None,
            min_distance: Some(PositiveDistance::from_str("100").expect("positive distance")),
        };
        for (index, georel) in every_georel().into_iter().chain([unbounded_near]).enumerate() {
            let geo = point_query(georel, LOCATION);
            assert_statement_snapshot!(
                format!("geo-predicate-every-georel-renders-its-probe-on-one-entity-{}", index + 1),
                render(probe(&geo, &WALKED, &resolved_location()).expect("renders"))
            );
        }
    }

    /// Clause 4.10 requires `near` to name at least one distance, whichever
    /// way the relation is read.
    #[test]
    fn the_probe_and_the_matches_refuse_near_without_a_distance() {
        let names = ResolvedNames::from_pairs([
            (name(LOCATION), AttributeNameKey::from(LOCATION_KEY)),
            (name(CUSTOM_GEOPROPERTY), AttributeNameKey::from(CUSTOM_GEOPROPERTY_ID)),
        ]);
        let unbounded = GeoRel::Near {
            max_distance: None,
            min_distance: None,
        };
        for geoproperty in [LOCATION, CUSTOM_GEOPROPERTY] {
            let geo = point_query(unbounded.clone(), geoproperty);
            let probed = probe(&geo, &WALKED, &names).expect_err("the probe must refuse");
            assert!(
                matches!(probed, RenderError::Query(QueryError::NearWithoutDistance)),
                "{geoproperty}: got {probed:?}"
            );
            let matched = matches(&geo, &UNALIASED, &UNNARROWED, &names).expect_err("the matches must refuse");
            assert!(
                matches!(matched, RenderError::Query(QueryError::NearWithoutDistance)),
                "{geoproperty}: got {matched:?}"
            );
        }
    }

    /// Clause 4.10 judges every relation on the horizontal plane, so the probe
    /// and the matches of a reference with an altitude are the statements of
    /// the same reference without one, binding the altitude they were given.
    #[test]
    fn a_three_dimensional_reference_renders_the_same_probe_and_matches_as_a_two_dimensional_one() {
        for georel in every_georel() {
            let flat = point_query(georel.clone(), LOCATION);
            let elevated = point_query_3d(georel, LOCATION);

            let (flat_probe, flat_probe_values) = render_parameterised(probe(&flat, &WALKED, &resolved_location()).expect("flat probe renders"));
            let (elevated_probe, elevated_probe_values) =
                render_parameterised(probe(&elevated, &WALKED, &resolved_location()).expect("elevated probe renders"));
            assert_eq!(
                elevated_probe, flat_probe,
                "{:?} probe must not vary with the reference dimensionality",
                flat.georel
            );
            assert_ne!(
                bound_bytes(&elevated_probe_values),
                bound_bytes(&flat_probe_values),
                "{:?} probe must still bind the altitude it was given",
                flat.georel
            );

            let matched = |geo: &GeoQuery| {
                matches(geo, &UNALIASED, &UNNARROWED, &resolved_location())
                    .expect("matches render")
                    .expect("location can match")
                    .build(PostgresQueryBuilder)
            };
            let (flat_matches, flat_matches_values) = matched(&flat);
            let (elevated_matches, elevated_matches_values) = matched(&elevated);
            assert_eq!(
                elevated_matches, flat_matches,
                "{:?} matches must not vary with the reference dimensionality",
                flat.georel
            );
            assert_ne!(
                bound_bytes(&elevated_matches_values),
                bound_bytes(&flat_matches_values),
                "{:?} matches must still bind the altitude they were given",
                flat.georel
            );
        }
    }
}
