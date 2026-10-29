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
//! A row is placed at `COALESCE(axis, deleted_at)`: the axis instant when it
//! carries one and the deletion instant otherwise, and a row with neither is
//! placed nowhere. Every part of the stream is read in descending placement
//! from the start and stopped after the rows tied with its first, which the
//! stream-led index of the same expression serves with one index entry per
//! chunk. On the live branch the queued part is read the same way beside the
//! recorded one and the two are folded by the same limit: an order or a limit
//! above a union of lateral parts would sort every row of every part.
//!
//! Every row at the latest placement is kept, so a repeated write at one
//! instant offers each of its values, as clause 4.9 asks of a term matched
//! against "any Value of such instances".

use crate::{
    history_source::{HistoryBranch, HistoryOf, HistoryStream, stream_parts::whole_stream_parts},
    q_predicate::history_aliases::HistoryAliases,
};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::row_ties::first_row_with_ties;
use chrono::{DateTime, Utc};
use sea_query::{Alias, Asterisk, Expr as SeaExpr, ExprTrait, Func, Iden, Order, Query, SelectStatement, SimpleExpr, UnionType};
use typed_builder::TypedBuilder;

/// The column holding the instant a row of the in-effect read is placed at.
#[derive(Iden, Clone, Copy)]
#[iden = "placed"]
struct Placed;

/// The instance one stream held when an interval with a start opened.
#[derive(Debug, Clone, TypedBuilder)]
pub(crate) struct InEffectInstance<'a> {
    /// The branch whose history the instance is placed in.
    branch: &'a HistoryBranch,
    /// The Temporal Evolution the stream source of the enclosing probe reads.
    of: &'a HistoryOf,
    /// The `attribute_history` column holding the requested Temporal Property.
    axis: AttributeHistory,
    /// The instant the interval opens at.
    start: DateTime<Utc>,
}

