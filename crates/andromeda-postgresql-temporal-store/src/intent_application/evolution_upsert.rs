//! The Temporal Evolutions an application writes, and the type memberships
//! their types gain.
//!
//! Clause 4.5.6 makes the Temporal Evolution "the Entity [...] as mandated by
//! clause 4.5.1", and clauses 5.6.11.4 and 5.6.12.4 add the Entity Type names a
//! write states that it does not hold yet to its list. No operation removes
//! one, so the list only grows, and the membership relation a temporal type
//! selection walks only gains rows.

use crate::intent_application::{
    established_evolutions::{Upserted, among, left_established, no_types},
    intent_source::{I, StagedSource, intent, source_intents, source_intents_as},
};
use andromeda_postgresql_query::{
    type_list_union::{TypeListColumn, type_list_union},
    type_membership::Memberships,
    type_membership_moves::{Member, carried_among, membership_insert},
};
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents, type_membership::MembershipSide};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    cast::cast,
    conflict_image::Excluded,
    literal::{false_literal, text_literal},
    pg_array::{all, array_cat, array_of_subquery, array_position, has_elements},
    pg_type::PostgresType,
    returning_image::{New, Old},
    unnest::unnest_of,
};
use sea_query::{
    CaseStatement,
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
    UnionType,
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
pub(crate) struct NewTypes;

/// The types the evolution was recorded under before the statement, null for
/// one it created, `old_types`, a column of `evolved`.
#[derive(Iden, Clone, Copy)]
#[iden = "old_types"]
pub(crate) struct OldTypes;

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

/// The columns `evolved` and `upserted` carry.
fn evolution_columns() -> [DynIden; 5] {
    [
        Entities::EntityKey.into_iden(),
        Entities::Id.into_iden(),
        Created.into_iden(),
        NewTypes.into_iden(),
        OldTypes.into_iden(),
    ]
}

/// Add to `with` the evolutions of the Entities the intents of `source` state
/// types for, as `evolved` ([`evolutions_statement`]), and the membership rows
/// of the types they gained, as `added_members` ([`gained_memberships`]).
///
/// When the commit established some evolutions itself, the upsert runs as
/// `upserted` and `evolved` adds the established evolutions it left alone
/// ([`left_established`]); the memberships are read from `upserted` alone,
/// since the create that established an evolution wrote its memberships
/// with both flags.
///
/// # Errors
///
/// Returns [`SeaQueryError`] if a projection ever stops matching its column
/// list, which the fixed shapes here make unreachable.
pub(crate) fn with_evolutions(with: &mut WithClause, source: &StagedSource<'_>, schema: Option<&DynIden>) -> Result<(), SeaQueryError> {
    let established = source.established_evolutions();
    let upsert = evolutions_statement(source, schema, established.as_ref())?;
    let written = match established {
        None => {
            with.cte(
                CommonTableExpression::new()
                    .table_name(Evolved)
                    .columns(evolution_columns())
                    .query(upsert)
                    .to_owned(),
            );
            Evolved.into_iden()
        }
        Some(established) => {
            let mut evolved = Query::select();
            evolved
                .columns(evolution_columns().map(|column| (Upserted, column)))
                .from(Upserted)
                .union(UnionType::All, left_established(source, schema, &established));
            with.cte(
                CommonTableExpression::new()
                    .table_name(Upserted)
                    .columns(evolution_columns())
                    .query(upsert)
                    .to_owned(),
            )
            .cte(
                CommonTableExpression::new()
                    .table_name(Evolved)
                    .columns(evolution_columns())
                    .query(evolved)
                    .to_owned(),
            );
            Upserted.into_iden()
        }
    };
    with.cte(
        CommonTableExpression::new()
            .table_name(AddedMembers)
            .query(gained_memberships(schema, &written)?)
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
///
/// An Entity in `established` holds the evolution its commit's create wrote,
/// which is what the upsert writes into an identity row holding none. Its
/// conflict action takes the inserted row's values as they are, and writes
/// nothing where the row already holds them, so the create's row stays the
/// only version. Such an evolution, when rewritten, is returned as one the
/// statement created, with no types before.
fn evolutions_statement(source: &StagedSource<'_>, schema: Option<&DynIden>, established: Option<&SimpleExpr>) -> Result<InsertStatement, SeaQueryError> {
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
        .into_table(qualified(schema, Entities::Table))
        .columns([Entities::Id].into_iter().chain(MERGED_COLUMNS.map(|(column, _)| column)))
        .select_from(select)?;
    match established {
        None => {
            statement
                .on_conflict(
                    OnConflict::column(Entities::Id)
                        .values(MERGED_COLUMNS.map(|(column, merged)| (column, merged())))
                        .to_owned(),
                )
                .returning(Query::returning().exprs([
                    Expr::col((New, Entities::EntityKey)),
                    Expr::col((New, Entities::Id)),
                    Expr::col((Old, Entities::EvolutionCreatedAt)).is_null(),
                    Expr::col((New, Entities::EvolutionTypeNameKeys)),
                    Expr::col((Old, Entities::EvolutionTypeNameKeys)),
                ]));
        }
        Some(established) => {
            let stored_established = || among(Expr::col((Entities::Table, Entities::Id)), established);
            let returned_established = || among(Expr::col((New, Entities::Id)), established);
            let held = MERGED_COLUMNS.into_iter().fold(stored_established(), |held, (column, _)| {
                held.and(Expr::col((Entities::Table, column)).eq(Expr::col((Excluded, column))))
            });
            statement
                .on_conflict(
                    OnConflict::column(Entities::Id)
                        .values(MERGED_COLUMNS.map(|(column, merged)| {
                            let value: SimpleExpr = CaseStatement::new()
                                .case(stored_established(), Expr::col((Excluded, column)))
                                .finally(merged())
                                .into();
                            (column, value)
                        }))
                        .action_and_where(held.not())
                        .to_owned(),
                )
                .returning(
                    Query::returning().exprs([
                        Expr::col((New, Entities::EntityKey)),
                        Expr::col((New, Entities::Id)),
                        Expr::col((Old, Entities::EvolutionCreatedAt)).is_null().or(returned_established()),
                        Expr::col((New, Entities::EvolutionTypeNameKeys)),
                        CaseStatement::new()
                            .case(returned_established(), no_types())
                            .finally(Expr::col((Old, Entities::EvolutionTypeNameKeys)))
                            .into(),
                    ]),
                );
        }
    }
    Ok(statement)
}

/// The evolution columns of the identity row the upsert writes, each with the
/// value its conflict action merges into a stored evolution.
const MERGED_COLUMNS: [(Entities, fn() -> SimpleExpr); 3] = [
    (Entities::EvolutionCreatedAt, first_creation),
    (Entities::EvolutionModifiedAt, latest_modification),
    (Entities::EvolutionTypeNameKeys, merged_types),
];

/// Every type the intents of `source` state, one list per Entity, under the
/// columns of [`type_list_union`]: each type once, in the order of the first
/// intent stating it.
fn stated_types(source: &StagedSource<'_>) -> SelectStatement {
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

/// The stored evolution's `createdAt`, or the proposed row's for an identity
/// row that held no Temporal Evolution yet: an Entity whose current state
/// was written before any history was recorded, or whose Evolution a clause
/// 5.6.16 deletion removed.
fn first_creation() -> SimpleExpr {
    Func::coalesce([
        Expr::col((Entities::Table, Entities::EvolutionCreatedAt)),
        Expr::col((Excluded, Entities::EvolutionCreatedAt)),
    ])
    .into()
}

/// The later of the stored evolution's `modified_at` and the proposed row's,
/// the proposed one alone where the identity row held no evolution.
///
/// Clause 4.8 makes `modifiedAt` the instant the Entity was last modified.
/// Writers whose history is not applied in entity order (a Temporal API write
/// whose fold stores no entity row) can commit after a later-stamped one, and
/// that later instant stays the last modification.
fn latest_modification() -> SimpleExpr {
    Func::greatest([
        Expr::col((Entities::Table, Entities::EvolutionModifiedAt)),
        Expr::col((Excluded, Entities::EvolutionModifiedAt)),
    ])
    .into()
}

/// The types of the stored evolution, then those of the proposed row it
/// lacks, in the proposed row's order: the list a conflict action leaves. An
/// identity row holding no evolution holds no type yet.
fn merged_types() -> SimpleExpr {
    let stored = || -> SimpleExpr {
        Func::coalesce([
            Expr::col((Entities::Table, Entities::EvolutionTypeNameKeys)),
            cast(text_literal("{}"), PostgresType::IntegerArray),
        ])
        .into()
    };
    let proposed = || Expr::col((Excluded, Entities::EvolutionTypeNameKeys));
    let mut lacking = Query::select();
    lacking
        .column((Added, Added))
        .from_function(unnest_of(proposed()), Added)
        .and_where(Expr::col((Added, Added)).ne(all(stored())))
        .order_by_expr(array_position(proposed(), Expr::col((Added, Added))), Order::Asc);
    array_cat(stored(), array_of_subquery(lacking))
}

/// The membership rows of the types each evolution `written` returns gained:
/// every type it is recorded under that it was not, which is every type of one
/// the statement created.
///
/// It reads the types the evolution had and has from the images its upsert
/// returned, never from `entities` itself: the row the upsert
/// replaced is the one it found after waiting on any concurrent writer, while
/// the relation read in the same `WITH` is the statement's snapshot. The
/// upsert only adds types, so no membership row ever leaves.
fn gained_memberships(schema: Option<&DynIden>, written: &DynIden) -> Result<InsertStatement, SeaQueryError> {
    let memberships = Memberships {
        side: MembershipSide::Evolution,
        schema,
    };
    // clone: every column names the same relation.
    let column = |name: DynIden| Expr::col((DynIden::clone(written), name));
    let member = Member {
        key: column(Entities::EntityKey.into_iden()),
        id: column(Entities::Id.into_iden()),
    };
    let gained = column(Created.into_iden()).or(carried_among(column(OldTypes.into_iden())).not());
    membership_insert(
        memberships,
        // clone: the rows and their columns name the same relation.
        Query::select().from(DynIden::clone(written)).take(),
        column(NewTypes.into_iden()),
        member,
        Some(gained),
        false_literal(),
    )
}
