//! The instance a stream held when a temporal interval opened.
//!
//! Clause 4.11 makes a temporal query select the values "valid within the
//! specified timeframe", and Annex C clause C.5.5 relies on the value observed
//! before the window and never replaced still being one of them. That value is
//! the stream's latest instance placed at or before the start: an instance is
//! placed at its instant on the requested Temporal Property, and a deletion
//! that carries no instant there is placed at the instant it was deleted
//! (clause 4.5.7), so an Attribute deleted before the interval opens holds
//! nothing in it.
//!
//! The latest placement is the greatest of descending probes, one per
//! placement rule, each reading a single index entry per chunk:
//!
//! - the latest instance whose axis instant is at or before the start, on the
//!   stream-led index of the axis;
//! - the latest deletion with no axis instant at or before the start, on the
//!   stream-led deletion index.
//!
//! That is exactly `MAX(COALESCE(axis, deleted_at))` over the rows placed at or
//! before the start: `COALESCE` takes the axis instant when there is one and
//! the deletion instant otherwise, a row with neither places nothing, and
//! `GREATEST` ignores the probe that found no row. The aggregate form cannot be
//! answered from any index and has to be evaluated once per scanned instance,
//! which makes the reading quadratic in the length of the stream.
//!
//! The recorded probes read the stream by its key, the recorded rows a queued
//! entry replaces left out. On the live branch the queued entries of the
//! stream's grain are placed by the same two rules beside them, as an
//! aggregate over the few entries the queue holds for the Entity.

use crate::{
    history_source::{
        HistoryBranch,
        HistoryOf,
        attribute_rows::{kept_beside_pending, pending_attribute_rows},
        grain::HistoryGrain,
    },
    q_predicate::history_window::HistoryAliases,
};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{branch::qualified, pg_operator::PgBinaryOperator};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Func, Iden, Order, Query, QueryStatementBuilder, SelectStatement, SimpleExpr};
use typed_builder::TypedBuilder;

/// The column of the placement source holding the instant the stream's
/// in-effect instance is placed at.
#[derive(Iden, Clone, Copy)]
#[iden = "placed"]
struct Placed;

/// The instance one stream held when an interval with a start opened.
#[derive(Debug, Clone, TypedBuilder)]
pub(crate) struct InEffectInstance<'a> {
    /// The branch whose history the instance is placed in.
    branch: &'a HistoryBranch,
    /// The `attribute_history` column holding the requested Temporal Property.
    axis: AttributeHistory,
    /// The instant the interval opens at.
    start: DateTime<Utc>,
}

