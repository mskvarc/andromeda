//! The candidates of a temporal geo query (clause 4.10): the Evolutions
//! holding a stream of the `GeoProperty` whose extent meets the box the
//! relation admits, read from the partial extent index.
//!
//! Clause 5.7.4.4 step S3 keeps an Entity one of whose in-window instances of
//! the `GeoProperty` meets the relation. Every geometry a stream holds lies
//! inside the stream's extent, so a stream whose extent misses the box holds
//! none that can meet it, whatever the window. The extents are a superset:
//! they never shrink, and they ignore the window, so the per-Entity probe
//! rechecks every candidate with the window and the exact relation. The box
//! test reads the extent column bare, the expression
//! `idx_temporal_attribute_streams_value_extent` holds.
//!
//! Clause 5.7.4.4 applies the geoquery of a split query only once the parts
//! from the Context Sources are aggregated, so an Entity a source contributed
//! is a candidate whatever its local extents hold.

use crate::{
    alias::Rs,
    stage1::{
        contributed_candidates::contributed_candidates,
        queued_candidates::{queued_candidates, stream_holder},
    },
};
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, contributed_entities::ContributedEntities};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::query_names::QueryNames,
    error::RenderError,
    history_source::{HistoryBranch, PendingEntries},
    stored_geometry::{BoxIndexing, GeometryBox, box_test},
};
use andromeda_postgresql_schema::temporal::TemporalAttributeStreams;
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Expr, ExprTrait, Query, SelectStatement, SimpleExpr, UnionType};

/// How many Evolutions a geo page walks before it reads the candidates: an
/// Evolution's geo probe costs about what the candidate read costs at its
/// bound divided by fifty.
pub(super) const EXTENT_SHORT_WALK: u32 = 50;

/// The most streams a page reads before it walks instead.
pub(super) const EXTENT_CANDIDATE_BOUND: u64 = 10_000;

/// The extent read a temporal geo query admits.
#[derive(Debug, Clone)]
pub(super) struct ExtentRead<'q> {
    /// The dictionary key of the `GeoProperty` the query names.
    geoproperty: AttributeNameKey,
    /// The box test over the stream extent under `rs`.
    overlap: SimpleExpr,
    /// The Entities a Context Source contributed to a split query.
    contributed: &'q ContributedEntities,
}

