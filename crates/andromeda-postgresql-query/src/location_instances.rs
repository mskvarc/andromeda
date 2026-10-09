//! A geo-query against every instance of an entity's default `location`.
//!
//! Clause 5.7.2.4: "if there are multiple instances of the `GeoProperty` on which
//! the geoquery is based, it is sufficient if any of these instances meets the
//! geospatial restrictions". `location` is held in two places. Its default
//! instance -- the one carrying no `datasetId`, or the first when every one
//! carries one (clause 4.5.16.1) -- is hoisted into the dedicated
//! `entity_search.location` columns, whose `GiST` indexes serve the common
//! single-instance entity. Every instance, the default one included, is also a
//! `current_attribute_values` row. The relation is therefore evaluated as the
//! union of two index-driven arms:
//!
//! - the hoisted default instance, through `idx_entity_search_location` (or its
//!   `geography` twin for `near`), restricted to the index's candidates when an
//!   in-memory spatial index already answered for the default instance;
//! - every instance carrying a `datasetId`, through `idx_cav_value_geometry`.
//!
//! An entity is in the answer when either arm finds one of its instances, and
//! the outer statement reaches the union as a semi-join, so the planner may
//! drive either from the geo-query or from the enclosing statement's other
//! predicates. Each arm excludes an instance past its own clause 4.22
//! `expiresAt`: the hoisted arm through `entity_search.location_expires_at`,
//! the default instance's own expiry, and the instance arm through the row's
//! `expires_at`.

use crate::{
    attribute_name_key::AttributeNameKey,
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    geo_predicate::{near_envelope_prefilter, relation_expression},
    instance_expiry::instance_stands,
};
use andromeda_ngsi_ld_query::geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery};
use andromeda_postgresql_schema::current::{CurrentAttributeValues, Entities, EntitySearch};
use andromeda_postgresql_seaquery::{
    cast::cast,
    literal::false_literal,
    pg_array::{any, text_array},
    pg_type::PostgresType,
};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, JoinType, Query, SelectStatement, SimpleExpr, UnionType};

/// Alias the hoisted-instance arm reads `entity_search` under.
const DEFAULT_ALIAS: &str = "geo_default";

/// Alias the hoisted-instance arm reads `entities` under when it is restricted
/// to an index's candidates.
const CANDIDATE_ALIAS: &str = "geo_candidate";

/// Alias the `datasetId`-instance arm reads `current_attribute_values` under.
const INSTANCE_ALIAS: &str = "geo_location_instance";

/// The entities one of whose `location` instances meets `geo`.
///
/// `location_key` is the `attribute_names` key of `location`, or `None` when
/// the dictionary has never recorded it, in which case no instance row exists
/// and only the hoisted arm is read.
///
/// # Errors
///
/// Returns [`RenderError`] exactly as [`relation_expression`] does.
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
    Ok(Condition::all().add(rows.entity_key_column().in_subquery(matching)))
}

/// The entities whose hoisted default instance meets `geo`, or `None` when an
/// index already established that none does.
fn default_instance_arm(geo: &GeoQuery, rows: &EntityRowRefs, default_location: &DefaultLocationNarrowing) -> Result<Option<SelectStatement>, RenderError> {
    let search = Alias::new(DEFAULT_ALIAS);
    let spatial = relation_expression(
        &geo.georel,
        SeaExpr::col((search.clone(), EntitySearch::Location)),
        SeaExpr::col((search.clone(), EntitySearch::LocationGeography)),
        &geo.geometry,
    )?;
    let mut arm = Query::select();
    match default_location {
        DefaultLocationNarrowing::Unnarrowed => {
            arm.column((search.clone(), EntitySearch::EntityKey))
                .from_as(rows.table(EntitySearch::Table), search);
        }
        DefaultLocationNarrowing::Candidates(candidates) if candidates.is_empty() => return Ok(None),
        DefaultLocationNarrowing::Candidates(candidates) => {
            let candidate = Alias::new(CANDIDATE_ALIAS);
            let ids: Vec<&str> = candidates.iter().map(AsRef::as_ref).collect();
            arm.column((candidate.clone(), Entities::EntityKey))
                .from_as(rows.table(Entities::Table), candidate.clone())
                .join_as(
                    JoinType::Join,
                    rows.table(EntitySearch::Table),
                    search.clone(),
                    SeaExpr::col((search, EntitySearch::EntityKey)).equals((candidate.clone(), Entities::EntityKey)),
                )
                // One bound `text[]` however many candidates the index found.
                .and_where(SeaExpr::col((candidate, Entities::Id)).eq(any(text_array(&ids))));
        }
    }
    // Clause 4.22: the hoisted default instance matches only while it stands.
    arm.and_where(instance_stands(SeaExpr::col((Alias::new(DEFAULT_ALIAS), EntitySearch::LocationExpiresAt))))
        .and_where(spatial);
    Ok(Some(arm.take()))
}

