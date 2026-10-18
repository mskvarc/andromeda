//! The Temporal Evolutions a temporal read selects, each with the types it is
//! recorded under.
//!
//! Clause 4.5.6 makes the Temporal Evolution the Entity as clause 4.5.1
//! mandates, and the application writes the types of an Entity's latest queued
//! intent stating types over the recorded ones, so on the live branch those
//! types are the Entity's before the drain as after it. An Entity only queued
//! intents stating no type name has no Temporal Evolution yet.
//!
//! The union's recorded branch carries no condition of its own: PostgreSQL
//! flattens a `UNION ALL` into one append relation only when every branch is a
//! bare relation, and only then does an identifier-ordered page read the
//! catalogue's identifier index and stop at the page. The recorded Evolutions
//! a queued intent retypes are therefore left out by a condition over the
//! union, through the flag each branch projects.
//!
//! A selection by type reads the recorded Evolutions from the walk of their
//! type memberships instead ([`type_member_rows`]), which yields one type's
//! Evolutions in identifier order and visits no other; the catalogue's
//! identifier index filtered by type reads every Evolution whose identifier
//! sorts ahead of the type's last. A selection by attribute alone reads them
//! from the walk of the attribute's streams in the same order
//! ([`attribute_holder_rows`]).

use crate::{
    attribute_name_key::AttributeNameKey,
    dictionary::resolved_names::ResolvedNames,
    history_source::{HistoryBranch, HistoryOf, PendingEntries, grain::HistoryGrain, pending},
    type_membership::{Memberships, members_of_any},
    type_predicate::condition as type_condition,
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
use andromeda_postgresql_schema::{
    temporal::{TemporalAttributeStreams, TemporalEntities},
    type_membership::TypeMembershipRelation,
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    cast::typed_null,
    literal::{false_literal, int_literal},
    pg_type::PostgresType,
};
use sea_query::{Condition, Expr, ExprTrait, Iden, IntoIden, Order, Query, SelectStatement, SimpleExpr, TableRef, UnionType};

/// Alias `re`: a recorded Temporal Evolution.
#[derive(Iden, Clone, Copy)]
#[iden = "re"]
struct RecordedEvolution;

/// Alias `rw`: a recorded Temporal Evolution the membership walk yields.
#[derive(Iden, Clone, Copy)]
#[iden = "rw"]
struct WalkedEvolution;

/// Alias `rq`: the latest queued intent of an Entity stating types.
#[derive(Iden, Clone, Copy)]
#[iden = "rq"]
struct QueuedEvolution;

/// Alias `ru`: one row of the union of recorded and queued Evolutions.
#[derive(Iden, Clone, Copy)]
#[iden = "ru"]
struct EvolutionRow;

/// Alias `held`: one stream row of the merged attribute walk.
#[derive(Iden, Clone, Copy)]
#[iden = "held"]
struct HeldStream;

/// Alias `hr`: one recorded stream of an attribute's range.
#[derive(Iden, Clone, Copy)]
#[iden = "hr"]
struct RecordedHolder;

/// Alias `hq`: an Entity a queued grain of the attributes extends.
#[derive(Iden, Clone, Copy)]
#[iden = "hq"]
struct QueuedHolder;

/// Alias `hm`: one holder of the merged attribute walk, read in key order.
#[derive(Iden, Clone, Copy)]
#[iden = "hm"]
struct MergedHolder;

/// Alias `hg`: a grain queued entries extend that no recorded stream holds.
#[derive(Iden, Clone, Copy)]
#[iden = "hg"]
struct QueuedGrain;

/// The flag a union row carries: whether it is a queued intent's.
#[derive(Iden, Clone, Copy)]
#[iden = "queued"]
struct Queued;

/// The Temporal Evolutions of `branch` under `alias`, with the columns `id`
/// and `entity_type_name_keys`, and `temporal_entity_key` where the branch
/// reads no queue.
pub fn entity_rows(branch: &HistoryBranch, alias: impl IntoIden) -> TableRef {
    let schema = branch.schema();
    match branch.pending() {
        PendingEntries::Absent => qualified(schema, TemporalEntities::Table).alias(alias),
        PendingEntries::Unioned => {
            let mut queued = pending::typed_entity_rows();
            queued.expr_as(SimpleExpr::Constant(true.into()), Queued);
            let mut union = Query::select();
            union
                .column((RecordedEvolution, TemporalEntities::Id))
                .column((RecordedEvolution, TemporalEntities::EntityTypeNameKeys))
                .expr_as(SimpleExpr::Constant(false.into()), Queued)
                .from(qualified(schema, TemporalEntities::Table).alias(RecordedEvolution))
                .union(UnionType::All, queued);
            let mut rows = Query::select();
            rows.column((EvolutionRow, TemporalEntities::Id))
                .column((EvolutionRow, TemporalEntities::EntityTypeNameKeys))
                .from_subquery(union.take(), EvolutionRow);
            TableRef::SubQuery(Box::new(unretyped(rows).take()), alias.into_iden())
        }
    }
}

/// The Temporal Evolutions of `branch` any of `exprs` selects, under `alias`,
/// with the column `id`, and `temporal_entity_key` where the branch reads no
/// queue, read by walking their type memberships in identifier order; `None`
/// when an expression has no range to walk.
///
/// The membership relation holds the recorded Evolutions alone. On the live
/// branch the Entities a queued intent states types for join the walk,
/// filtered by the same expressions over the types of their latest such
/// intent, and a recorded Evolution such an intent retypes is left out of the
/// walk, as [`entity_rows`] leaves it out.
#[must_use]
pub fn type_member_rows(branch: &HistoryBranch, alias: impl IntoIden, exprs: &[&TypeExpr], types: &ResolvedNames<EntityTypeName>) -> Option<TableRef> {
    let memberships = Memberships {
        relation: TypeMembershipRelation::Temporal,
        schema: branch.schema(),
    };
    let mut walk = members_of_any(exprs.iter().copied(), types, memberships)?;
    Some(match branch.pending() {
        PendingEntries::Absent => TableRef::SubQuery(Box::new(walk), alias.into_iden()),
        PendingEntries::Unioned => {
            // The walk filters its relation, so it is not flattened into the
            // union's append relation; it reaches the union as a subquery,
            // which keeps its order only when it states one. Ordered, the
            // union merges both branches in identifier order and stops at the
            // page instead of sorting every member first.
            walk.order_by(memberships.relation.id(), Order::Asc);
            let queued_types = Expr::col((QueuedEvolution, TemporalEntities::EntityTypeNameKeys));
            let mut queued = Query::select();
            queued
                .column((QueuedEvolution, TemporalEntities::Id))
                .expr_as(SimpleExpr::Constant(true.into()), Queued)
                .from_subquery(pending::typed_entity_rows(), QueuedEvolution)
                .cond_where(
                    exprs
                        .iter()
                        .fold(Condition::any(), |any, expr| any.add(type_condition(expr, &queued_types, types))),
                );
            let mut union = Query::select();
            union
                .column((WalkedEvolution, TemporalEntities::Id))
                .expr_as(SimpleExpr::Constant(false.into()), Queued)
                .from_subquery(walk, WalkedEvolution)
                .union(UnionType::All, queued);
            let mut rows = Query::select();
            rows.column((EvolutionRow, TemporalEntities::Id)).from_subquery(union.take(), EvolutionRow);
            TableRef::SubQuery(Box::new(unretyped(rows).take()), alias.into_iden())
        }
    })
}

/// The Temporal Evolutions of `branch` holding a stream of any attribute
/// `keys` names, under `alias`, with the columns `id` and
/// `temporal_entity_key` (null for an Evolution only queued), read by walking
/// each attribute's streams in identifier order; no row when `keys` is empty.
///
/// An Evolution holds one stream per `datasetId` of an attribute and may hold
/// several of the attributes, so the ranges merge in identifier order with
/// each Evolution once. On the live branch the Entities whose queued entries
/// extend a grain of one of the attributes that no recorded stream holds
/// join the merge. Every branch reaches the merge as a subquery stating its
/// own order: beside the queued branch, which is no bare relation, a bare
/// range is appended unordered and every holder sorted before the first is
/// kept.
#[must_use]
pub fn attribute_holder_rows(branch: &HistoryBranch, alias: impl IntoIden, keys: &[AttributeNameKey]) -> TableRef {
    let walk = holder_merge(branch, keys).unwrap_or_else(|| no_holder(branch));
    TableRef::SubQuery(Box::new(walk), alias.into_iden())
}

/// The Temporal Evolutions [`attribute_holder_rows`] walks, under `alias`,
/// with the column `id`, yielded in the order of their surrogate key; for a
/// count, which needs no order of its own.
///
/// Each holder's history is probed by the stream and history indexes, whose
/// entries follow the surrogate keys as the Evolutions were minted. Probed in
/// identifier order, which follows no key, a dense attribute's probes read
/// those indexes at random; in key order they read them in sequence. The
/// merge in identifier order stays, so an Evolution holding several streams
/// is still yielded once; a queued holder, with no key yet, comes last.
#[must_use]
pub fn attribute_holder_rows_by_key(branch: &HistoryBranch, alias: impl IntoIden, keys: &[AttributeNameKey]) -> TableRef {
    let Some(merge) = holder_merge(branch, keys) else {
        return TableRef::SubQuery(Box::new(no_holder(branch)), alias.into_iden());
    };
    let mut by_key = Query::select();
    by_key
        .column((MergedHolder, TemporalEntities::Id))
        .from_subquery(merge, MergedHolder)
        .order_by((MergedHolder, TemporalAttributeStreams::TemporalEntityKey), Order::Asc);
    TableRef::SubQuery(Box::new(by_key.take()), alias.into_iden())
}

/// The holders of `keys`' streams on `branch`, each once, in identifier
/// order, with the identifier and the Evolution's surrogate key, null for a
/// holder only queued; `None` when `keys` is empty.
fn holder_merge(branch: &HistoryBranch, keys: &[AttributeNameKey]) -> Option<SelectStatement> {
    let schema = branch.schema();
    let mut ranges = keys.iter().map(|key| {
        let mut holders = Query::select();
        holders
            .column((RecordedHolder, TemporalEntities::Id))
            .column((RecordedHolder, TemporalAttributeStreams::TemporalEntityKey));
        let mut range = Query::select();
        range
            .column(TemporalAttributeStreams::Id)
            .column(TemporalAttributeStreams::TemporalEntityKey)
            .from(qualified(schema, TemporalAttributeStreams::Table))
            .and_where(Expr::col(TemporalAttributeStreams::AttributeNameKey).eq(int_literal(i64::from(key.get()))))
            .order_by(TemporalAttributeStreams::Id, Order::Asc);
        holders.from_subquery(range.take(), RecordedHolder);
        holders
    });
    let mut union = ranges.next()?;
    for range in ranges {
        union.union(UnionType::All, range);
    }
    match branch.pending() {
        PendingEntries::Absent => {}
        PendingEntries::Unioned => {
            let mut queued = Query::select();
            queued
                .expr_as(Expr::col((QueuedGrain, HistoryGrain::EntityId)), TemporalEntities::Id)
                .from_subquery(pending::stream_rows(schema, &HistoryOf::EveryEntity), QueuedGrain)
                .and_where(Expr::col((QueuedGrain, HistoryGrain::AttributeNameKey)).is_in(keys.iter().map(|key| int_literal(i64::from(key.get())))))
                .order_by(TemporalEntities::Id, Order::Asc);
            let mut holders = Query::select();
            holders
                .column((QueuedHolder, TemporalEntities::Id))
                .expr_as(typed_null(PostgresType::Bigint), TemporalAttributeStreams::TemporalEntityKey)
                .from_subquery(queued.take(), QueuedHolder);
            union.union(UnionType::All, holders.take());
        }
    }
    let mut walk = Query::select();
    walk.distinct_on([(HeldStream, TemporalEntities::Id)])
        .column((HeldStream, TemporalEntities::Id))
        .column((HeldStream, TemporalAttributeStreams::TemporalEntityKey))
        .from_subquery(union.take(), HeldStream)
        .order_by((HeldStream, TemporalEntities::Id), Order::Asc);
    Some(walk.take())
}

/// A walk of no attribute key: an attribute the dictionary never keyed was
/// never recorded, so no Evolution holds it.
fn no_holder(branch: &HistoryBranch) -> SelectStatement {
    let mut none = Query::select();
    none.column(TemporalAttributeStreams::Id)
        .column(TemporalAttributeStreams::TemporalEntityKey)
        .from(qualified(branch.schema(), TemporalAttributeStreams::Table))
        .and_where(false_literal());
    none.take()
}

/// `rows` over the union of recorded and queued Evolutions, keeping a
/// recorded one only while no queued intent states types for it: the
/// application writes those types over the recorded ones.
fn unretyped(mut rows: SelectStatement) -> SelectStatement {
    rows.cond_where(
        Condition::any()
            .add(Expr::col((EvolutionRow, Queued)))
            .add(pending::types_queued_for(Expr::col((EvolutionRow, TemporalEntities::Id))).not()),
    );
    rows
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_type_name_key::EntityTypeNameKey,
        history_source::{
            HistoryBranch,
            entity_rows::{attribute_holder_rows, attribute_holder_rows_by_key, entity_rows, type_member_rows},
        },
    };
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use iri_rs::IriBuf;
    use sea_query::{Alias, Asterisk, IntoIden, PostgresQueryBuilder, Query};

    fn rendered(source: sea_query::TableRef) -> String {
        Query::select().column(Asterisk).from(source).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn pending_entities_take_the_types_of_their_latest_typed_intent() {
        let sql = rendered(entity_rows(&HistoryBranch::live(), Alias::new("e")));
        assert!(
            sql.contains(
                r#"SELECT DISTINCT ON ("pt"."entity_id") "pt"."entity_id" AS "id", "pt"."entity_type_name_keys" FROM "temporal_append_intents" AS "pt" WHERE cardinality("pt"."entity_type_name_keys") > 0 ORDER BY "pt"."entity_id" ASC, "pt"."intent_key" DESC"#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(r#"WHERE "ru"."queued" OR (NOT EXISTS(SELECT 1 FROM "temporal_append_intents" AS "pt" WHERE "pt"."entity_id" = "ru"."id""#),
            "{sql}"
        );
    }

    /// The recorded branch is the bare catalogue, so the planner flattens the
    /// union and an identifier-ordered page reads the identifier index.
    #[test]
    fn the_recorded_branch_carries_no_condition_of_its_own() {
        let sql = rendered(entity_rows(&HistoryBranch::live(), Alias::new("e")));
        assert!(
            sql.contains(r#"(SELECT "re"."id", "re"."entity_type_name_keys", FALSE AS "queued" FROM "temporal_entities" AS "re" UNION ALL"#),
            "{sql}"
        );
    }

    #[test]
    fn a_snapshot_reads_its_catalogue_alone() {
        let sql = rendered(entity_rows(&HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()), Alias::new("e")));
        pretty_assertions::assert_eq!(sql, r#"SELECT * FROM "snap_1"."temporal_entities" AS "e""#);
    }

    fn vehicle() -> TypeExpr {
        TypeExpr::Type(IriBuf::new("https://example.com/Vehicle".to_owned()).expect("valid IRI"))
    }

    fn vehicle_is_known() -> ResolvedNames<EntityTypeName> {
        ResolvedNames::from_pairs([(
            "https://example.com/Vehicle".parse::<EntityTypeName>().expect("valid type name"),
            EntityTypeNameKey::from(3),
        )])
    }

    fn walked(branch: &HistoryBranch, expr: &TypeExpr) -> String {
        rendered(type_member_rows(branch, Alias::new("e"), &[expr], &vehicle_is_known()).expect("the type has a range"))
    }

    /// Clause 4.17 on the live branch: the recorded Evolutions of the type come
    /// from its membership range, the queued ones from their latest typed
    /// intent filtered by the same type, and a recorded one a queued intent
    /// retypes is left to that intent.
    #[test]
    fn a_live_type_walk_reads_the_membership_range_beside_the_queued_typed_intents() {
        let sql = walked(&HistoryBranch::live(), &vehicle());
        assert!(
            sql.contains(r#"(SELECT "rw"."id", FALSE AS "queued" FROM (SELECT "temporal_entity_key", "id" FROM "temporal_entity_type_members" WHERE "entity_type_name_key" = (3) ORDER BY "id" ASC) AS "rw" UNION ALL"#),
            "{sql}"
        );
        assert!(sql.contains(r#"AS "rq" WHERE "rq"."entity_type_name_keys" @> (ARRAY[3])"#), "{sql}");
        assert!(
            sql.contains(r#"WHERE "ru"."queued" OR (NOT EXISTS(SELECT 1 FROM "temporal_append_intents" AS "pt" WHERE "pt"."entity_id" = "ru"."id""#),
            "{sql}"
        );
        assert!(!sql.contains(r#"FROM "temporal_entities""#), "{sql}");
    }

    #[test]
    fn a_snapshot_type_walk_reads_its_membership_range_alone() {
        let sql = walked(&HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()), &vehicle());
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT * FROM (SELECT "temporal_entity_key", "id" FROM "snap_1"."temporal_entity_type_members" WHERE "entity_type_name_key" = (3)) AS "e""#
        );
    }

    fn holders(branch: &HistoryBranch, keys: &[i32]) -> String {
        let keys: Vec<AttributeNameKey> = keys.iter().copied().map(AttributeNameKey::from).collect();
        rendered(attribute_holder_rows(branch, Alias::new("e"), &keys))
    }

    /// Each attribute's streams are one range of the attribute key in
    /// identifier order, ordered on their own, and the ranges merge with each
    /// Evolution once, however many streams it holds, carrying its surrogate
    /// key for a source compared by key.
    #[test]
    fn an_attribute_walk_merges_the_ordered_stream_ranges_of_each_attribute() {
        let sql = holders(&HistoryBranch::live_recorded(), &[7, 9]);
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"SELECT * FROM (SELECT DISTINCT ON ("held"."id") "held"."id", "held"."temporal_entity_key" FROM ("#,
                r#"SELECT "hr"."id", "hr"."temporal_entity_key" FROM (SELECT "id", "temporal_entity_key" FROM "temporal_attribute_streams" WHERE "attribute_name_key" = (7) ORDER BY "id" ASC) AS "hr" UNION ALL "#,
                r#"(SELECT "hr"."id", "hr"."temporal_entity_key" FROM (SELECT "id", "temporal_entity_key" FROM "temporal_attribute_streams" WHERE "attribute_name_key" = (9) ORDER BY "id" ASC) AS "hr")"#,
                r#") AS "held" ORDER BY "held"."id" ASC) AS "e""#
            )
        );
    }

    /// On the live branch an Entity whose queued entries extend a grain of the
    /// attribute that no recorded stream holds joins the merge, ordered on its
    /// own.
    #[test]
    fn a_live_attribute_walk_merges_the_grains_the_queued_entries_extend() {
        let sql = holders(&HistoryBranch::live(), &[7]);
        assert!(
            sql.contains(
                r#"UNION ALL (SELECT "hq"."id", NULL::bigint AS "temporal_entity_key" FROM (SELECT "hg"."entity_id" AS "id" FROM (SELECT DISTINCT "pe"."entity_id""#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#"FROM "temporal_append_entries" AS "pe""#), "{sql}");
        assert!(
            sql.contains(r#") AS "hg" WHERE "hg"."attribute_name_key" IN (7) ORDER BY "id" ASC) AS "hq")"#),
            "{sql}"
        );
    }

    #[test]
    fn a_snapshot_attribute_walk_reads_its_own_streams() {
        let sql = holders(&HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()), &[7]);
        assert!(
            sql.contains(r#"FROM "snap_1"."temporal_attribute_streams" WHERE "attribute_name_key" = (7)"#),
            "{sql}"
        );
        assert!(!sql.contains("temporal_append_entries"), "{sql}");
    }

    /// An attribute the dictionary never keyed was never recorded, so a walk
    /// of no key yields no Evolution.
    #[test]
    fn an_attribute_walk_of_no_key_yields_no_row() {
        let none = r#"SELECT * FROM (SELECT "id", "temporal_entity_key" FROM "temporal_attribute_streams" WHERE FALSE) AS "e""#;
        pretty_assertions::assert_eq!(holders(&HistoryBranch::live(), &[]), none);
        pretty_assertions::assert_eq!(rendered(attribute_holder_rows_by_key(&HistoryBranch::live(), Alias::new("e"), &[])), none);
    }

    /// The same merge, each holder once, read back in the order of its
    /// surrogate key; a holder only queued has none and carries a null one.
    #[test]
    fn a_holder_walk_by_key_merges_in_identifier_order_and_yields_in_key_order() {
        let keys = [AttributeNameKey::from(7), AttributeNameKey::from(9)];
        let sql = rendered(attribute_holder_rows_by_key(&HistoryBranch::live(), Alias::new("e"), &keys));
        assert!(
            sql.starts_with(r#"SELECT * FROM (SELECT "hm"."id" FROM (SELECT DISTINCT ON ("held"."id") "held"."id", "held"."temporal_entity_key" FROM ("#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"(SELECT "hr"."id", "hr"."temporal_entity_key" FROM (SELECT "id", "temporal_entity_key" FROM "temporal_attribute_streams" WHERE "attribute_name_key" = (9) ORDER BY "id" ASC) AS "hr")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"(SELECT "hq"."id", NULL::bigint AS "temporal_entity_key" FROM (SELECT "hg"."entity_id" AS "id""#),
            "{sql}"
        );
        assert!(
            sql.ends_with(r#"ORDER BY "held"."id" ASC) AS "hm" ORDER BY "hm"."temporal_entity_key" ASC) AS "e""#),
            "{sql}"
        );
    }

    /// The wildcard selects every Evolution, so it has no range to walk.
    #[test]
    fn a_selection_without_a_range_has_no_member_rows() {
        assert!(type_member_rows(&HistoryBranch::live(), Alias::new("e"), &[&TypeExpr::Wildcard], &vehicle_is_known()).is_none());
    }
}
