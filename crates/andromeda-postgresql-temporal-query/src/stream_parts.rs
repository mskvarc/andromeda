//! The parts one attribute stream's history is read in, and the Entity's
//! streams a statement reads them for.
//!
//! Every attribute stream (an attribute and a `datasetId`) is read on its own
//! in a LATERAL over the Entity's streams, so the stream-led axis indexes
//! serve each read. A stream's recorded rows and its queued
//! entries are parts read one by one and only then joined, so an order, a
//! limit or a bound reaches the index of each part.

use crate::alias::{Ah, An};
use andromeda_postgresql_query::history_source::{
    HistoryBranch,
    HistoryOf,
    HistoryStream,
    attribute_rows::stream_rows,
    grain::HistoryGrain,
    stream_parts::whole_stream_parts,
};
use andromeda_postgresql_schema::name_dictionary::AttributeNames;
use sea_query::{Asterisk, Condition, Expr, ExprTrait, Iden, IntoIden, JoinType, Query, SelectStatement, SimpleExpr, TableRef, UnionType};

/// Alias `st`: the stream a lateral read narrows to.
#[derive(Iden, Clone, Copy)]
#[iden = "st"]
pub(crate) struct St;

/// Alias `part`: one part's read, joined with the other part's.
#[derive(Iden, Clone, Copy)]
#[iden = "part"]
struct Part;

/// One part of the history of the stream under `st`, read under `ah`.
pub(crate) fn under_ah(part: SelectStatement) -> TableRef {
    TableRef::SubQuery(Box::new(part), Ah.into_iden())
}

/// `read` of one part, its rows under `part` so it can join another read.
pub(crate) fn wrapped(read: SelectStatement) -> SelectStatement {
    let mut wrapped = Query::select();
    wrapped.expr(Expr::col((Part, Asterisk))).from_subquery(read, Part);
    wrapped
}

/// The parts of the stream under `st`: the first, and every other after it.
fn stream_parts(branch: &HistoryBranch) -> (SelectStatement, impl Iterator<Item = SelectStatement>) {
    whole_stream_parts(branch, &HistoryStream::under(St)).into_reads()
}

/// `read` of every part of the stream under `st`, the reads joined.
pub(crate) fn over_parts(branch: &HistoryBranch, read: impl Fn(TableRef) -> SelectStatement) -> SelectStatement {
    let (first, others) = stream_parts(branch);
    let mut joined = wrapped(read(under_ah(first)));
    for other in others {
        joined.union(UnionType::All, wrapped(read(under_ah(other))));
    }
    joined
}

/// Whether `read` of any part of the stream under `st` returns a row.
pub(crate) fn in_any_part(branch: &HistoryBranch, read: impl Fn(TableRef) -> SelectStatement) -> SimpleExpr {
    let (first, others) = stream_parts(branch);
    others.fold(Expr::exists(read(under_ah(first))), |any, other| any.or(Expr::exists(read(under_ah(other)))))
}

/// The instances of `part` that `condition` admits.
pub(crate) fn admitted(part: TableRef, condition: &Condition) -> SelectStatement {
    let mut select = Query::select();
    select.from(part).cond_where(condition.clone());
    select
}

/// The Entity's streams under `st`, each beside the name of its attribute,
/// which every instance of the stream carries.
pub(crate) fn entity_streams(select: &mut SelectStatement, branch: &HistoryBranch, of: &HistoryOf) {
    select.from(stream_rows(branch, of, St)).join_as(
        JoinType::Join,
        AttributeNames::Table,
        An,
        Expr::col((An, AttributeNames::AttributeNameKey)).equals((St, HistoryGrain::AttributeNameKey)),
    );
}
