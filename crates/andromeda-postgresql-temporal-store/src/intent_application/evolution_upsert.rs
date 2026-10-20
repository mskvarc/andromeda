//! The Temporal Evolutions an application writes, and the type memberships
//! their types gain.
//!
//! Clause 4.5.6 makes the Temporal Evolution "the Entity [...] as mandated by
//! clause 4.5.1", and clauses 5.6.11.4 and 5.6.12.4 add the Entity Type names a
//! write states that it does not hold yet to its list. No operation removes
//! one, so the list only grows, and the membership relation a temporal type
//! selection walks only gains rows.

use crate::intent_application::intent_source::{I, IntentSource, intent, source_intents, source_intents_as};
use andromeda_postgresql_query::{
    type_list_union::{TypeListColumn, type_list_union},
    type_membership::Memberships,
    type_membership_moves::{Member, carried_among, membership_insert},
};
use andromeda_postgresql_schema::{queue::TemporalAppendIntents, temporal::TemporalEntities, type_membership::TypeMembershipRelation};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    conflict_image::Excluded,
    pg_array::{all, array_cat, array_of_subquery, array_position, has_elements},
    returning_image::{New, Old},
    unnest::unnest_of,
};
use sea_query::{
    CommonTableExpression,
    DynIden,
    Expr,
    ExprTrait,
    Func,
    Iden,
    InsertStatement,
    IntoIden,
    OnConflict,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    WindowStatement,
    WithClause,
    error::Error as SeaQueryError,
};

/// The Temporal Evolutions the statement wrote, `evolved`.
#[derive(Iden, Clone, Copy)]
#[iden = "evolved"]
pub(crate) struct Evolved;

/// Whether the statement inserted the evolution rather than updating one,
/// `created`, a column of `evolved` and `grains`.
#[derive(Iden, Clone, Copy)]
#[iden = "created"]
pub(crate) struct Created;

/// The types the evolution is recorded under after the statement,
/// `new_types`, a column of `evolved`.
#[derive(Iden, Clone, Copy)]
#[iden = "new_types"]
struct NewTypes;

/// The types the evolution was recorded under before the statement, null for
/// one it created, `old_types`, a column of `evolved`.
#[derive(Iden, Clone, Copy)]
#[iden = "old_types"]
struct OldTypes;

/// The insert of the memberships of the types an evolution gained,
/// `added_members`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "added_members"]
struct AddedMembers;

/// An intent of the source read for the types it states, `si`.
#[derive(Iden, Clone, Copy)]
#[iden = "si"]
struct StatingIntent;

/// The types the intents of an Entity state, gathered into one list,
/// `stated_types`.
#[derive(Iden, Clone, Copy)]
#[iden = "stated_types"]
struct StatedTypes;

/// One type key of the row an evolution upsert proposed, `added`, both the
/// unnested relation and its one column.
#[derive(Iden, Clone, Copy)]
#[iden = "added"]
struct Added;

