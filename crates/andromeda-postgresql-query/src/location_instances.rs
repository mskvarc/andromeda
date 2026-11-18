//! A geo-query against every instance of an entity's default `location`.
//!
//! Clause 5.7.2.4: "if there are multiple instances of the `GeoProperty` on which
//! the geoquery is based, it is sufficient if any of these instances meets the
//! geospatial restrictions". `location` is held in two places. Its default
//! instance (the one carrying no `datasetId`, or the first when every one
//! carries one, clause 4.5.16.1) is hoisted into the dedicated
//! `entity_search.location` columns, whose `GiST` indexes serve the common
//! single-instance entity. Every instance, the default one included, is also a
//! `current_attributes` row. The relation is therefore evaluated as the
//! union of two index-driven arms:
//!
//! - the hoisted default instance, through `idx_entity_search_location`, or,
//!   when an in-memory spatial index already answered for the default
//!   instance, through the candidates it named, looked up by identifier;
//! - every instance carrying a `datasetId`, through
//!   `idx_ca_dataset_value_geometry`, which holds no other instance: a box
//!   that holds thousands of default instances reads none of them twice.
//!
//! Each arm tests the box its index keys, the instance's great-circle reach
//! first, before the exact relation. The candidates' box test reads no index,
//! so the planner never scans the spatial index beside them. An index names a
//! few dozen candidates at most
//! ([`LocationCandidates::MAX`](andromeda_ngsi_ld_query::geo::default_location_narrowing::LocationCandidates::MAX)),
//! since past them the scan reads less.
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
//!
//! The arms are read three ways. [`any_location_instance`] is the semi-join a
//! statement filters by. [`location_matches`] is the union alone, for a page
//! that reads the matching entities as candidates before it reads the entities
//! themselves. [`location_probe`] tests one entity's instances, looked up by
//! its key: the hoisted default instance through the search row's primary key,
//! the `datasetId` instances through `idx_ca_entity_geometry`. An arm and its
//! probe add one set of predicates, so the two cannot judge an instance
//! differently. The probe tests the box without its index, since an indexable
//! box test lets the planner AND a geometry `GiST` into each per-entity
//! primary-key lookup. It reads no in-memory narrowing either: the narrowing
//! only names the entities the store re-verifies, and the probe judges the
//! entity's own instance exactly.

use crate::{
    attribute_instance_correlation::{existence_marker, instance_stored_geometry},
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    geo_predicate::restriction,
    standing_instant::instant_of,
    stored_geometry::{BoxIndexing, StoredGeometry},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery},
    standing_instant::StandingInstant,
};
use andromeda_postgresql_schema::current::{CurrentAttributes, Entities, EntitySearch};
use andromeda_postgresql_seaquery::{literal::false_literal, pg_array::is_any_of};
use andromeda_postgresql_stored_form::{default_instance::is_named_instance, expiry::unexpired};
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, JoinType, Order, Query, SelectStatement, UnionType};

/// Alias the hoisted-instance arm reads `entity_search` under.
const DEFAULT_ALIAS: &str = "geo_default";

/// Alias the hoisted-instance arm reads `entities` under when it is restricted
/// to an index's candidates.
const CANDIDATE_ALIAS: &str = "geo_candidate";

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
    let Some(matching) = location_matches(geo, rows, default_location, location_key)? else {
        // No arm can hold a matching instance: the index found no default
        // instance and the dictionary has never recorded `location`.
        return Ok(Condition::all().add(false_literal()));
    };
    Ok(Condition::all().add(rows.entity_key_column().in_subquery(matched_in_key_order(matching))))
}

/// The `entity_key` of every entity one of whose `location` instances meets
/// `geo`, once per matching instance and in no order: the `UNION ALL` of the
/// arms, or `None` when no arm can hold a match because the index found no
/// default instance and the dictionary has never recorded `location`.
///
/// `rows` names the branch and the instant the arms read; nothing correlates
/// to it. `location_key` is as [`any_location_instance`] takes it.
///
/// # Errors
///
/// Returns [`RenderError`] exactly as [`restriction`] does.
pub fn location_matches(
    geo: &GeoQuery,
    rows: &EntityRowRefs,
    default_location: &DefaultLocationNarrowing,
    location_key: Option<AttributeNameKey>,
) -> Result<Option<SelectStatement>, RenderError> {
    let hoisted = default_instance_arm(geo, rows, default_location)?;
    let named = location_key.map(|key| dataset_instance_arm(geo, rows, key)).transpose()?;
    Ok(hoisted.into_iter().chain(named).reduce(|mut matching, arm| {
        matching.union(UnionType::All, arm);
        matching
    }))
}

