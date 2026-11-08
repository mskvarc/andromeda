//! The Temporal Evolutions a commit's own create established, as the
//! application reads them.
//!
//! A create whose history its commit records inline writes the identity row
//! with the evolution that history establishes and its memberships on both
//! sides. The application leaves such a row where it already holds what the
//! intents establish, and reads it back as an evolution it created, so the
//! streams, the census and the instance judgements see what they see for an
//! evolution the upsert inserted.

use crate::intent_application::intent_source::{StagedSource, source_intents_as};
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents};
use andromeda_postgresql_seaquery::{branch::qualified, cast::cast, literal::true_literal, pg_array::has_elements, pg_type::PostgresType, unnest::unnest_of};
use sea_query::{DynIden, Expr, ExprTrait, Iden, Keyword, Query, SelectStatement, SimpleExpr};

/// The Temporal Evolutions the upsert inserted or rewrote, `upserted`, under
/// the columns of `evolved`, when the commit established some evolutions
/// itself.
#[derive(Iden, Clone, Copy)]
#[iden = "upserted"]
pub(crate) struct Upserted;

/// Alias `est`: the identity row of an Entity whose Temporal Evolution the
/// commit established and the upsert left as it was.
#[derive(Iden, Clone, Copy)]
#[iden = "est"]
struct EstablishedRow;

/// Alias `ei`: an intent of the source stating types for an established
/// Entity.
#[derive(Iden, Clone, Copy)]
#[iden = "ei"]
struct EstablishingIntent;

/// Whether `id` is one of the identifiers the array `established` holds:
/// `id IN (SELECT UNNEST(established))`.
///
/// An uncorrelated sublink, which PostgreSQL evaluates as one hashed subplan,
/// so each row probes a hash table; `id = ANY(established)` over a bound array
/// compares each row with every element.
pub(crate) fn among(id: SimpleExpr, established: &SimpleExpr) -> SimpleExpr {
    id.in_subquery(Query::select().expr(unnest_of(SimpleExpr::clone(established))).take())
}

/// The Temporal Evolutions the commit established that the upsert left as
/// they were, each as an evolution the application created, under the types
/// it holds and none before.
///
/// An established Entity with a typed intent in the source is always
/// proposed to the upsert, which either leaves its row, when the row already
/// holds what the intents establish, or rewrites it and returns it. This
/// reads the first kind: the identity row as the commit's create wrote it,
/// which the statement's snapshot sees, the create having run earlier in the
/// same transaction.
///
/// Every test is an uncorrelated sublink ([`among`]), so each is one hashed
/// subplan however many Entities the commit wrote.
pub(crate) fn left_established(source: &StagedSource<'_>, schema: Option<&DynIden>, established: &SimpleExpr) -> SelectStatement {
    let mut stating = Query::select();
    stating.column((EstablishingIntent, TemporalAppendIntents::EntityId));
    source_intents_as(&mut stating, source, EstablishingIntent);
    stating.and_where(has_elements(Expr::col((EstablishingIntent, TemporalAppendIntents::EntityTypeNameKeys))));
    let rewritten = Query::select().column((Upserted, Entities::Id)).from(Upserted).take();
    let id = || Expr::col((EstablishedRow, Entities::Id));
    let mut rows = Query::select();
    rows.column((EstablishedRow, Entities::EntityKey))
        .column((EstablishedRow, Entities::Id))
        .expr(true_literal())
        .column((EstablishedRow, Entities::EvolutionTypeNameKeys))
        .expr(no_types())
        .from(qualified(schema, Entities::Table).alias(EstablishedRow))
        .and_where(among(id(), established))
        .and_where(id().in_subquery(stating))
        .and_where(id().in_subquery(rewritten).not());
    rows
}

/// A missing type list, `NULL::integer[]`: the types an evolution the
/// statement created held before it.
pub(crate) fn no_types() -> SimpleExpr {
    cast(SimpleExpr::Keyword(Keyword::Null), PostgresType::IntegerArray)
}