/// The entities one of whose `location` instances carrying a `datasetId` meets
/// `geo`.
///
/// The hoisted instance of an entity whose every instance carries a
/// `datasetId` is read by both arms, which the semi-join answers once.
fn dataset_instance_arm(geo: &GeoQuery, rows: &EntityRowRefs, location_key: AttributeNameKey) -> Result<SelectStatement, RenderError> {
    let instance = Alias::new(INSTANCE_ALIAS);
    let geometry: SimpleExpr = SeaExpr::col((instance.clone(), CurrentAttributeValues::ValueGeometry));
    let mut arm = Query::select();
    arm.column((instance.clone(), CurrentAttributeValues::EntityKey))
        .from_as(rows.table(CurrentAttributeValues::Table), instance.clone())
        .and_where(SeaExpr::col((instance.clone(), CurrentAttributeValues::AttributeNameKey)).eq(location_key.get()))
        .and_where(SeaExpr::col((instance.clone(), CurrentAttributeValues::DatasetId)).is_not_null())
        .and_where(instance_stands(SeaExpr::col((instance, CurrentAttributeValues::ExpiresAt))))
        .and_where(SeaExpr::expr(geometry.clone()).is_not_null()); // clone: the column is read by the presence test and the relation
    if let Some(prefilter) = near_envelope_prefilter(geo, &geometry) {
        arm.and_where(prefilter);
    }
    arm.and_where(relation_expression(
        &geo.georel,
        geometry.clone(), // clone: the relation reads the column planar and cast
        cast(geometry, PostgresType::Geography),
        &geo.geometry,
    )?);
    Ok(arm.take())
}

#[cfg(test)]
mod tests {
    use crate::{attribute_name_key::AttributeNameKey, entity_row_refs::EntityRowRefs, location_instances::any_location_instance};
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
            sql.contains(r#""entities"."entity_key" IN (SELECT "geo_default"."entity_key" FROM "entity_search" AS "geo_default" WHERE ("geo_default"."location_expires_at" IS NULL OR "geo_default"."location_expires_at" > NOW()) AND ST_Within("geo_default"."location","#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"UNION ALL (SELECT "geo_location_instance"."entity_key" FROM "current_attribute_values" AS "geo_location_instance" WHERE "geo_location_instance"."attribute_name_key" = $2 AND "geo_location_instance"."dataset_id" IS NOT NULL AND ("geo_location_instance"."expires_at" IS NULL OR "geo_location_instance"."expires_at" > NOW()) AND "geo_location_instance"."value_geometry" IS NOT NULL AND ST_Within("geo_location_instance"."value_geometry","#),
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
        assert!(!sql.contains("current_attribute_values"), "{sql}");
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
            sql.contains(r#""entities"."entity_key" IN (SELECT "geo_location_instance"."entity_key""#),
            "{sql}"
        );
    }

    /// Nothing can match when the index found no default instance and no
    /// instance row exists.
    #[test]
    fn no_arm_at_all_matches_nothing() {
        let (sql, _) =
            render(any_location_instance(&geo(GeoRel::Within), &unaliased(), &DefaultLocationNarrowing::Candidates(Vec::new()), None).expect("renders"));
        assert!(sql.ends_with("WHERE FALSE"), "{sql}");
    }

    /// `near` reads the hoisted `geography` column, and prefilters the
    /// `datasetId` instances by a bounding box the geometry `GiST` answers,
    /// since a cast column reaches no index of its own.
    #[test]
    fn near_reads_the_geography_column_and_prefilters_the_dataset_instances_by_their_box() {
        let (sql, _) = render(
            any_location_instance(
                &geo(near("1000")),
                &unaliased(),
                &DefaultLocationNarrowing::Unnarrowed,
                Some(AttributeNameKey::from(LOCATION_KEY)),
            )
            .expect("renders"),
        );
        assert!(sql.contains(r#"ST_DWithin("geo_default"."location_geography","#), "{sql}");
        assert!(sql.contains(r#""geo_location_instance"."value_geometry" && ST_MakeEnvelope("#), "{sql}");
        assert!(sql.contains(r#"ST_DWithin("geo_location_instance"."value_geometry"::geography,"#), "{sql}");
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
            sql.matches(r#""geo_location_instance"."value_geometry" && ST_MakeEnvelope("#).count(),
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
        assert!(sql.contains(r#"FROM "snap_7"."current_attribute_values" AS "geo_location_instance""#), "{sql}");
        assert!(sql.contains(r#""e"."entity_key" IN ("#), "{sql}");
    }
}