/// Add to `with` the evolutions of the Entities the intents of `source` state
/// types for, as `evolved` ([`evolutions_statement`]), and the membership rows
/// of the types they gained, as `added_members` ([`gained_memberships`]).
///
/// # Errors
///
/// Returns [`SeaQueryError`] if a projection ever stops matching its column
/// list, which the fixed shapes here make unreachable.
pub(crate) fn with_evolutions(with: &mut WithClause, source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<(), SeaQueryError> {
    with.cte(
        CommonTableExpression::new()
            .table_name(Evolved)
            .columns([
                TemporalEntities::TemporalEntityKey.into_iden(),
                TemporalEntities::Id.into_iden(),
                Created.into_iden(),
                NewTypes.into_iden(),
                OldTypes.into_iden(),
            ])
            .query(evolutions_statement(source, schema)?)
            .to_owned(),
    )
    .cte(
        CommonTableExpression::new()
            .table_name(AddedMembers)
            .query(gained_memberships(schema)?)
            .to_owned(),
    );
    Ok(())
}

/// The Temporal Evolution of every Entity an intent states types for, created
/// when it is missing and given every type those intents state that it lacks,
/// returning its key, its identifier, whether the insert created it and the
/// types it had and has.
///
/// Clause 4.5.6 makes the Temporal Evolution "the Entity [...] as mandated by
/// clause 4.5.1", so an intent stating types brings it into being; one stating
/// none (every deletion among them) only extends one that exists. Clauses
/// 5.6.11.4 and 5.6.12.4 add the Entity Type names a write states that the
/// Temporal Evolution does not hold yet to its list, and no operation removes
/// one, so the list only grows. The merge reads the stored list in the conflict
/// action, which sees the row as the last writer to commit left it, so a type a
/// concurrent writer added is kept whichever of them commits first.
///
/// A source can hold several intents of one Entity, and a conflict action may
/// change a row once per statement, so one row per Entity is inserted: every
/// type its intents state ([`stated_types`]), its first intent's instant as
/// `created_at` and its latest's as `modified_at`, which is what applying them
/// one after the other leaves. `created_at` is never moved and `modified_at`
/// never moves back ([`latest_modification`], clause 4.8).
fn evolutions_statement(source: &IntentSource<'_>, schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
    let mut select = Query::select();
    select
        .distinct_on([(I, TemporalAppendIntents::EntityId)])
        .expr(intent(TemporalAppendIntents::EntityId))
        .expr_window(
            Func::min(intent(TemporalAppendIntents::RecordedAt)),
            WindowStatement::partition_by((I, TemporalAppendIntents::EntityId)),
        )
        .expr(intent(TemporalAppendIntents::RecordedAt))
        .column((StatedTypes, TypeListColumn::EntityTypeNameKeys));
    source_intents(&mut select, source);
    select
        .from_subquery(stated_types(source), StatedTypes)
        .and_where(Expr::col((StatedTypes, TypeListColumn::Id)).equals((I, TemporalAppendIntents::EntityId)))
        .and_where(has_elements(intent(TemporalAppendIntents::EntityTypeNameKeys)))
        .order_by((I, TemporalAppendIntents::EntityId), Order::Asc)
        .order_by((I, TemporalAppendIntents::IntentKey), Order::Desc);
    let mut statement = Query::insert();
    statement
        .into_table(qualified(schema, TemporalEntities::Table))
        .columns([
            TemporalEntities::Id,
            TemporalEntities::CreatedAt,
            TemporalEntities::ModifiedAt,
            TemporalEntities::EntityTypeNameKeys,
        ])
        .select_from(select)?
        .on_conflict(
            OnConflict::column(TemporalEntities::Id)
                .values([
                    (TemporalEntities::ModifiedAt, latest_modification()),
                    (TemporalEntities::EntityTypeNameKeys, merged_types()),
                ])
                .to_owned(),
        )
        .returning(Query::returning().exprs([
            Expr::col((New, TemporalEntities::TemporalEntityKey)),
            Expr::col((New, TemporalEntities::Id)),
            Expr::col((Old, TemporalEntities::TemporalEntityKey)).is_null(),
            Expr::col((New, TemporalEntities::EntityTypeNameKeys)),
            Expr::col((Old, TemporalEntities::EntityTypeNameKeys)),
        ]));
    Ok(statement)
}

/// Every type the intents of `source` state, one list per Entity, under the
/// columns of [`type_list_union`]: each type once, in the order of the first
/// intent stating it.
fn stated_types(source: &IntentSource<'_>) -> SelectStatement {
    let mut lists = Query::select();
    lists
        .expr_as(Expr::col((StatingIntent, TemporalAppendIntents::EntityId)), TypeListColumn::Id)
        .expr_as(
            Expr::col((StatingIntent, TemporalAppendIntents::EntityTypeNameKeys)),
            TypeListColumn::EntityTypeNameKeys,
        )
        .expr_as(Expr::col((StatingIntent, TemporalAppendIntents::IntentKey)), TypeListColumn::ListRank);
    source_intents_as(&mut lists, source, StatingIntent);
    type_list_union(lists)
}

/// The later of the stored evolution's `modified_at` and the proposed row's.
///
/// Clause 4.8 makes `modifiedAt` the instant the Entity was last modified.
/// Writers whose history is not applied in entity order (a Temporal API write
/// whose fold stores no entity row) can commit after a later-stamped one, and
/// that later instant stays the last modification.
fn latest_modification() -> SimpleExpr {
    Func::greatest([
        Expr::col((TemporalEntities::Table, TemporalEntities::ModifiedAt)),
        Expr::col((Excluded, TemporalEntities::ModifiedAt)),
    ])
    .into()
}

/// The types of the stored evolution, then those of the proposed row it
/// lacks, in the proposed row's order: the list a conflict action leaves.
fn merged_types() -> SimpleExpr {
    let stored = || Expr::col((TemporalEntities::Table, TemporalEntities::EntityTypeNameKeys));
    let proposed = || Expr::col((Excluded, TemporalEntities::EntityTypeNameKeys));
    let mut lacking = Query::select();
    lacking
        .column((Added, Added))
        .from_function(unnest_of(proposed()), Added)
        .and_where(Expr::col((Added, Added)).ne(all(stored())))
        .order_by_expr(array_position(proposed(), Expr::col((Added, Added))), Order::Asc);
    array_cat(stored(), array_of_subquery(lacking))
}

/// The membership rows of the types each evolution `evolved` returns gained:
/// every type it is recorded under that it was not, which is every type of one
/// the statement created.
///
/// It reads the types the evolution had and has from the images its upsert
/// returned, never from `temporal_entities` itself: the row the upsert
/// replaced is the one it found after waiting on any concurrent writer, while
/// the relation read in the same `WITH` is the statement's snapshot. The
/// upsert only adds types, so no membership row ever leaves.
fn gained_memberships(schema: Option<&DynIden>) -> Result<InsertStatement, SeaQueryError> {
    let memberships = Memberships {
        relation: TypeMembershipRelation::Temporal,
        schema,
    };
    let member = Member {
        key: Expr::col((Evolved, TemporalEntities::TemporalEntityKey)),
        id: Expr::col((Evolved, TemporalEntities::Id)),
    };
    let gained = Expr::col((Evolved, Created)).or(carried_among(Expr::col((Evolved, OldTypes)), memberships).not());
    membership_insert(
        memberships,
        Query::select().from(Evolved).take(),
        Expr::col((Evolved, NewTypes)),
        member,
        Some(gained),
    )
}
