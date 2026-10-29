//! A geo-query against every instance of an entity's default `location`.
//!
//! Clause 5.7.2.4: "if there are multiple instances of the `GeoProperty` on which
//! the geoquery is based, it is sufficient if any of these instances meets the
//! geospatial restrictions". `location` is held in two places. Its default
//! instance -- the one carrying no `datasetId`, or the first when every one
//! carries one (clause 4.5.16.1) -- is hoisted into the dedicated
//! `entity_search.location` columns, whose `GiST` indexes serve the common
//! single-instance entity. Every instance, the default one included, is also a
//! `current_attributes` row. The relation is therefore evaluated as the
//! union of two index-driven arms:
//!
//! - the hoisted default instance, through `idx_entity_search_location`, or,
//!   when an in-memory spatial index already answered for the default
//!   instance, through the candidates it named, looked up by identifier;
//! - every instance carrying a `datasetId`, through `idx_ca_value_geometry`.
//!
//! Each arm tests the box its index keys, the instance's great-circle reach
//! first, before the exact relation. The candidates' box test reads no index,
//! so the planner never scans the spatial index beside them; past a few dozen
//! candidates the scan reads less, and the arm drops them for it.
//!
//! An entity is in the answer when either arm finds one of its instances. The
//! outer statement reads the union once per entity and in `entity_key` order:
//! a wide geo-query matches tens of thousands of entities, and the outer
//! statement probes the entity tables' primary keys for each, which stay local
//! in key order and land at random in the hash order the planner otherwise
//! deduplicates them in. Each arm excludes an instance past its own clause 4.22
//! `expiresAt`: the hoisted arm through `entity_search.location_expires_at`,
//! the default instance's own expiry, and the instance arm through the row's
//! `expires_at`.

use crate::{
    attribute_name_key::AttributeNameKey,
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    geo_predicate::restriction,
    instance_expiry::instance_stands,
    stored_geometry::{BoxIndexing, StoredGeometry},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery};
use andromeda_postgresql_schema::current::{CurrentAttributes, Entities, EntitySearch};
use andromeda_postgresql_seaquery::{literal::false_literal, pg_array::is_any_of};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, JoinType, Order, Query, SelectStatement, UnionType};

/// Alias the hoisted-instance arm reads `entity_search` under.
const DEFAULT_ALIAS: &str = "geo_default";

/// Alias the hoisted-instance arm reads `entities` under when it is restricted
/// to an index's candidates.
const CANDIDATE_ALIAS: &str = "geo_candidate";

/// The most candidates the hoisted-instance arm looks up one by one. Each
/// candidate costs an identifier probe and a search-row probe, while the
/// spatial index returns the same rows from a few adjacent pages; past this
/// count the arm scans `idx_entity_search_location` for the query's box
/// instead.
const MAX_PROBED_CANDIDATES: usize = 64;

/// Alias the `datasetId`-instance arm reads `current_attributes` under.
const INSTANCE_ALIAS: &str = "geo_location_instance";

/// Alias the union of the arms is read under.
const MATCHED_ALIAS: &str = "geo_matched";

/// The entities one of whose `location` instances meets `geo`.
///
/// `location_key` is the `attribute_names` key of `location`, or `None` when
/// the dictionary has never recorded it, in which case no instance row exists
/// and only the hoisted arm is read.
///
/// # Errors
///
/// Returns [`RenderError`] exactly as [`restriction`] does.
pub fn any_location_instance(
    geo: &GeoQuery,
    rows: &EntityRowRefs,
    default_location: &DefaultLocationNarrowing,
    location_key: Option<AttributeNameKey>,
) -> Result<Condition, RenderError> {
    let mut arms = Vec::with_capacity(2);
    if let Some(arm) = default_instance_arm(geo, rows, default_location)? {
        arms.push(arm);
    }
    if let Some(key) = location_key {
        arms.push(dataset_instance_arm(geo, rows, key)?);
    }
    let mut arms = arms.into_iter();
    let Some(mut matching) = arms.next() else {
        // No arm can hold a matching instance: the index found no default
        // instance and the dictionary has never recorded `location`.
        return Ok(Condition::all().add(false_literal()));
    };
    for arm in arms {
        matching.union(UnionType::All, arm);
    }
    Ok(Condition::all().add(rows.entity_key_column().in_subquery(matched_in_key_order(matching))))
}