/// Whether one of the `location` instances of the entity `rows` names meets
/// `geo`: its hoisted default instance, or one of its `datasetId` instances
/// when the dictionary has recorded `location` under `location_key`.
///
/// # Errors
///
/// Returns [`RenderError`] exactly as [`restriction`] does.
pub fn location_probe(geo: &GeoQuery, rows: &EntityRowRefs, location_key: Option<AttributeNameKey>) -> Result<Condition, RenderError> {
    let hoisted = default_instance_probe(geo, rows)?;
    let named = location_key.map(|key| dataset_instance_probe(geo, rows, key)).transpose()?;
    Ok(Condition::any().add(SeaExpr::exists(hoisted)).add_option(named.map(SeaExpr::exists)))
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
    let mut arm = Query::select();
    let indexing = match default_location {
        DefaultLocationNarrowing::Candidates(candidates) if candidates.is_empty() => return Ok(None),
        DefaultLocationNarrowing::Candidates(candidates) => {
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
        DefaultLocationNarrowing::Unnarrowed => {
            arm.column((search.clone(), EntitySearch::EntityKey))
                .from_as(rows.table(EntitySearch::Table), search);
            BoxIndexing::Indexed
        }
    };
    restrict_to_meeting_default_instances(&mut arm, geo, rows.standing(), indexing)?;
    Ok(Some(arm.take()))
}

/// `SELECT 1` when the hoisted default instance of the entity `rows` names
/// meets `geo`, read by its key.
fn default_instance_probe(geo: &GeoQuery, rows: &EntityRowRefs) -> Result<SelectStatement, RenderError> {
    let search = Alias::new(DEFAULT_ALIAS);
    let mut probe = Query::select();
    probe
        .expr(existence_marker())
        .from_as(rows.table(EntitySearch::Table), search.clone()) // clone: the alias names the relation and qualifies the correlation
        .and_where(SeaExpr::col((search, EntitySearch::EntityKey)).eq(rows.entity_key_column()));
    restrict_to_meeting_default_instances(&mut probe, geo, rows.standing(), BoxIndexing::Unindexed)?;
    Ok(probe.take())
}

/// Restrict `scan`, which reads `entity_search` as the hoisted-instance arm's
/// alias, to the hoisted default instances that stand at `standing` and meet
/// `geo`.
fn restrict_to_meeting_default_instances(
    scan: &mut SelectStatement,
    geo: &GeoQuery,
    standing: StandingInstant,
    indexing: BoxIndexing,
) -> Result<(), RenderError> {
    let search = Alias::new(DEFAULT_ALIAS);
    // The hoisted instance is stored already repaired.
    let stored = StoredGeometry::valid(
        SeaExpr::col((search.clone(), EntitySearch::LocationReach)), // clone: the alias qualifies every column
        SeaExpr::col((search.clone(), EntitySearch::Location)),
    );
    // Clause 4.22: the hoisted default instance matches only while it stands.
    scan.and_where(unexpired(SeaExpr::col((search, EntitySearch::LocationExpiresAt)), instant_of(standing)))
        .cond_where(restriction(&geo.georel, &geo.geometry, &stored, indexing)?);
    Ok(())
}

/// The entities one of whose `location` instances carrying a `datasetId` meets
/// `geo`.
///
/// The hoisted instance of an entity whose every instance carries a
/// `datasetId` is read by both arms, and the entity is answered once.
fn dataset_instance_arm(geo: &GeoQuery, rows: &EntityRowRefs, location_key: AttributeNameKey) -> Result<SelectStatement, RenderError> {
    let instance = Alias::new(INSTANCE_ALIAS);
    let mut arm = Query::select();
    arm.column((instance.clone(), CurrentAttributes::EntityKey)) // clone: the alias qualifies the column and names the relation
        .from_as(rows.table(CurrentAttributes::Table), instance);
    restrict_to_meeting_dataset_instances(&mut arm, geo, location_key, rows.standing(), BoxIndexing::Indexed)?;
    Ok(arm.take())
}

/// `SELECT 1` when one of the `location` instances carrying a `datasetId` of
/// the entity `rows` names meets `geo`, read by its key.
fn dataset_instance_probe(geo: &GeoQuery, rows: &EntityRowRefs, location_key: AttributeNameKey) -> Result<SelectStatement, RenderError> {
    let instance = Alias::new(INSTANCE_ALIAS);
    let mut probe = Query::select();
    probe
        .expr(existence_marker())
        .from_as(rows.table(CurrentAttributes::Table), instance.clone()) // clone: the alias names the relation and qualifies the correlation
        .and_where(SeaExpr::col((instance, CurrentAttributes::EntityKey)).eq(rows.entity_key_column()));
    restrict_to_meeting_dataset_instances(&mut probe, geo, location_key, rows.standing(), BoxIndexing::Unindexed)?;
    Ok(probe.take())
}

/// Restrict `scan`, which reads `current_attributes` as the
/// `datasetId`-instance arm's alias, to the `location` instances carrying a
/// `datasetId` that stand at `standing` and meet `geo`.
fn restrict_to_meeting_dataset_instances(
    scan: &mut SelectStatement,
    geo: &GeoQuery,
    location_key: AttributeNameKey,
    standing: StandingInstant,
    indexing: BoxIndexing,
) -> Result<(), RenderError> {
    let instance = Alias::new(INSTANCE_ALIAS);
    scan.and_where(SeaExpr::col((instance.clone(), CurrentAttributes::AttributeNameKey)).eq(location_key.get())) // clone: the alias qualifies every column
        .and_where(is_named_instance(SeaExpr::col((instance.clone(), CurrentAttributes::DatasetKey))))
        .and_where(unexpired(SeaExpr::col((instance.clone(), CurrentAttributes::ExpiresAt)), instant_of(standing)))
        // With the `datasetId` test, the predicate the partial
        // `idx_ca_dataset_value_geometry` is restricted by.
        .and_where(SeaExpr::col((instance, CurrentAttributes::ValueGeometry)).is_not_null())
        .cond_where(restriction(&geo.georel, &geo.geometry, &instance_stored_geometry(INSTANCE_ALIAS), indexing)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_row_refs::EntityRowRefs,
        location_instances::{any_location_instance, location_matches},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::geo::{
        default_location_narrowing::{DefaultLocationNarrowing, LocationCandidates},
        query::GeoQuery,
    };
    use andromeda_postgresql_schema::current::Entities;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Condition, Query, SelectStatement};
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

    fn render(condition: Condition) -> SelectStatement {
        Query::select().column(Entities::Id).from(Entities::Table).cond_where(condition).to_owned()
    }

    fn unaliased() -> EntityRowRefs {
        EntityRowRefs::unaliased(None)
    }

    /// The narrowing an index hands over when it found `candidates`.
    fn narrowed_to(candidates: Vec<Identifier>) -> DefaultLocationNarrowing {
        DefaultLocationNarrowing::Candidates(LocationCandidates::new(candidates).expect("within the bound"))
    }

    const LOCATION_KEY: i32 = 9;

    /// Clause 5.7.2.4: any instance meeting the restriction is enough, so the
    /// hoisted default instance and every `datasetId` instance are both arms of
    /// the one semi-join.
    #[test]
    fn every_location_instance_is_an_arm_of_one_semi_join() {
        let statement = render(
            any_location_instance(
                &geo(GeoRel::Within),
                &unaliased(),
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert_statement_snapshot!("location-instances-every-location-instance-is-an-arm-of-one-semi-join", statement);
    }

    /// A `location` the dictionary has never recorded has no instance rows,
    /// so only the hoisted arm is read.
    #[test]
    fn an_unrecorded_location_reads_the_hoisted_arm_alone() {
        let statement = render(any_location_instance(&geo(GeoRel::Within), &unaliased(), &DefaultLocationNarrowing::Unnarrowed, None).expect("renders"));
        assert_statement_snapshot!("location-instances-an-unrecorded-location-reads-the-hoisted-arm-alone", statement);
    }

    /// An index's candidates restrict the hoisted arm only, bound as one array;
    /// the `datasetId` instances are still evaluated in full.
    #[test]
    fn index_candidates_restrict_the_hoisted_arm_and_leave_the_dataset_instances_alone() {
        let candidates = narrowed_to(vec!["urn:ngsi-ld:V:1".parse().expect("uri"), "urn:ngsi-ld:V:2".parse().expect("uri")]);
        let statement =
            render(any_location_instance(&geo(GeoRel::Within), &unaliased(), &candidates, Some(AttributeNameKey::from(LOCATION_KEY))).expect("renders"));
        assert_statement_snapshot!(
            "location-instances-index-candidates-restrict-the-hoisted-arm-and-leave-the-dataset-instances-alone",
            statement
        );
    }

    /// The candidates drive the hoisted arm, so its box test reads no spatial
    /// index the planner could scan beside them.
    #[test]
    fn index_candidates_test_the_hoisted_box_without_the_spatial_index() {
        let candidates = narrowed_to(vec!["urn:ngsi-ld:V:1".parse().expect("uri")]);
        let statement = render(any_location_instance(&geo(near("1000")), &unaliased(), &candidates, None).expect("renders"));
        assert_statement_snapshot!("location-instances-index-candidates-test-the-hoisted-box-without-the-spatial-index", statement);
    }

    /// An index that found no default instance leaves the `datasetId`
    /// instances to decide alone, which is the case of an entity whose only matching
    /// `location` carries a `datasetId`.
    #[test]
    fn an_empty_candidate_set_leaves_the_dataset_instances_to_decide() {
        let statement = render(
            any_location_instance(
                &geo(GeoRel::Within),
                &unaliased(),
                &narrowed_to(Vec::new()),
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert_statement_snapshot!("location-instances-an-empty-candidate-set-leaves-the-dataset-instances-to-decide", statement);
    }

    /// The outer statement probes the entity tables once per matched entity,
    /// in key order, so a wide geo-query's probes stay local.
    #[test]
    fn the_matched_entities_are_read_once_each_in_key_order() {
        let statement = render(
            any_location_instance(
                &geo(near("1000000")),
                &unaliased(),
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert_statement_snapshot!("location-instances-the-matched-entities-are-read-once-each-in-key-order", statement);
    }

    /// Nothing can match when the index found no default instance and no
    /// instance row exists.
    #[test]
    fn no_arm_at_all_matches_nothing() {
        let statement = render(any_location_instance(&geo(GeoRel::Within), &unaliased(), &narrowed_to(Vec::new()), None).expect("renders"));
        assert_statement_snapshot!("location-instances-no-arm-at-all-matches-nothing", statement);
    }

    /// `near` measures both arms cast to `geography`, which no index keys, so
    /// each arm is prefiltered by the box its geometry `GiST` keys, the
    /// great-circle reach first.
    #[test]
    fn near_prefilters_both_arms_by_their_reach_box_and_measures_on_the_spheroid() {
        let statement = render(
            any_location_instance(
                &geo(near("1000")),
                &unaliased(),
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert_statement_snapshot!(
            "location-instances-near-prefilters-both-arms-by-their-reach-box-and-measures-on-the-spheroid",
            statement
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
        let statement = render(
            any_location_instance(
                &geo,
                &unaliased(),
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert_statement_snapshot!(
            "location-instances-the-dataset-instance-arm-prefilters-both-sides-of-the-antimeridian",
            statement
        );
    }

    /// The arms read the statement's branch schema, so a Snapshot query never
    /// reaches the live relations.
    #[test]
    fn both_arms_read_the_statements_branch() {
        let rows = EntityRowRefs::aliased(Some(Alias::new("snap_7").into()), Alias::new("e"), Alias::new("s"));
        let statement = render(
            any_location_instance(
                &geo(GeoRel::Within),
                &rows,
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert_statement_snapshot!("location-instances-both-arms-read-the-statements-branch", statement);
    }

    /// The matches are the arms alone, read through the spatial indexes: a
    /// page reading them as candidates bounds them itself and needs neither
    /// one row per entity nor an order.
    #[test]
    fn the_location_matches_are_every_arm_unioned() {
        let statement = location_matches(
            &geo(GeoRel::Within),
            &unaliased(),
            &DefaultLocationNarrowing::Unnarrowed,
            Some(AttributeNameKey::from(LOCATION_KEY)),
        )
        .expect("renders")
        .expect("both arms can hold a match");
        assert_statement_snapshot!("location-instances-the-location-matches-are-every-arm-unioned", statement);
    }

    /// A `location` the dictionary has never recorded has no instance rows,
    /// so its matches are the hoisted arm's.
    #[test]
    fn the_location_matches_of_an_unrecorded_location_are_its_hoisted_arm() {
        let statement = location_matches(&geo(GeoRel::Within), &unaliased(), &DefaultLocationNarrowing::Unnarrowed, None)
            .expect("renders")
            .expect("the hoisted arm can hold a match");
        assert_statement_snapshot!(
            "location-instances-the-location-matches-of-an-unrecorded-location-are-its-hoisted-arm",
            statement
        );
    }

    /// An index that found no default instance, beside a `location` the
    /// dictionary has never recorded, leaves no arm to read.
    #[test]
    fn no_arm_has_no_location_matches() {
        let matches = location_matches(&geo(GeoRel::Within), &unaliased(), &narrowed_to(Vec::new()), None).expect("renders");
        assert_eq!(matches, None);
    }
}