impl InEffectInstance<'_> {
    /// The `LATERAL` source placing the stream's in-effect instance:
    /// `SELECT GREATEST(<latest instance>, <latest deletion>, ...) AS placed`.
    ///
    /// On the `deletedAt` axis every deletion carries its instant on the axis
    /// itself, so the first probe already finds it and no deletion probe is read.
    pub(crate) fn placement_source(&self, aliases: &HistoryAliases) -> SelectStatement {
        let instances = || Condition::all();
        let mut placements = vec![self.recorded_placement(aliases.prior(), self.axis, instances(), aliases)];
        placements.extend(self.queued_placement(aliases.queued_prior(), self.axis, instances(), aliases));
        if self.axis != AttributeHistory::DeletedAt {
            let without_instant = |relation: Alias| Condition::all().add(SeaExpr::col((relation, self.axis)).is_null());
            placements.push(self.recorded_placement(aliases.deletion(), AttributeHistory::DeletedAt, without_instant(aliases.deletion()), aliases));
            placements.extend(self.queued_placement(
                aliases.queued_deletion(),
                AttributeHistory::DeletedAt,
                without_instant(aliases.queued_deletion()),
                aliases,
            ));
        }
        let placed: SimpleExpr = match <[SimpleExpr; 1]>::try_from(placements) {
            Ok([only]) => only,
            Err(placements) => Func::greatest(placements).into(),
        };
        Query::select().expr_as(placed, Placed).to_owned()
    }

    /// The latest instant on `column` at or before the start among the
    /// stream's recorded rows that pass `placed_by`, read under `relation` as
    /// one descending probe.
    fn recorded_placement(&self, relation: Alias, column: AttributeHistory, placed_by: Condition, aliases: &HistoryAliases) -> SimpleExpr {
        let mut latest = Query::select();
        latest
            .expr(SeaExpr::col((relation.clone(), column)))
            .from(qualified(self.branch.schema(), AttributeHistory::Table).alias(relation.clone()))
            .and_where(SeaExpr::col((relation.clone(), AttributeHistory::StreamKey)).eq(aliases.stream_column(HistoryGrain::StreamKey)))
            .cond_where(placed_by)
            .and_where(SeaExpr::col((relation.clone(), column)).lte(self.start));
        if let Some(kept) = kept_beside_pending(
            self.branch.pending(),
            aliases.stream_column(HistoryGrain::EntityId),
            aliases.stream_column(HistoryGrain::AttributeNameKey),
            SeaExpr::col((relation.clone(), AttributeHistory::VerbatimInstanceId)),
        ) {
            latest.cond_where(kept);
        }
        latest.order_by((relation, column), Order::Desc).limit(1);
        SimpleExpr::SubQuery(None, Box::new(latest.into_sub_query_statement()))
    }

    /// The latest instant on `column` at or before the start among the queued
    /// entries of the stream's grain that pass `placed_by`, read under
    /// `relation`, or `None` where the branch reads no queue.
    fn queued_placement(&self, relation: Alias, column: AttributeHistory, placed_by: Condition, aliases: &HistoryAliases) -> Option<SimpleExpr> {
        let entries = pending_attribute_rows(self.branch, &HistoryOf::Entity(aliases.stream_column(HistoryGrain::EntityId)), relation.clone())?;
        let mut latest = Query::select();
        latest
            .expr(Func::max(SeaExpr::col((relation.clone(), column))))
            .from(entries)
            .and_where(SeaExpr::col((relation.clone(), HistoryGrain::AttributeNameKey)).eq(aliases.stream_column(HistoryGrain::AttributeNameKey)))
            .and_where(
                SimpleExpr::from(SeaExpr::col((relation.clone(), HistoryGrain::DatasetId))).not_distinct_from(aliases.stream_column(HistoryGrain::DatasetId)),
            )
            .cond_where(placed_by)
            .and_where(SeaExpr::col((relation, column)).lte(self.start));
        Some(SimpleExpr::SubQuery(None, Box::new(latest.into_sub_query_statement())))
    }

    /// The instant the in-effect instance is placed at, as the placement
    /// source exposes it.
    fn placed(aliases: &HistoryAliases) -> SimpleExpr {
        SeaExpr::col((aliases.placement(), Placed))
    }

    /// The instances of the stream placed at exactly the placement, read from
    /// the attribute source under the alias the placed instance is read from.
    ///
    /// Every instance sharing the instant is kept, so a repeated write at one
    /// instant offers each of its values, as clause 4.9 asks of a term matched
    /// against "any Value of such instances".
    pub(crate) fn placed_on_stream(&self, aliases: &HistoryAliases, placed_instance: &Alias) -> Condition {
        let column = |grain: HistoryGrain| SeaExpr::col((placed_instance.clone(), grain));
        Condition::all()
            .add(column(HistoryGrain::AttributeNameKey).eq(aliases.stream_column(HistoryGrain::AttributeNameKey)))
            .add(SimpleExpr::from(column(HistoryGrain::DatasetId)).not_distinct_from(aliases.stream_column(HistoryGrain::DatasetId)))
            .add(SeaExpr::col((placed_instance.clone(), self.axis)).eq(Self::placed(aliases)))
    }

    /// Whether the placement lies strictly before the start.
    ///
    /// An instance placed at the start itself lies inside the interval, which
    /// clause 4.11 opens inclusively, so it is the window's own and nothing
    /// before it is in effect; a stream with no placement at all places NULL,
    /// which no comparison admits.
    pub(crate) fn opens_before_start(&self, aliases: &HistoryAliases) -> SimpleExpr {
        SeaExpr::expr(Self::placed(aliases)).lt(self.start)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        history_source::HistoryBranch,
        q_predicate::{history_window::HistoryAliases, in_effect_instance::InEffectInstance},
    };
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Condition, Expr, IntoIden, JoinType, PostgresQueryBuilder, Query, SimpleExpr};

    fn start() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2019-01-01T00:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
    }

    /// The placement source, the instance at the placement and the strict
    /// start test, rendered as the probe arranges them, on the branch
    /// `schema` names.
    fn render_on(branch: &HistoryBranch, axis: AttributeHistory, aliases: &HistoryAliases) -> String {
        let instance = InEffectInstance::builder().branch(branch).axis(axis).start(start()).build();
        Query::select()
            .expr(Expr::val(1))
            .from_as(Alias::new("streams"), aliases.stream())
            .join_lateral(JoinType::Join, instance.placement_source(aliases), aliases.placement(), SimpleExpr::from(true))
            .from_as(Alias::new("rows"), aliases.instance())
            .cond_where(instance.placed_on_stream(aliases, &aliases.instance()))
            .cond_where(Condition::all().add(instance.opens_before_start(aliases)))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    fn render(axis: AttributeHistory, aliases: &HistoryAliases) -> String {
        render_on(&HistoryBranch::live(), axis, aliases)
    }

    /// Clause 4.11: the in-effect instance is the latest at or before the
    /// start, found by one descending probe on the axis.
    #[test]
    fn the_placement_seeks_the_latest_observation_at_or_before_the_start() {
        let sql = render(AttributeHistory::ObservedAt, &HistoryAliases::top_level());
        for fragment in [
            r#"FROM "attribute_history" AS "qp" WHERE "qp"."stream_key" = "qs"."stream_key""#,
            r#""qp"."observed_at" <= '2019-01-01"#,
            r#"ORDER BY "qp"."observed_at" DESC LIMIT 1"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }

    /// Clause 4.5.7: a deletion with no instant on the axis is placed at the
    /// instant it was deleted, found by its own descending probe.
    #[test]
    fn the_placement_seeks_the_latest_deletion_that_carries_no_axis_instant() {
        let sql = render(AttributeHistory::ObservedAt, &HistoryAliases::top_level());
        for fragment in [
            r#""qd"."observed_at" IS NULL AND "qd"."deleted_at" <= '2019-01-01"#,
            r#"ORDER BY "qd"."deleted_at" DESC LIMIT 1"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }

    /// On a Snapshot branch the placement is the greater of the two recorded
    /// probes, exposed as `placed`, with no aggregate.
    #[test]
    fn the_placement_is_the_greater_of_the_two() {
        let snapshot = HistoryBranch::recorded_in(Alias::new("snap_1").into_iden());
        let sql = render_on(&snapshot, AttributeHistory::ObservedAt, &HistoryAliases::top_level());
        for fragment in [r#"GREATEST((SELECT "qp"."observed_at""#, r#"AS "placed""#, "JOIN LATERAL (SELECT GREATEST("] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        assert!(!sql.contains("MAX(") && !sql.contains("COALESCE("), "{sql}");
        assert!(!sql.contains("temporal_append"), "{sql}");
    }

    /// On the live branch the queued entries of the stream's grain are placed
    /// by the same two rules, and a recorded row a queued entry replaces is
    /// not placed.
    #[test]
    fn the_live_placement_also_places_the_queued_entries_of_the_grain() {
        let sql = render(AttributeHistory::ObservedAt, &HistoryAliases::top_level());
        for fragment in [
            r#"(SELECT MAX("qpq"."observed_at") FROM (SELECT"#,
            r#""pe"."entity_id" = "qs"."entity_id""#,
            r#""qpq"."attribute_name_key" = "qs"."attribute_name_key" AND ("qpq"."dataset_id" IS NOT DISTINCT FROM "qs"."dataset_id")"#,
            r#"(SELECT MAX("qdq"."deleted_at")"#,
            r#""qp"."verbatim_instance_id" IS NULL OR (NOT EXISTS(SELECT 1 FROM "temporal_append_entries" AS "pr" WHERE "pr"."entity_id" = "qs"."entity_id""#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }

    /// On the `deletedAt` axis a deletion carries its instant on the axis, so
    /// the first probe already places it and no deletion probe is read.
    #[test]
    fn a_deleted_at_window_places_no_separate_deletion_arm() {
        let sql = render(AttributeHistory::DeletedAt, &HistoryAliases::top_level());
        assert!(!sql.contains(r#""qd""#) && !sql.contains(r#""qdq""#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "qp"."deleted_at" DESC LIMIT 1"#), "{sql}");
        let snapshot = HistoryBranch::recorded_in(Alias::new("snap_1").into_iden());
        let recorded = render_on(&snapshot, AttributeHistory::DeletedAt, &HistoryAliases::top_level());
        assert!(!recorded.contains("GREATEST("), "{recorded}");
    }

    /// The instances of the stream's grain placed at exactly the placed
    /// instant are the ones read.
    #[test]
    fn the_instance_is_read_at_the_placed_instant_of_its_grain() {
        let sql = render(AttributeHistory::ObservedAt, &HistoryAliases::top_level());
        assert!(
            sql.contains(
                r#""qh"."attribute_name_key" = "qs"."attribute_name_key" AND ("qh"."dataset_id" IS NOT DISTINCT FROM "qs"."dataset_id") AND "qh"."observed_at" = "qe"."placed""#
            ),
            "{sql}"
        );
    }

    /// Clause 4.11 opens the interval inclusively: an instance placed at the
    /// start itself belongs to the window, so the in-effect test is strict.
    #[test]
    fn a_placement_at_the_start_itself_admits_nothing() {
        let sql = render(AttributeHistory::ObservedAt, &HistoryAliases::top_level());
        assert!(sql.contains(r#""qe"."placed" < '2019-01-01"#), "{sql}");
        assert!(!sql.contains(r#""qe"."placed" <="#), "{sql}");
    }

    /// Clause 5.2.21: each axis is placed on its own column, never on the
    /// write clock.
    #[test]
    fn each_axis_places_on_its_own_column() {
        for (axis, column) in [(AttributeHistory::CreatedAt, "created_at"), (AttributeHistory::ModifiedAt, "modified_at")] {
            let snapshot = HistoryBranch::recorded_in(Alias::new("snap_1").into_iden());
            let sql = render_on(&snapshot, axis, &HistoryAliases::top_level());
            assert!(sql.contains(&format!(r#"ORDER BY "qp"."{column}" DESC"#)), "{column}: {sql}");
            assert!(sql.contains(&format!(r#""qd"."{column}" IS NULL"#)), "{column}: {sql}");
            assert!(!sql.contains("recorded_at"), "{column}: {sql}");
        }
    }

    /// A linked-entity hop places its stream under aliases of its own.
    #[test]
    fn a_hop_places_under_its_own_aliases() {
        let sql = render(AttributeHistory::ObservedAt, &HistoryAliases::at_depth(2));
        for alias in [r#""qe_ln2""#, r#""qp_ln2""#, r#""qd_ln2""#] {
            assert!(sql.contains(alias), "{alias} missing: {sql}");
        }
        assert!(!sql.contains(r#""qe""#), "{sql}");
    }
}