/// Each entity `matching` holds, once, in `entity_key` order.
fn matched_in_key_order(matching: SelectStatement) -> SelectStatement {
    let matched = Alias::new(MATCHED_ALIAS);
    Query::select()
        .distinct()
        .column((matched.clone(), EntitySearch::EntityKey)) // clone: the alias qualifies the column and names the subquery
        .from_subquery(matching, matched.clone())
        .order_by((matched, EntitySearch::EntityKey), Order::Asc)
        .take()
}

/// The entities whose hoisted default instance meets `geo`, or `None` when an
/// index already established that none does.
fn default_instance_arm(geo: &GeoQuery, rows: &EntityRowRefs, default_location: &DefaultLocationNarrowing) -> Result<Option<SelectStatement>, RenderError> {
    let search = Alias::new(DEFAULT_ALIAS);
    // The hoisted instance is stored already repaired.
    let stored = StoredGeometry::valid(
        SeaExpr::col((search.clone(), EntitySearch::LocationReach)), // clone: the alias qualifies both columns
        SeaExpr::col((search.clone(), EntitySearch::Location)),
    );
    let mut arm = Query::select();
    let indexing = match default_location {
        DefaultLocationNarrowing::Candidates(candidates) if candidates.is_empty() => return Ok(None),
        DefaultLocationNarrowing::Candidates(candidates) if candidates.len() <= MAX_PROBED_CANDIDATES => {
            let candidate = Alias::new(CANDIDATE_ALIAS);
            arm.column((candidate.clone(), Entities::EntityKey))
                .from_as(rows.table(Entities::Table), candidate.clone())
                .join_as(
                    JoinType::Join,
                    rows.table(EntitySearch::Table),
                    search.clone(),
                    SeaExpr::col((search, EntitySearch::EntityKey)).equals((candidate.clone(), Entities::EntityKey)),
                )
                .and_where(is_any_of(SeaExpr::col((candidate, Entities::Id)), candidates.iter().map(Identifier::to_string)));
            // The candidates drive the arm: an indexable box test would let the
            // planner scan the spatial index as well and join the two reads.
            BoxIndexing::Unindexed
        }
        DefaultLocationNarrowing::Unnarrowed | DefaultLocationNarrowing::Candidates(_) => {
            arm.column((search.clone(), EntitySearch::EntityKey))
                .from_as(rows.table(EntitySearch::Table), search);
            BoxIndexing::Indexed
        }
    };
    // Clause 4.22: the hoisted default instance matches only while it stands.
    arm.and_where(instance_stands(SeaExpr::col((Alias::new(DEFAULT_ALIAS), EntitySearch::LocationExpiresAt))))
        .cond_where(restriction(&geo.georel, &geo.geometry, &stored, indexing)?);
    Ok(Some(arm.take()))
}