impl<'q> ExtentRead<'q> {
    /// The extent read of `compiled`: `None` without a geoquery, for a
    /// `GeoProperty` the dictionary never keyed, which no stream records, and
    /// for a relation that admits no box.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::ReferenceGeometryEncoding`] when the reference
    /// geometry has no EWKB spelling the store can bind.
    pub(super) fn of(compiled: &'q CompiledTemporalQuery, names: &QueryNames) -> Result<Option<ExtentRead<'q>>, RenderError> {
        let Some(geo) = &compiled.entities.geo else {
            return Ok(None);
        };
        let Some(geoproperty) = names.attributes.get(&geo.geoproperty) else {
            return Ok(None);
        };
        Ok(box_test(
            &geo.georel,
            &geo.geometry,
            &GeometryBox::stream_extent(Expr::col((Rs, TemporalAttributeStreams::ValueExtent))),
            BoxIndexing::Indexed,
        )?
        .map(|overlap| ExtentRead {
            geoproperty,
            overlap,
            contributed: &compiled.contributed,
        }))
    }

    /// The Evolutions of `branch` holding a stream of the `GeoProperty` whose
    /// extent meets the box, beside every Entity a Context Source contributed:
    /// their surrogate keys over recorded rows alone; where the branch reads
    /// its queue, their identifiers beside every queued Entity, whose queued
    /// geometries no extent holds.
    pub(super) fn candidates(&self, branch: &HistoryBranch) -> SelectStatement {
        let mut candidates = Query::select();
        candidates.from(qualified(branch.schema(), TemporalAttributeStreams::Table).alias(Rs));
        stream_holder(&mut candidates, branch);
        candidates
            .and_where(Expr::col((Rs, TemporalAttributeStreams::AttributeNameKey)).eq(self.geoproperty.get()))
            // clone: the page and the count each read the candidates
            .and_where(self.overlap.clone());
        if let Some(contributed) = contributed_candidates(branch, self.contributed) {
            candidates.union(UnionType::All, contributed);
        }
        match branch.pending() {
            PendingEntries::Absent => {}
            PendingEntries::Unioned => {
                candidates.union(UnionType::All, queued_candidates(branch));
            }
        }
        candidates
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        extent_candidates::ExtentRead,
        query_fixture::{CountParts, PageBranches, geo_query, ids, near_query, opened, point, recorded_stage1, sensor_names, stage1_on},
    };
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, great_circle::great_circle_bounds, near_envelope::NearEnvelope, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        page_window::PageWindow,
        temporal::{
            compiled::CompiledTemporalQuery,
            contributed_entities::{ContributedEntities, ContributedRendering},
        },
    };
    use andromeda_postgresql_query::{
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        entity_type_name_key::EntityTypeNameKey,
        history_source::HistoryBranch,
    };
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Value, Values};
    use std::str::FromStr;

    /// The candidates' comparison of the `GeoProperty`'s key, up to its
    /// placeholder.
    const GEOPROPERTY_KEY: &str = r#""rs"."attribute_name_key" = $"#;

    /// The rendered page of `query` over the `Sensor` walk.
    fn type_page(query: &CompiledTemporalQuery) -> String {
        recorded_stage1(query).select.to_string(PostgresQueryBuilder)
    }

    /// The value bound to the placeholder right after the first `fragment`.
    fn bound_after(sql: &str, values: &Values, fragment: &str) -> Option<Value> {
        let start = sql.find(fragment)? + fragment.len();
        let digits: String = sql[start..].chars().take_while(char::is_ascii_digit).collect();
        let position = digits.parse::<usize>().ok()?.checked_sub(1)?;
        values.0.get(position).cloned()
    }

    /// The doubles `values` binds, in order.
    fn doubles(values: &Values) -> Vec<f64> {
        values
            .0
            .iter()
            .filter_map(|value| if let Value::Double(Some(double)) = value { Some(*double) } else { None })
            .collect()
    }

    /// The four bounds of each rectangle of the near envelope of `metres`
    /// around `longitude`, 52.5.
    fn envelope_bounds(longitude: f64, metres: &str) -> Vec<f64> {
        let reference = great_circle_bounds(&point(&serde_json::json!([longitude, 52.5])).planar()).expect("a point has bounds");
        NearEnvelope::around(reference, PositiveDistance::from_str(metres).expect("positive distance"))
            .rects()
            .iter()
            .flat_map(|rect| [rect.min().x, rect.min().y, rect.max().x, rect.max().y])
            .collect()
    }

    #[test]
    fn a_near_query_over_a_type_walk_reads_the_stream_extents() {
        let stage1 = recorded_stage1(&near_query(4.5, "2000"));
        assert_eq!(stage1.planning, StatementPlanning::GatedBranches);
        let (sql, values) = stage1.select.build(PostgresQueryBuilder);
        let candidates = PageBranches::of(&sql).candidates;
        assert!(
            candidates.starts_with(r#"SELECT "rs"."entity_key" FROM "temporal_attribute_streams" AS "rs" WHERE "rs"."attribute_name_key" = $"#),
            "{candidates}"
        );
        assert_eq!(bound_after(candidates, &values, GEOPROPERTY_KEY), Some(Value::Int(Some(2))), "{sql}");
        assert!(candidates.contains(r#" AND ("rs"."value_extent" && ST_MakeEnvelope("#), "{candidates}");
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_001))), "{:?}", values.0);
    }

    #[test]
    fn the_near_envelope_is_the_entity_geo_querys() {
        for (longitude, rectangles) in [(4.5, 1), (179.99, 2)] {
            let compiled = near_query(longitude, "2000");
            let read = ExtentRead::of(&compiled, &sensor_names()).expect("renders").expect("a near box");
            let (sql, values) = read.candidates(&HistoryBranch::live_recorded()).build(PostgresQueryBuilder);
            assert_eq!(doubles(&values), envelope_bounds(longitude, "2000"), "{longitude}: {sql}");
            assert_eq!(sql.matches(r#""rs"."value_extent" && ST_MakeEnvelope("#).count(), rectangles, "{sql}");
            if rectangles == 2 {
                assert!(sql.contains(r#")) OR ("rs"."value_extent" && ST_MakeEnvelope("#), "{sql}");
            }
        }
    }

    #[test]
    fn the_extent_test_names_the_indexed_expression() {
        let sql = type_page(&near_query(4.5, "2000"));
        let candidates = PageBranches::of(&sql).candidates;
        assert!(candidates.contains(r#" AND ("rs"."value_extent" && ST_MakeEnvelope("#), "{candidates}");
        assert!(
            !candidates.contains("::box2d") && !candidates.contains(r#"("rs"."value_extent")"#),
            "{candidates}"
        );
    }

    #[test]
    fn an_area_relation_reads_the_reference_bounding_box() {
        for (name, georel) in [
            ("within", GeoRel::Within),
            ("intersects", GeoRel::Intersects),
            ("contains", GeoRel::Contains),
            ("overlaps", GeoRel::Overlaps),
            ("equals", GeoRel::Equals),
        ] {
            let sql = type_page(&geo_query(georel, &serde_json::json!([4.5, 52.5])));
            let candidates = PageBranches::of(&sql).candidates;
            assert!(
                candidates.contains(r#" AND ("rs"."value_extent" && ST_MakeValid(ST_GeomFromEWKB("#),
                "{name}: {candidates}"
            );
        }
    }

    /// Neither relation admits a box, so the query takes the step of the
    /// window clause 5.7.4.4 makes every query carry, and the relation stays
    /// in the probe.
    #[test]
    fn a_disjoint_or_minimum_distance_query_reads_no_extents() {
        let far = geo_query(
            GeoRel::Near {
                max_distance: None,
                min_distance: Some(PositiveDistance::from_str("10").expect("positive distance")),
            },
            &serde_json::json!([4.5, 52.5]),
        );
        for compiled in [opened(geo_query(GeoRel::Disjoint, &serde_json::json!([4.5, 52.5]))), opened(far)] {
            let sql = type_page(&compiled);
            let branches = PageBranches::of(&sql);
            assert!(!sql.contains("value_extent"), "{sql}");
            assert!(branches.candidates.contains(r#""evolution_active_until" >= "#), "{sql}");
            assert!(branches.indexed.contains(r#""qh"."value_geometry""#), "{sql}");
        }
    }

    #[test]
    fn the_exact_relation_stays_the_residual() {
        let sql = type_page(&near_query(4.5, "2000"));
        let branches = PageBranches::of(&sql);
        for residual in [branches.short, branches.indexed, branches.walk] {
            assert!(
                residual.contains(r#" _ST_DWithin(COALESCE("qh"."value_geometry_valid", "qh"."value_geometry")::geography"#),
                "{residual}"
            );
        }
    }

    /// No stream records a `GeoProperty` the dictionary never keyed, so the
    /// query reads no extent and takes the window's step.
    #[test]
    fn an_unrecorded_geoproperty_reads_no_extents() {
        let types_alone = QueryNames {
            attributes: ResolvedNames::default(),
            types: ResolvedNames::from_pairs([(
                "https://uri.etsi.org/ngsi-ld/default-context/Sensor".parse().expect("valid type name"),
                EntityTypeNameKey::from(1),
            )]),
            ..QueryNames::default()
        };
        let stage1 = build_stage1_entity_ids(
            &HistoryBranch::live_recorded(),
            &opened(near_query(4.5, "2000")),
            &PageWindow::offset(20, 0),
            CountPolicy::Emit,
            &types_alone,
        )
        .expect("valid temporal query");
        let sql = stage1.select.to_string(PostgresQueryBuilder);
        assert!(!sql.contains("value_extent"), "{sql}");
        assert!(PageBranches::of(&sql).candidates.contains(r#""evolution_active_until" >= "#), "{sql}");
    }

    #[test]
    fn a_geo_page_walks_fifty_evolutions_first() {
        let sql = type_page(&near_query(4.5, "2000"));
        let branches = PageBranches::of(&sql);
        assert!(branches.short.contains("LIMIT 50 OFFSET 0"), "{}", branches.short);
        assert!(branches.walk.contains(r#"(SELECT COUNT(*) FROM "candidates") > 10000"#), "{}", branches.walk);
    }

    #[test]
    fn a_queued_branch_admits_every_queued_entity_for_its_geometry() {
        let sql = stage1_on(&HistoryBranch::live(), &near_query(4.5, "2000"), &PageWindow::offset(20, 0))
            .select
            .to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(
            branches.candidates.starts_with(concat!(
                r#"SELECT "ri"."id" FROM "temporal_attribute_streams" AS "rs" JOIN "entities" AS "ri" ON "ri"."entity_key" = "rs"."entity_key" "#,
                r#"WHERE "rs"."attribute_name_key" = 2 AND"#
            )),
            "{}",
            branches.candidates
        );
        assert!(
            branches
                .candidates
                .ends_with(r#" UNION ALL (SELECT "entity_id" FROM "temporal_append_intents")"#),
            "{}",
            branches.candidates
        );
        assert!(
            branches.indexed.contains(r#""e"."id" IN (SELECT "id" FROM "candidates")"#),
            "{}",
            branches.indexed
        );
    }

    #[test]
    fn a_snapshot_reads_its_own_extent_index() {
        let sql = stage1_on(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &near_query(4.5, "2000"),
            &PageWindow::offset(20, 0),
        )
        .select
        .to_string(PostgresQueryBuilder);
        assert!(
            PageBranches::of(&sql)
                .candidates
                .contains(r#"FROM "snap_7"."temporal_attribute_streams" AS "rs" WHERE"#),
            "{sql}"
        );
    }

    /// Clause 5.7.4.4 applies the geoquery of a split query only to the
    /// aggregated Entities, so a contributed Entity is a candidate whatever
    /// its local extents hold, and the residual still exempts it.
    #[test]
    fn a_contributed_entity_is_a_candidate_whatever_its_geometry() {
        let mut compiled = near_query(4.5, "2000");
        compiled.contributed = ContributedEntities::new(ids(&["urn:ngsi-ld:Sensor:000009"]), ContributedRendering::Local);
        let sql = type_page(&compiled);
        let branches = PageBranches::of(&sql);
        assert!(
            branches.candidates.ends_with(concat!(
                r#" UNION ALL (SELECT "entities"."entity_key" FROM "entities""#,
                r#" WHERE "entities"."id" = 'urn:ngsi-ld:Sensor:000009' AND "entities"."evolution_created_at" IS NOT NULL)"#
            )),
            "{}",
            branches.candidates
        );
        assert!(
            branches.indexed.contains(r#" OR "e"."id" = 'urn:ngsi-ld:Sensor:000009'"#),
            "{}",
            branches.indexed
        );
    }

    #[test]
    fn a_counted_geo_query_counts_under_the_count_bound() {
        let stage1 = recorded_stage1(&near_query(4.5, "2000"));
        assert_eq!(stage1.count_planning, StatementPlanning::BoundValues);
        let (sql, values) = stage1.count.expect("count requested").build(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"WITH "candidates" AS  MATERIALIZED (SELECT "rs"."entity_key" FROM "temporal_attribute_streams" AS "rs""#),
            "{sql}"
        );
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_001))), "{:?}", values.0);
        assert!(values.0.contains(&Value::BigUnsigned(Some(10_000))), "{:?}", values.0);
        let parts = CountParts::of(&sql);
        assert!(
            parts.indexed.contains(r#""entity_key" IN (SELECT "entity_key" FROM "candidates")"#),
            "{}",
            parts.indexed
        );
        assert!(parts.walked.contains(r#"(SELECT COUNT(*) FROM "candidates") > $"#), "{}", parts.walked);
        assert_eq!(parts.walked.matches("candidates").count(), 2, "the two gates alone: {}", parts.walked);
        assert!(parts.filtered.contains(" _ST_DWithin("), "{}", parts.filtered);
        assert!(!parts.indexed.contains("ST_DWithin(") && !parts.walked.contains("ST_DWithin("), "{sql}");
        assert!(!sql.contains("CASE WHEN"), "{sql}");
    }
}
