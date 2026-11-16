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
    error::RenderError,
    history_source::{HistoryBranch, PendingEntries},
    query_names::QueryNames,
    stored_geometry::{BoxIndexing, GeometryBox, box_test},
};
use andromeda_postgresql_schema::temporal::TemporalAttributeStreams;
use andromeda_postgresql_seaquery::branch::qualified;
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Expr, ExprTrait, Query, SelectStatement, SimpleExpr, UnionType};

/// How many Evolutions a geo page walks before it reads the candidates.
///
/// An Evolution's geo probe costs about 5 µs, so the walk costs about a
/// quarter of a millisecond, a fortieth of reading the candidates to the
/// bound. It fills the page only for a geo-query most Evolutions meet. Among
/// 1 000 000 Evolutions, one that seven in ten meet is answered in 0.9 ms,
/// where reading the candidates first costs 12 ms.
pub(super) const EXTENT_SHORT_WALK: u32 = 50;

/// The most streams a page reads before it walks instead.
///
/// A stream costs about 4 µs to read and narrow the walk to, so the bound's
/// streams cost about 40 ms among 1 000 000 Evolutions. Past the bound the
/// page walks, which trades the two placements of the matches that no count
/// tells apart. 20 000 streams spread through identifier order cost 19 ms
/// with the walk, the read that stopped at the bound included, against 70 ms
/// read in full, while 20 000 streams holding the last identifiers make the
/// walk probe every Evolution, 5.4 s against 30 ms read in full.
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
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        extent_candidates::ExtentRead,
        query_fixture::{geo_query, ids, near_query, opened, point, recorded_stage1, sensor_names, stage1_on},
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
    use andromeda_postgresql_query::{history_source::HistoryBranch, query_names::QueryNames};
    use andromeda_postgresql_seaquery::{assert_statement_snapshot, statement_planning::StatementPlanning};
    use andromeda_postgresql_stored_values::{entity_type_name_key::EntityTypeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, SelectStatement, Value, Values};
    use std::str::FromStr;

    /// The page of `query` over the `Sensor` walk.
    fn type_page(query: &CompiledTemporalQuery) -> SelectStatement {
        recorded_stage1(query).select
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
        assert_eq!(stage1.planning, StatementPlanning::GenericGatedBranches);
        let statement = stage1.select;
        assert_statement_snapshot!("extent-candidates-a-near-query-over-a-type-walk-reads-the-stream-extents", statement);
    }

    #[test]
    fn the_near_envelope_is_the_entity_geo_querys() {
        let compiled = near_query(4.5, "2000");
        let read = ExtentRead::of(&compiled, &sensor_names()).expect("renders").expect("a near box");
        let single = read.candidates(&HistoryBranch::live_recorded());
        assert_statement_snapshot!("extent-candidates-the-near-envelope-is-the-entity-geo-querys-1", single);
        let (_, values) = single.build(PostgresQueryBuilder);
        assert_eq!(doubles(&values), envelope_bounds(4.5, "2000"));

        let compiled = near_query(179.99, "2000");
        let read = ExtentRead::of(&compiled, &sensor_names()).expect("renders").expect("a near box");
        let split = read.candidates(&HistoryBranch::live_recorded());
        assert_statement_snapshot!("extent-candidates-the-near-envelope-is-the-entity-geo-querys-2", split);
        let (_, values) = split.build(PostgresQueryBuilder);
        assert_eq!(doubles(&values), envelope_bounds(179.99, "2000"));
    }

    #[test]
    fn the_extent_test_names_the_indexed_expression() {
        let statement = type_page(&near_query(4.5, "2000"));
        assert_statement_snapshot!("extent-candidates-the-extent-test-names-the-indexed-expression", statement);
    }

    #[test]
    fn an_area_relation_reads_the_reference_bounding_box() {
        let within = type_page(&geo_query(GeoRel::Within, &serde_json::json!([4.5, 52.5])));
        assert_statement_snapshot!("extent-candidates-an-area-relation-reads-the-reference-bounding-box-1", within);
        let intersects = type_page(&geo_query(GeoRel::Intersects, &serde_json::json!([4.5, 52.5])));
        assert_statement_snapshot!("extent-candidates-an-area-relation-reads-the-reference-bounding-box-2", intersects);
        let contains = type_page(&geo_query(GeoRel::Contains, &serde_json::json!([4.5, 52.5])));
        assert_statement_snapshot!("extent-candidates-an-area-relation-reads-the-reference-bounding-box-3", contains);
        let overlaps = type_page(&geo_query(GeoRel::Overlaps, &serde_json::json!([4.5, 52.5])));
        assert_statement_snapshot!("extent-candidates-an-area-relation-reads-the-reference-bounding-box-4", overlaps);
        let equals = type_page(&geo_query(GeoRel::Equals, &serde_json::json!([4.5, 52.5])));
        assert_statement_snapshot!("extent-candidates-an-area-relation-reads-the-reference-bounding-box-5", equals);
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
        let disjoint = type_page(&opened(geo_query(GeoRel::Disjoint, &serde_json::json!([4.5, 52.5]))));
        assert_statement_snapshot!("extent-candidates-a-disjoint-or-minimum-distance-query-reads-no-extents-1", disjoint);
        let minimum = type_page(&opened(far));
        assert_statement_snapshot!("extent-candidates-a-disjoint-or-minimum-distance-query-reads-no-extents-2", minimum);
    }

    #[test]
    fn the_exact_relation_stays_the_residual() {
        let statement = type_page(&near_query(4.5, "2000"));
        assert_statement_snapshot!("extent-candidates-the-exact-relation-stays-the-residual", statement);
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
        let statement = stage1.select;
        assert_statement_snapshot!("extent-candidates-an-unrecorded-geoproperty-reads-no-extents", statement);
    }

    #[test]
    fn a_geo_page_walks_fifty_evolutions_first() {
        let statement = type_page(&near_query(4.5, "2000"));
        assert_statement_snapshot!("extent-candidates-a-geo-page-walks-fifty-evolutions-first", statement);
    }

    #[test]
    fn a_queued_branch_admits_every_queued_entity_for_its_geometry() {
        let statement = stage1_on(&HistoryBranch::live(), &near_query(4.5, "2000"), &PageWindow::offset(20, 0)).select;
        assert_statement_snapshot!("extent-candidates-a-queued-branch-admits-every-queued-entity-for-its-geometry", statement);
    }

    #[test]
    fn a_snapshot_reads_its_own_extent_index() {
        let statement = stage1_on(
            &HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            &near_query(4.5, "2000"),
            &PageWindow::offset(20, 0),
        )
        .select;
        assert_statement_snapshot!("extent-candidates-a-snapshot-reads-its-own-extent-index", statement);
    }

    /// Clause 5.7.4.4 applies the geoquery of a split query only to the
    /// aggregated Entities, so a contributed Entity is a candidate whatever
    /// its local extents hold, and the residual still exempts it.
    #[test]
    fn a_contributed_entity_is_a_candidate_whatever_its_geometry() {
        let mut compiled = near_query(4.5, "2000");
        compiled.contributed = ContributedEntities::new(ids(&["urn:ngsi-ld:Sensor:000009"]), ContributedRendering::Local);
        let statement = type_page(&compiled);
        assert_statement_snapshot!("extent-candidates-a-contributed-entity-is-a-candidate-whatever-its-geometry", statement);
    }

    #[test]
    fn a_counted_geo_query_counts_under_the_count_bound() {
        let stage1 = recorded_stage1(&near_query(4.5, "2000"));
        assert_eq!(stage1.count_planning, StatementPlanning::BoundValues);
        let statement = stage1.count.expect("count requested");
        assert_statement_snapshot!("extent-candidates-a-counted-geo-query-counts-under-the-count-bound", statement);
    }
}
