//! The Temporal Evolutions a temporal read selects, each with the types it is
//! recorded under.
//!
//! Clause 4.5.6 makes the Temporal Evolution the Entity as clause 4.5.1
//! mandates, and the application adds every type an Entity's queued intents
//! state to the recorded ones, so on the live branch an Entity with such an
//! intent is read under that union, its types before the drain as after it.
//! An Entity only queued intents stating no type name has no Temporal
//! Evolution yet.
//!
//! The union's recorded branch carries no condition of its own: PostgreSQL
//! flattens a `UNION ALL` into one append relation only when every branch is a
//! bare relation, and only then does an identifier-ordered page read the
//! catalogue's identifier index and stop at the page. The recorded Evolutions
//! a queued intent adds types to are therefore left out by a condition over
//! the union, through the flag each branch projects, and read from the queued
//! branch with their recorded types.
//!
//! A selection by type reads the recorded Evolutions from the walk of their
//! type memberships instead ([`type_member_rows`]), which yields one type's
//! Evolutions in identifier order and visits no other; the catalogue's
//! identifier index filtered by type reads every Evolution whose identifier
//! sorts ahead of the type's last. A selection by attribute alone reads them
//! from the walk of the attribute's holders in the same order
//! ([`attribute_holder_rows`](crate::history_source::holder_rows::attribute_holder_rows)).
//!
//! Either walk can be narrowed to the recorded Evolutions a statement names by
//! surrogate key ([`WalkedKeys`]), for a page cut from candidates an index
//! yielded rather than from every member.

use crate::{
    dictionary::resolved_names::ResolvedNames,
    entity_sides::holds_evolution,
    history_source::{HistoryBranch, PendingEntries, pending},
    type_list_union::TypeListColumn,
    type_membership::{Memberships, members_of_any},
    type_predicate::condition as type_condition,
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
use andromeda_postgresql_schema::{
    current::{Entities, EntityTypeMembers},
    type_membership::MembershipSide,
};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Asterisk, Condition, Expr, ExprTrait, Iden, IntoIden, Order, Query, SelectStatement, SimpleExpr, TableRef, UnionType};

/// Alias `re`: a recorded Temporal Evolution.
#[derive(Iden, Clone, Copy)]
#[iden = "re"]
struct RecordedEvolution;

/// Alias `rw`: a recorded Temporal Evolution the membership walk yields.
#[derive(Iden, Clone, Copy)]
#[iden = "rw"]
struct WalkedEvolution;

/// Alias `rq`: an Entity a queued intent states types for, under the types
/// the drain leaves it.
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

/// The recorded Evolutions a membership or holder walk visits.
#[derive(Clone, Debug)]
pub enum WalkedKeys {
    /// Every Evolution of the walk's ranges.
    Every,
    /// Those whose surrogate key the statement yields; an Evolution only
    /// queued, which has no key, still joins the walk.
    Among(Box<SelectStatement>),
}

impl WalkedKeys {
    /// Narrow `range`, which carries the surrogate key in `key`, to the
    /// Evolutions `self` names.
    pub(crate) fn narrow(&self, range: &mut SelectStatement, key: SimpleExpr) {
        match self {
            WalkedKeys::Every => {}
            WalkedKeys::Among(keys) => {
                // clone: every range of the walk reads the same keys
                range.and_where(key.in_subquery(SelectStatement::clone(keys)));
            }
        }
    }
}