impl InEffectInstance<'_> {
    /// The rows of the stream under the stream alias at its latest placement
    /// at or before the start, deletions included, each with every column of
    /// the attribute source and its placement under `placed`: the `LATERAL`
    /// source the instance is read from.
    pub(crate) fn rows(&self, aliases: &HistoryAliases) -> SelectStatement {
        let stream = HistoryStream::read_for(self.branch, self.of, aliases.stream());
        let (recorded, others) = whole_stream_parts(self.branch, &stream).into_reads();
        let mut others = others.map(|part| self.latest(part, aliases)).peekable();
        let mut joined = self.latest(recorded, aliases);
        if others.peek().is_none() {
            return joined;
        }
        for other in others {
            joined.union(UnionType::All, other);
        }
        Self::tied(joined, SeaExpr::col(Placed), aliases)
    }

    /// The rows of `part` at its latest placement at or before the start,
    /// each with its placement.
    fn latest(&self, part: SelectStatement, aliases: &HistoryAliases) -> SelectStatement {
        let placed = self.placement(&aliases.placed_instance());
        let mut read = Query::select();
        read.column((aliases.placed_instance(), Asterisk))
            .expr_as(placed.clone(), Placed) // clone: the placement is projected, bounded and ordered on
            .from_subquery(part, aliases.placed_instance())
            .and_where(placed.clone().lte(self.start)); // clone: the bound and the order name the same placement
        Self::tied(read, placed, aliases)
    }

    /// The first row of `read` in descending `order` and every row tied with
    /// it, every column projected.
    fn tied(read: SelectStatement, order: SimpleExpr, aliases: &HistoryAliases) -> SelectStatement {
        let mut tied = Query::select();
        tied.column(Asterisk).from(first_row_with_ties(read, order, Order::Desc, aliases.placement()));
        tied
    }

    /// `COALESCE(axis, deleted_at)` of the row under `row`, written as the
    /// placement indexes declare it so the planner matches them to it.
    fn placement(&self, row: &Alias) -> SimpleExpr {
        // clone: each column reference takes the borrowed alias by value.
        Func::coalesce([SeaExpr::col((row.clone(), self.axis)), SeaExpr::col((row.clone(), AttributeHistory::DeletedAt))]).into()
    }

    /// Whether the instance read under the instance alias is placed strictly
    /// before the start.
    ///
    /// An instance placed at the start itself lies inside the interval, which
    /// clause 4.11 opens inclusively, so it is the window's own and nothing
    /// before it is in effect.
    pub(crate) fn opens_before_start(&self, aliases: &HistoryAliases) -> SimpleExpr {
        SeaExpr::col((aliases.instance(), Placed)).lt(self.start)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        history_source::{EvolutionColumns, HistoryBranch, HistoryOf},
        q_predicate::{history_aliases::HistoryAliases, in_effect_instance::InEffectInstance},
    };
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Expr, IntoIden, JoinType, PostgresQueryBuilder, Query, SimpleExpr};

    fn start() -> DateTime<Utc> {
        "2019-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn entity() -> HistoryOf {
        HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id"))))
    }

    fn snapshot() -> HistoryBranch {
        HistoryBranch::recorded_in(Alias::new("snap_1").into_iden())
    }

    /// The in-effect rows joined to a stream and narrowed to those placed
    /// before the start, as the probe arranges them.
    fn render_of(branch: &HistoryBranch, of: &HistoryOf, axis: AttributeHistory, aliases: &HistoryAliases) -> String {
        let instance = InEffectInstance::builder().branch(branch).of(of).axis(axis).start(start()).build();
        Query::select()
            .expr(Expr::val(1))
            .from_as(Alias::new("streams"), aliases.stream())
            .join_lateral(JoinType::Join, instance.rows(aliases), aliases.instance(), SimpleExpr::from(true))
            .and_where(instance.opens_before_start(aliases))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    fn render(branch: &HistoryBranch, axis: AttributeHistory) -> String {
        render_of(branch, &entity(), axis, &HistoryAliases::top_level())
    }

    /// Clause 4.11: the in-effect instance is the latest at or before the
    /// start, read by one descending read of the stream stopped after the
    /// rows tied with its first.
    #[test]
    fn the_stream_is_read_down_from_the_start_and_stopped_after_the_tied_rows() {
        let sql = render(&snapshot(), AttributeHistory::ObservedAt);
        for fragment in [
            r#"JOIN LATERAL (SELECT * FROM ((SELECT "qi".*, COALESCE("qi"."observed_at", "qi"."deleted_at") AS "placed" FROM (SELECT"#,
            r#"FROM "snap_1"."attribute_history" AS "rh" WHERE "rh"."stream_key" = "qs"."stream_key") AS "qi""#,
            r#"WHERE COALESCE("qi"."observed_at", "qi"."deleted_at") <= '2019-01-01"#,
            r#"ORDER BY COALESCE("qi"."observed_at", "qi"."deleted_at") DESC) FETCH FIRST 1 ROW WITH TIES) AS "qe") AS "qh" ON TRUE"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        for absent in ["UNION", "MAX(", "GREATEST(", "temporal_append"] {
            assert!(!sql.contains(absent), "{absent}: {sql}");
        }
    }

    /// Clause 4.5.7: a deletion with no instant on the axis is placed at the
    /// instant it was deleted, so the placement falls back to it.
    #[test]
    fn a_deletion_without_an_axis_instant_is_placed_at_its_deletion() {
        let sql = render(&snapshot(), AttributeHistory::ObservedAt);
        assert!(sql.contains(r#"COALESCE("qi"."observed_at", "qi"."deleted_at")"#), "{sql}");
        assert!(!sql.contains(r#""observed_at" IS NULL"#), "{sql}");
    }

    /// On the live branch the queued entries of the stream's grain are a part
    /// of their own, read down from the start like the recorded part and
    /// folded with it by the same limit, and a recorded row a queued entry
    /// replaces is not read.
    #[test]
    fn the_live_read_folds_the_queued_part_by_the_same_limit() {
        let sql = render(&HistoryBranch::live(), AttributeHistory::ObservedAt);
        for fragment in [
            r#"UNION ALL (SELECT * FROM ((SELECT "qi".*, COALESCE("qi"."observed_at", "qi"."deleted_at") AS "placed" FROM (SELECT"#,
            r#""pe"."entity_id" = "qs"."entity_id""#,
            r#"COALESCE("pe"."dataset_id", '') = COALESCE("qs"."dataset_id", '')"#,
            r#"ORDER BY "placed" DESC) FETCH FIRST 1 ROW WITH TIES) AS "qe") AS "qh" ON TRUE"#,
            r#""rh"."verbatim_instance_id" IS NULL OR (NOT EXISTS(SELECT 1 FROM "temporal_append_entries" AS "pr" WHERE "pr"."entity_id" = "qs"."entity_id""#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        pretty_assertions::assert_eq!(sql.matches("FETCH FIRST 1 ROW WITH TIES").count(), 3, "{sql}");
    }

    /// Clause 4.11 opens the interval inclusively: an instance placed at the
    /// start itself belongs to the window, so the in-effect test is strict.
    #[test]
    fn a_placement_at_the_start_itself_admits_nothing() {
        let sql = render(&snapshot(), AttributeHistory::ObservedAt);
        assert!(sql.contains(r#"WHERE "qh"."placed" < '2019-01-01"#), "{sql}");
        assert!(!sql.contains(r#""qh"."placed" <="#), "{sql}");
    }

    /// Clause 5.2.21: each axis is placed on its own column, never on the
    /// write clock, the stream read whole.
    #[test]
    fn each_axis_places_on_its_own_column() {
        for (axis, column) in [(AttributeHistory::CreatedAt, "created_at"), (AttributeHistory::ModifiedAt, "modified_at")] {
            let sql = render(&snapshot(), axis);
            assert!(
                sql.contains(&format!(r#"ORDER BY COALESCE("qi"."{column}", "qi"."deleted_at") DESC"#)),
                "{column}: {sql}"
            );
            assert!(!sql.contains(r#""rh"."recorded_at" AS"#) && !sql.contains("DISTINCT FROM"), "{column}: {sql}");
        }
    }

    /// Read for an Evolution by key over recorded rows, the stream names its
    /// Evolution by the key, with no identifier.
    #[test]
    fn a_stream_read_by_key_names_its_evolution_by_key() {
        let of = HistoryOf::Evolution(EvolutionColumns {
            id: Expr::col((Alias::new("e"), Alias::new("id"))),
            key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
        });
        let sql = render_of(&HistoryBranch::live_recorded(), &of, AttributeHistory::ObservedAt, &HistoryAliases::top_level());
        assert!(sql.contains(r#""qs"."entity_key" AS "entity_key""#), "{sql}");
        assert!(!sql.contains("entity_id"), "{sql}");
    }

    /// A linked-entity hop places its stream under aliases of its own.
    #[test]
    fn a_hop_places_under_its_own_aliases() {
        let sql = render_of(&HistoryBranch::live(), &entity(), AttributeHistory::ObservedAt, &HistoryAliases::at_depth(2));
        for alias in [r#"AS "qe_ln2""#, r#"AS "qi_ln2""#, r#"AS "qh_ln2""#, r#""qs_ln2"."stream_key""#] {
            assert!(sql.contains(alias), "{alias} missing: {sql}");
        }
        assert!(!sql.contains(r#""qe""#) && !sql.contains(r#""qi""#), "{sql}");
    }
}
