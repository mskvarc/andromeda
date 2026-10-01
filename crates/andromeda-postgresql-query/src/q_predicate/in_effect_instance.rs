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
//! The latest placement is the greater of two descending probes, one per
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

use crate::q_predicate::history_window::HistoryAliases;
use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams};
use andromeda_postgresql_seaquery::branch::qualified;
use chrono::{DateTime, Utc};
use sea_query::{Alias, Condition, DynIden, Expr as SeaExpr, ExprTrait, Func, Iden, Order, Query, QueryStatementBuilder, SelectStatement, SimpleExpr};
use typed_builder::TypedBuilder;

/// The column of the placement source holding the instant the stream's
/// in-effect instance is placed at.
#[derive(Iden, Clone, Copy)]
#[iden = "placed"]
struct Placed;

/// The instance one stream held when an interval with a start opened.
#[derive(Debug, Clone, TypedBuilder)]
pub(crate) struct InEffectInstance<'a> {
    /// The context branch schema the history relation is qualified with, or
    /// `None` for the live branch.
    schema: Option<&'a DynIden>,
    /// The `attribute_history` column holding the requested Temporal Property.
    axis: AttributeHistory,
    /// The instant the interval opens at.
    start: DateTime<Utc>,
}

impl InEffectInstance<'_> {
    /// The `LATERAL` source placing the stream's in-effect instance:
    /// `SELECT GREATEST(<latest instance>, <latest deletion>) AS placed`.
    ///
    /// On the `deletedAt` axis every deletion carries its instant on the axis
    /// itself, so the first probe already finds it and no second one is read.
    pub(crate) fn placement_source(&self, aliases: &HistoryAliases) -> SelectStatement {
        let latest_instance = self.latest_placement(|| aliases.prior(), self.axis, Condition::all(), aliases);
        let placed = if self.axis == AttributeHistory::DeletedAt {
            latest_instance
        } else {
            let without_instant = Condition::all().add(SeaExpr::col((aliases.deletion(), self.axis)).is_null());
            let latest_deletion = self.latest_placement(|| aliases.deletion(), AttributeHistory::DeletedAt, without_instant, aliases);
            Func::greatest([latest_instance, latest_deletion]).into()
        };
        Query::select().expr_as(placed, Placed).to_owned()
    }

    /// The latest instant on `column` at or before the start among the
    /// stream's rows that pass `placed_by`, read under the alias `relation`
    /// names as one descending probe.
    fn latest_placement(&self, relation: impl Fn() -> Alias, column: AttributeHistory, placed_by: Condition, aliases: &HistoryAliases) -> SimpleExpr {
        let latest = Query::select()
            .expr(SeaExpr::col((relation(), column)))
            .from(qualified(self.schema, AttributeHistory::Table).alias(relation()))
            .and_where(SeaExpr::col((relation(), AttributeHistory::StreamKey)).eq(aliases.stream_column(TemporalAttributeStreams::StreamKey)))
            .cond_where(placed_by)
            .and_where(SeaExpr::col((relation(), column)).lte(self.start))
            .order_by((relation(), column), Order::Desc)
            .limit(1)
            .to_owned();
        SimpleExpr::SubQuery(None, Box::new(latest.into_sub_query_statement()))
    }

    /// The instant the in-effect instance is placed at, as the placement
    /// source exposes it.
    fn placed(aliases: &HistoryAliases) -> SimpleExpr {
        SeaExpr::col((aliases.placement(), Placed))
    }

    /// The join of the scanned instance to its stream's placement: the
    /// instances of the stream placed at exactly that instant.
    ///
    /// Every instance sharing the instant is joined, so a repeated write at one
    /// instant offers each of its values, as clause 4.9 asks of a term matched
    /// against "any Value of such instances".
    pub(crate) fn joins_instance(&self, aliases: &HistoryAliases) -> Condition {
        Condition::all()
            .add(SeaExpr::expr(aliases.instance_column(AttributeHistory::StreamKey)).eq(aliases.stream_column(TemporalAttributeStreams::StreamKey)))
            .add(SeaExpr::expr(aliases.instance_column(self.axis)).eq(Self::placed(aliases)))
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
    use crate::q_predicate::{history_window::HistoryAliases, in_effect_instance::InEffectInstance};
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Condition, Expr, JoinType, PostgresQueryBuilder, Query, SimpleExpr};

    fn start() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2019-01-01T00:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
    }

    fn instance(axis: AttributeHistory) -> InEffectInstance<'static> {
        InEffectInstance::builder().schema(None).axis(axis).start(start()).build()
    }

    /// The placement source, the join back to the instance and the strict
    /// start test, rendered as the probe arranges them.
    fn render(axis: AttributeHistory, aliases: &HistoryAliases) -> String {
        let instance = instance(axis);
        Query::select()
            .expr(Expr::val(1))
            .from_as(Alias::new("temporal_attribute_streams"), aliases.stream())
            .join_lateral(JoinType::Join, instance.placement_source(aliases), aliases.placement(), SimpleExpr::from(true))
            .join_as(
                JoinType::Join,
                Alias::new("attribute_history"),
                aliases.instance(),
                instance.joins_instance(aliases),
            )
            .cond_where(Condition::all().add(instance.opens_before_start(aliases)))
            .to_owned()
            .to_string(PostgresQueryBuilder)
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

    /// The placement is the greater of the two probes, exposed as `placed`.
    #[test]
    fn the_placement_is_the_greater_of_the_two() {
        let sql = render(AttributeHistory::ObservedAt, &HistoryAliases::top_level());
        for fragment in [r#"GREATEST((SELECT "qp"."observed_at""#, r#"AS "placed""#, "JOIN LATERAL (SELECT GREATEST("] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        assert!(!sql.contains("MAX(") && !sql.contains("COALESCE("), "{sql}");
    }

    /// On the `deletedAt` axis a deletion carries its instant on the axis, so
    /// the first probe already places it and no second probe is read.
    #[test]
    fn a_deleted_at_window_places_no_separate_deletion_arm() {
        let sql = render(AttributeHistory::DeletedAt, &HistoryAliases::top_level());
        assert!(!sql.contains(r#""qd""#), "{sql}");
        assert!(!sql.contains("GREATEST("), "{sql}");
        assert!(sql.contains(r#"ORDER BY "qp"."deleted_at" DESC LIMIT 1"#), "{sql}");
    }

    /// The instances placed at exactly the placed instant are the ones joined.
    #[test]
    fn the_instance_joins_back_at_the_placed_instant() {
        let sql = render(AttributeHistory::ObservedAt, &HistoryAliases::top_level());
        assert!(
            sql.contains(r#""qh"."stream_key" = "qs"."stream_key" AND "qh"."observed_at" = "qe"."placed""#),
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
            let sql = render(axis, &HistoryAliases::top_level());
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
