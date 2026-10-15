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
//! sorts ahead of the type's last.

use crate::{
    dictionary::resolved_names::ResolvedNames,
    history_source::{HistoryBranch, PendingEntries, pending},
    type_membership::{Memberships, members_of_any},
    type_predicate::condition as type_condition,
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
use andromeda_postgresql_schema::{temporal::TemporalEntities, type_membership::TypeMembershipRelation};
use andromeda_postgresql_seaquery::branch::qualified;
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

/// The flag a union row carries: whether it is a queued intent's.
#[derive(Iden, Clone, Copy)]
#[iden = "queued"]
struct Queued;

/// The Temporal Evolutions of `branch` under `alias`, with the columns `id`
/// and `entity_type_name_keys`.
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
/// with the column `id`, read by walking their type memberships in identifier
/// order; `None` when an expression has no range to walk.
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
        dictionary::resolved_names::ResolvedNames,
        entity_type_name_key::EntityTypeNameKey,
        history_source::{
            HistoryBranch,
            entity_rows::{entity_rows, type_member_rows},
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

    /// The wildcard selects every Evolution, so it has no range to walk.
    #[test]
    fn a_selection_without_a_range_has_no_member_rows() {
        assert!(type_member_rows(&HistoryBranch::live(), Alias::new("e"), &[&TypeExpr::Wildcard], &vehicle_is_known()).is_none());
    }
}