/// The Temporal Evolutions of `branch` under `alias`, with the columns `id`
/// and `evolution_type_name_keys`, and every column of `entities` where the
/// branch reads no queue.
///
/// An identity row whose Entity has no Temporal Evolution (a current state
/// alone, with no history recorded yet) is no Evolution; the condition is a
/// plain filter of a simple subquery, which PostgreSQL pulls up into the
/// statement reading it.
pub fn entity_rows(branch: &HistoryBranch, alias: impl IntoIden) -> TableRef {
    let schema = branch.schema();
    match branch.pending() {
        PendingEntries::Absent => TableRef::SubQuery(
            Box::new(
                Query::select()
                    .column(Asterisk)
                    .from(qualified(schema, Entities::Table))
                    .and_where(holds_evolution(Entities::Table))
                    .take(),
            ),
            alias.into_iden(),
        ),
        PendingEntries::Unioned => {
            let mut queued = pending::typed_entity_rows(schema);
            queued.expr_as(SimpleExpr::Constant(true.into()), Queued);
            let mut union = Query::select();
            union
                .column((RecordedEvolution, Entities::Id))
                .column((RecordedEvolution, Entities::EvolutionTypeNameKeys))
                .expr_as(SimpleExpr::Constant(false.into()), Queued)
                .from(qualified(schema, Entities::Table).alias(RecordedEvolution))
                .and_where(holds_evolution(RecordedEvolution))
                .union(UnionType::All, queued);
            let mut rows = Query::select();
            rows.column((EvolutionRow, Entities::Id))
                .column((EvolutionRow, Entities::EvolutionTypeNameKeys))
                .from_subquery(union.take(), EvolutionRow);
            TableRef::SubQuery(Box::new(unretyped(rows).take()), alias.into_iden())
        }
    }
}

/// The Temporal Evolutions of `branch` any of `exprs` selects, under `alias`,
/// with the column `id`, and `entity_key` where the branch reads no
/// queue, read by walking their type memberships in identifier order; `None`
/// when an expression has no range to walk.
///
/// The membership relation holds the recorded Evolutions alone. On the live
/// branch the Entities a queued intent states types for join the walk,
/// filtered by the same expressions over their recorded types and every type
/// their queued intents state, and the recorded Evolution of such an Entity
/// is left out of the walk, as [`entity_rows`] leaves it out.
///
/// Narrowed by `walked`, the recorded walk reads the named Evolutions in
/// identifier order behind `OFFSET 0`, so PostgreSQL neither merges it into
/// the statement reading it nor probes those Evolutions before it orders them.
#[must_use]
pub fn type_member_rows(
    branch: &HistoryBranch,
    alias: impl IntoIden,
    exprs: &[&TypeExpr],
    types: &ResolvedNames<EntityTypeName>,
    walked: &WalkedKeys,
) -> Option<TableRef> {
    let memberships = Memberships {
        side: MembershipSide::Evolution,
        schema: branch.schema(),
    };
    let mut walk = members_of_any(exprs.iter().copied(), types, memberships)?;
    walked.narrow(&mut walk, Expr::col(EntityTypeMembers::EntityKey));
    Some(match branch.pending() {
        PendingEntries::Absent => {
            match walked {
                WalkedKeys::Every => {}
                WalkedKeys::Among(_) => {
                    walk.order_by(EntityTypeMembers::Id, Order::Asc).offset(0);
                }
            }
            TableRef::SubQuery(Box::new(walk), alias.into_iden())
        }
        PendingEntries::Unioned => {
            // The walk filters its relation, so it is not flattened into the
            // union's append relation; it reaches the union as a subquery,
            // which keeps its order only when it states one. Ordered, the
            // union merges both branches in identifier order and stops at the
            // page instead of sorting every member first.
            walk.order_by(EntityTypeMembers::Id, Order::Asc);
            let queued_types = Expr::col((QueuedEvolution, TypeListColumn::EntityTypeNameKeys));
            let mut queued = Query::select();
            queued
                .column((QueuedEvolution, Entities::Id))
                .expr_as(SimpleExpr::Constant(true.into()), Queued)
                .from_subquery(pending::typed_entity_rows(memberships.schema), QueuedEvolution)
                .cond_where(
                    exprs
                        .iter()
                        .fold(Condition::any(), |any, expr| any.add(type_condition(expr, &queued_types, types))),
                );
            let mut union = Query::select();
            union
                .column((WalkedEvolution, Entities::Id))
                .expr_as(SimpleExpr::Constant(false.into()), Queued)
                .from_subquery(walk, WalkedEvolution)
                .union(UnionType::All, queued);
            let mut rows = Query::select();
            rows.column((EvolutionRow, Entities::Id)).from_subquery(union.take(), EvolutionRow);
            TableRef::SubQuery(Box::new(unretyped(rows).take()), alias.into_iden())
        }
    })
}