/// The entities one of whose `location` instances carrying a `datasetId` meets
/// `geo`.
///
/// The hoisted instance of an entity whose every instance carries a
/// `datasetId` is read by both arms, and the entity is answered once.
fn dataset_instance_arm(geo: &GeoQuery, rows: &EntityRowRefs, location_key: AttributeNameKey) -> Result<SelectStatement, RenderError> {
    let instance = Alias::new(INSTANCE_ALIAS);
    // clone: the alias qualifies every column the arm reads
    let stored = StoredGeometry::written(
        SeaExpr::col((instance.clone(), CurrentAttributes::ValueGeometryReach)),
        SeaExpr::col((instance.clone(), CurrentAttributes::ValueGeometryValid)),
        SeaExpr::col((instance.clone(), CurrentAttributes::ValueGeometry)),
    );
    let mut arm = Query::select();
    arm.column((instance.clone(), CurrentAttributes::EntityKey))
        .from_as(rows.table(CurrentAttributes::Table), instance.clone())
        .and_where(SeaExpr::col((instance.clone(), CurrentAttributes::AttributeNameKey)).eq(location_key.get()))
        .and_where(SeaExpr::col((instance.clone(), CurrentAttributes::DatasetId)).is_not_null())
        .and_where(instance_stands(SeaExpr::col((instance.clone(), CurrentAttributes::ExpiresAt))))
        // The written column, which the partial `idx_ca_value_geometry` is
        // restricted by.
        .and_where(SeaExpr::col((instance, CurrentAttributes::ValueGeometry)).is_not_null());
    arm.cond_where(restriction(&geo.georel, &geo.geometry, &stored, BoxIndexing::Indexed)?);
    Ok(arm.take())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        entity_row_refs::EntityRowRefs,
        location_instances::{MAX_PROBED_CANDIDATES, any_location_instance},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_query::geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery};
    use andromeda_postgresql_schema::current::Entities;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Condition, PostgresQueryBuilder, Query, Values};
    use std::str::FromStr;

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";

    fn geo(georel: GeoRel) -> GeoQuery {
        GeoQuery {
            georel,
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [1.0, 2.0] })).expect("point parses"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        }
    }

    fn near(metres: &str) -> GeoRel {
        GeoRel::Near {
            max_distance: Some(PositiveDistance::from_str(metres).expect("positive distance")),
            min_distance: None,
        }
    }

    fn render(condition: Condition) -> (String, Values) {
        Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder)
    }

    fn unaliased() -> EntityRowRefs {
        EntityRowRefs::unaliased(None)
    }

    const LOCATION_KEY: i32 = 9;

    /// Clause 5.7.2.4: any instance meeting the restriction is enough, so the
    /// hoisted default instance and every `datasetId` instance are both arms of
    /// the one semi-join.
    #[test]
    fn every_location_instance_is_an_arm_of_one_semi_join() {
        let (sql, _) = render(
            any_location_instance(
                &geo(GeoRel::Within),
                &unaliased(),
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert!(
            sql.contains(r#""entities"."entity_key" IN (SELECT DISTINCT "geo_matched"."entity_key" FROM (SELECT "geo_default"."entity_key" FROM "entity_search" AS "geo_default" WHERE ("geo_default"."location_expires_at" IS NULL OR "geo_default"."location_expires_at" > NOW()) AND (COALESCE("geo_default"."location_reach", "geo_default"."location") && ST_MakeValid(ST_GeomFromEWKB($1))) AND _ST_Within("geo_default"."location","#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"UNION ALL (SELECT "geo_location_instance"."entity_key" FROM "current_attributes" AS "geo_location_instance" WHERE "geo_location_instance"."attribute_name_key" = $3 AND "geo_location_instance"."dataset_id" IS NOT NULL AND ("geo_location_instance"."expires_at" IS NULL OR "geo_location_instance"."expires_at" > NOW()) AND "geo_location_instance"."value_geometry" IS NOT NULL AND (COALESCE("geo_location_instance"."value_geometry_reach", "geo_location_instance"."value_geometry_valid", "geo_location_instance"."value_geometry") && ST_MakeValid(ST_GeomFromEWKB($4))) AND _ST_Within(COALESCE("geo_location_instance"."value_geometry_valid", "geo_location_instance"."value_geometry"),"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"("geo_default"."location_expires_at" IS NULL OR "geo_default"."location_expires_at" > NOW())"#),
            "the hoisted default instance stands only until its own expiry: {sql}"
        );
        assert!(
            sql.contains(r#"("geo_location_instance"."expires_at" IS NULL OR "geo_location_instance"."expires_at" > NOW())"#),
            "every dataset instance stands only until its own expiry: {sql}"
        );
        assert!(
            !sql.contains(r#""entity_search"."location_expires_at""#),
            "no entity-level expiry is mixed in: {sql}"
        );
    }

    /// A `location` the dictionary has never recorded has no instance rows,
    /// so only the hoisted arm is read.
    #[test]
    fn an_unrecorded_location_reads_the_hoisted_arm_alone() {
        let (sql, _) = render(any_location_instance(&geo(GeoRel::Within), &unaliased(), &DefaultLocationNarrowing::Unnarrowed, None).expect("renders"));
        assert!(sql.contains(r#"FROM "entity_search" AS "geo_default""#), "{sql}");
        assert!(!sql.contains("current_attributes"), "{sql}");
        assert!(!sql.contains("UNION"), "{sql}");
    }

    /// An index's candidates restrict the hoisted arm only, bound as one array;
    /// the `datasetId` instances are still evaluated in full.
    #[test]
    fn index_candidates_restrict_the_hoisted_arm_and_leave_the_dataset_instances_alone() {
        let candidates = DefaultLocationNarrowing::Candidates(vec!["urn:ngsi-ld:V:1".parse().expect("uri"), "urn:ngsi-ld:V:2".parse().expect("uri")]);
        let (sql, values) =
            render(any_location_instance(&geo(GeoRel::Within), &unaliased(), &candidates, Some(AttributeNameKey::from(LOCATION_KEY))).expect("renders"));
        assert!(
            sql.contains(r#"SELECT "geo_candidate"."entity_key" FROM "entities" AS "geo_candidate" JOIN "entity_search" AS "geo_default" ON "geo_default"."entity_key" = "geo_candidate"."entity_key" WHERE "geo_candidate"."id" = ANY($"#),
            "{sql}"
        );
        assert!(sql.contains(r#""geo_location_instance"."dataset_id" IS NOT NULL"#), "{sql}");
        let arrays = values.0.iter().filter(|value| matches!(value, sea_query::Value::Array(..))).count();
        assert_eq!(arrays, 1, "the candidates bind as one array: {sql}");
    }

    /// The candidates drive the hoisted arm, so its box test reads no spatial
    /// index the planner could scan beside them.
    #[test]
    fn index_candidates_test_the_hoisted_box_without_the_spatial_index() {
        let candidates = DefaultLocationNarrowing::Candidates(vec!["urn:ngsi-ld:V:1".parse().expect("uri")]);
        let (sql, _) = render(any_location_instance(&geo(near("1000")), &unaliased(), &candidates, None).expect("renders"));
        assert!(
            sql.contains(r#"COALESCE("geo_default"."location_reach", "geo_default"."location")::box2d && ST_MakeEnvelope("#),
            "{sql}"
        );
        assert!(
            !sql.contains(r#"COALESCE("geo_default"."location_reach", "geo_default"."location") && "#),
            "{sql}"
        );
        assert!(sql.contains(r#" AND _ST_DWithin("geo_default"."location"::geography,"#), "{sql}");
    }

    /// Past the probe cap the hoisted arm scans the spatial index for the box
    /// and binds no candidates.
    #[test]
    fn candidates_past_the_probe_cap_leave_the_hoisted_arm_to_the_spatial_index() {
        let many = (0..=MAX_PROBED_CANDIDATES)
            .map(|n| format!("urn:ngsi-ld:V:{n}").parse().expect("uri"))
            .collect();
        let (sql, values) =
            render(any_location_instance(&geo(near("1000")), &unaliased(), &DefaultLocationNarrowing::Candidates(many), None).expect("renders"));
        let (unnarrowed, _) = render(any_location_instance(&geo(near("1000")), &unaliased(), &DefaultLocationNarrowing::Unnarrowed, None).expect("renders"));
        assert_eq!(sql, unnarrowed);
        assert!(!values.0.iter().any(|value| matches!(value, sea_query::Value::Array(..))), "{sql}");
    }

    /// An index that found no default instance leaves the `datasetId`
    /// instances to decide alone -- the case of an entity whose only matching
    /// `location` carries a `datasetId`.
    #[test]
    fn an_empty_candidate_set_leaves_the_dataset_instances_to_decide() {
        let (sql, _) = render(
            any_location_instance(
                &geo(GeoRel::Within),
                &unaliased(),
                &DefaultLocationNarrowing::Candidates(Vec::new()),
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert!(!sql.contains("geo_default"), "{sql}");
        assert!(
            sql.contains(r#""entities"."entity_key" IN (SELECT DISTINCT "geo_matched"."entity_key" FROM (SELECT "geo_location_instance"."entity_key""#),
            "{sql}"
        );
    }

    /// The outer statement probes the entity tables once per matched entity,
    /// in key order, so a wide geo-query's probes stay local.
    #[test]
    fn the_matched_entities_are_read_once_each_in_key_order() {
        let (sql, _) = render(
            any_location_instance(
                &geo(near("1000000")),
                &unaliased(),
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert!(sql.contains(r#"IN (SELECT DISTINCT "geo_matched"."entity_key" FROM (SELECT "#), "{sql}");
        assert!(sql.ends_with(r#") AS "geo_matched" ORDER BY "geo_matched"."entity_key" ASC)"#), "{sql}");
    }

    /// Nothing can match when the index found no default instance and no
    /// instance row exists.
    #[test]
    fn no_arm_at_all_matches_nothing() {
        let (sql, _) =
            render(any_location_instance(&geo(GeoRel::Within), &unaliased(), &DefaultLocationNarrowing::Candidates(Vec::new()), None).expect("renders"));
        assert!(sql.ends_with("WHERE FALSE"), "{sql}");
    }

    /// `near` measures both arms cast to `geography`, which no index keys, so
    /// each arm is prefiltered by the box its geometry `GiST` keys, the
    /// great-circle reach first.
    #[test]
    fn near_prefilters_both_arms_by_their_reach_box_and_measures_on_the_spheroid() {
        let (sql, _) = render(
            any_location_instance(
                &geo(near("1000")),
                &unaliased(),
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert!(
            sql.contains(r#"COALESCE("geo_default"."location_reach", "geo_default"."location") && ST_MakeEnvelope("#),
            "{sql}"
        );
        assert!(sql.contains(r#" AND _ST_DWithin("geo_default"."location"::geography,"#), "{sql}");
        assert!(!sql.contains("geom_to_geog"), "{sql}");
        assert!(
            sql.contains(r#"COALESCE("geo_location_instance"."value_geometry_reach", "geo_location_instance"."value_geometry_valid", "geo_location_instance"."value_geometry") && ST_MakeEnvelope("#),
            "{sql}"
        );
        assert!(
            sql.contains(r#" AND _ST_DWithin(COALESCE("geo_location_instance"."value_geometry_valid", "geo_location_instance"."value_geometry")::geography,"#),
            "{sql}"
        );
    }

    /// A reference by the antimeridian reaches both its sides, so the
    /// `datasetId` instances are prefiltered by both halves of the envelope.
    #[test]
    fn the_dataset_instance_arm_prefilters_both_sides_of_the_antimeridian() {
        let geo = GeoQuery {
            georel: near("300"),
            geometry: serde_json::from_value::<Geometry>(serde_json::json!({ "type": "Point", "coordinates": [179.999, -17.0] })).expect("point parses"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        };
        let (sql, _) = render(
            any_location_instance(
                &geo,
                &unaliased(),
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert_eq!(
            sql.matches(r#"COALESCE("geo_location_instance"."value_geometry_reach", "geo_location_instance"."value_geometry_valid", "geo_location_instance"."value_geometry") && ST_MakeEnvelope("#)
                .count(),
            2,
            "{sql}"
        );
    }

    /// The arms read the statement's branch schema, so a Snapshot query never
    /// reaches the live relations.
    #[test]
    fn both_arms_read_the_statements_branch() {
        let rows = EntityRowRefs::aliased(Some(Alias::new("snap_7").into()), Alias::new("e"), Alias::new("s"));
        let (sql, _) = render(
            any_location_instance(
                &geo(GeoRel::Within),
                &rows,
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert!(sql.contains(r#"FROM "snap_7"."entity_search" AS "geo_default""#), "{sql}");
        assert!(sql.contains(r#"FROM "snap_7"."current_attributes" AS "geo_location_instance""#), "{sql}");
        assert!(sql.contains(r#""e"."entity_key" IN ("#), "{sql}");
    }
}