/// `rows` over the union of recorded and queued Evolutions, keeping a
/// recorded one only while no queued intent states types for it: the queued
/// branch reads it then, under the types the drain leaves it.
fn unretyped(mut rows: SelectStatement) -> SelectStatement {
    rows.cond_where(
        Condition::any()
            .add(Expr::col((EvolutionRow, Queued)))
            .add(pending::types_queued_for(Expr::col((EvolutionRow, Entities::Id))).not()),
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
            entity_rows::{WalkedKeys, entity_rows, type_member_rows},
        },
    };
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_query::type_selection::grammar::TypeExpr;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Asterisk, IntoIden, PostgresQueryBuilder, Query};

    fn rendered(source: sea_query::TableRef) -> String {
        Query::select().column(Asterisk).from(source).to_owned().to_string(PostgresQueryBuilder)
    }

    /// Clauses 5.6.11.4 and 5.6.12.4 only add types: an Entity a queued intent
    /// states types for is read under its recorded types, then every type
    /// its queued intents state, and its recorded row is left to that read.
    #[test]
    fn pending_entities_take_their_recorded_types_and_every_queued_type() {
        let sql = rendered(entity_rows(&HistoryBranch::live(), Alias::new("e")));
        assert!(
            sql.contains(concat!(
                r#"(SELECT "pv"."id" AS "id", "pv"."evolution_type_name_keys" AS "entity_type_name_keys", 0 AS "list_rank" FROM "entities" AS "pv" "#,
                r#"WHERE "pv"."evolution_created_at" IS NOT NULL AND EXISTS(SELECT 1 FROM "temporal_append_intents" AS "pt" WHERE "pt"."entity_id" = "pv"."id""#
            )),
            "{sql}"
        );
        assert!(
            sql.contains(concat!(
                r#"UNION ALL (SELECT "pt"."entity_id" AS "id", "pt"."entity_type_name_keys" AS "entity_type_name_keys", "pt"."intent_key" AS "list_rank" "#,
                r#"FROM "temporal_append_intents" AS "pt" WHERE cardinality("pt"."entity_type_name_keys") > 0)"#
            )),
            "{sql}"
        );
        assert!(sql.contains(r#"GROUP BY "first_stated"."id""#), "{sql}");
        assert!(
            sql.contains(r#"WHERE "ru"."queued" OR (NOT EXISTS(SELECT 1 FROM "temporal_append_intents" AS "pt" WHERE "pt"."entity_id" = "ru"."id""#),
            "{sql}"
        );
    }

    /// The recorded types of an Entity a queued intent states types for are
    /// read in the schema the live branch's relations are named in.
    #[test]
    fn the_recorded_types_of_a_queued_entity_are_read_in_the_branch_schema() {
        let schema = Alias::new("live_x").into_iden();
        let sql = rendered(entity_rows(&HistoryBranch::live_in(schema), Alias::new("e")));
        assert!(sql.contains(r#"FROM "live_x"."entities" AS "pv""#), "{sql}");
    }

    /// The recorded branch is the identity relation filtered by a plain
    /// condition on its own columns, so the planner still flattens the union
    /// and an identifier-ordered page reads the identifier index.
    #[test]
    fn the_recorded_branch_carries_only_the_evolution_condition() {
        let sql = rendered(entity_rows(&HistoryBranch::live(), Alias::new("e")));
        assert!(
            sql.contains(concat!(
                r#"(SELECT "re"."id", "re"."evolution_type_name_keys", FALSE AS "queued" FROM "entities" AS "re" "#,
                r#"WHERE "re"."evolution_created_at" IS NOT NULL UNION ALL"#
            )),
            "{sql}"
        );
    }

    #[test]
    fn a_snapshot_reads_its_catalogue_alone() {
        let sql = rendered(entity_rows(&HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()), Alias::new("e")));
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT * FROM (SELECT * FROM "snap_1"."entities" WHERE "entities"."evolution_created_at" IS NOT NULL) AS "e""#
        );
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
        rendered(type_member_rows(branch, Alias::new("e"), &[expr], &vehicle_is_known(), &WalkedKeys::Every).expect("the type has a range"))
    }

    /// Clause 4.17 on the live branch: the recorded Evolutions of the type come
    /// from its membership range, the queued ones from the types the drain
    /// leaves them filtered by the same type, and a recorded one a queued
    /// intent states types for is left to the queued branch. The catalogue is
    /// read for those Entities alone.
    #[test]
    fn a_live_type_walk_reads_the_membership_range_beside_the_queued_typed_intents() {
        let sql = walked(&HistoryBranch::live(), &vehicle());
        assert!(
            sql.contains(r#"(SELECT "rw"."id", FALSE AS "queued" FROM (SELECT "entity_key", "id" FROM "entity_type_members" WHERE "entity_type_name_key" = (3) AND "is_evolution_member" ORDER BY "id" ASC) AS "rw" UNION ALL"#),
            "{sql}"
        );
        assert!(sql.contains(r#"AS "rq" WHERE "rq"."entity_type_name_keys" @> (ARRAY[3])"#), "{sql}");
        assert!(
            sql.contains(r#"WHERE "ru"."queued" OR (NOT EXISTS(SELECT 1 FROM "temporal_append_intents" AS "pt" WHERE "pt"."entity_id" = "ru"."id""#),
            "{sql}"
        );
        assert_eq!(sql.matches(r#"FROM "entities""#).count(), 1, "{sql}");
        assert!(
            sql.contains(r#"FROM "entities" AS "pv" WHERE "pv"."evolution_created_at" IS NOT NULL AND EXISTS(SELECT 1 FROM "temporal_append_intents" AS "pt" WHERE "pt"."entity_id" = "pv"."id""#),
            "{sql}"
        );
    }

    #[test]
    fn a_snapshot_type_walk_reads_its_membership_range_alone() {
        let sql = walked(&HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()), &vehicle());
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT * FROM (SELECT "entity_key", "id" FROM "snap_1"."entity_type_members" WHERE "entity_type_name_key" = (3) AND "is_evolution_member") AS "e""#
        );
    }

    fn among_candidates() -> WalkedKeys {
        WalkedKeys::Among(Box::new(Query::select().column(Alias::new("entity_key")).from(Alias::new("candidates")).take()))
    }

    /// Narrowed, a type walk reads the named members of its range and
    /// yields them in identifier order behind a fence.
    #[test]
    fn a_narrowed_type_walk_reads_the_named_members_in_identifier_order() {
        let walk = type_member_rows(
            &HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()),
            Alias::new("e"),
            &[&vehicle()],
            &vehicle_is_known(),
            &among_candidates(),
        )
        .expect("the type has a range");
        pretty_assertions::assert_eq!(
            rendered(walk),
            concat!(
                r#"SELECT * FROM (SELECT "entity_key", "id" FROM "snap_1"."entity_type_members" WHERE "entity_type_name_key" = (3) AND "is_evolution_member""#,
                r#" AND "entity_key" IN (SELECT "entity_key" FROM "candidates") ORDER BY "id" ASC OFFSET 0) AS "e""#
            )
        );
    }

    /// The wildcard selects every Evolution, so it has no range to walk.
    #[test]
    fn a_selection_without_a_range_has_no_member_rows() {
        assert!(
            type_member_rows(
                &HistoryBranch::live(),
                Alias::new("e"),
                &[&TypeExpr::Wildcard],
                &vehicle_is_known(),
                &WalkedKeys::Every
            )
            .is_none()
        );
    }
}
