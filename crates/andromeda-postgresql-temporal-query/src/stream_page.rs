//! A clause 6.3.10 page of one Entity's history, or its `lastN`, read stream
//! by stream.
//!
//! Every attribute stream (an attribute and a `datasetId`) is read on its own
//! in a LATERAL over the Entity's streams, ordered on the requested axis and
//! limited, so the stream-led axis index returns the page and stops: a page
//! costs the instances it keeps, not every instance in the window. A stream's
//! recorded rows and its queued entries are each read that way and only then
//! joined, so the order and the limit reach the index.
//!
//! A forward or backward page keeps the group of instances sharing the instant
//! at its edge whole, so a client resuming from the `Content-Range` end neither
//! skips nor splits it. The read finds the edge first, the instant of the
//! page's last counted instance, and then keeps every instance up to it. A
//! stream holding instances beyond the edge was cut, which the page reports
//! beside its rows as `stream_continues`.
//!
//! An instance carrying no instant on the requested axis has no position on
//! it. A window bounded on the axis holds none; an unbounded window holds them
//! after every dated instance as one group, so a page keeps them only when it
//! keeps every dated instance, and a `lastN`, most recent first, takes them
//! ahead of the dated ones. They are read apart from the dated instances, so
//! the axis index, which holds dated instances alone, still serves the page.

use crate::{
    alias::{Ah, An},
    axis::axis_column,
    history::project_base_columns,
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, window::TimeInterval};
use andromeda_postgresql_query::history_source::{
    HistoryBranch,
    HistoryOf,
    HistoryStream,
    attribute_rows::{stream_attribute_parts, stream_rows},
    grain::HistoryGrain,
};
use andromeda_postgresql_schema::{name_dictionary::AttributeNames, temporal::AttributeHistory};
use andromeda_postgresql_seaquery::{
    literal::int_literal,
    open_instant::{or_end_of_time, or_start_of_time},
};
use sea_query::{Alias, Asterisk, Condition, Expr, ExprTrait, Iden, IntoIden, JoinType, Order, Query, SelectStatement, SimpleExpr, TableRef, UnionType};
use std::num::NonZeroU32;

/// Alias `st`: the stream a lateral read narrows to.
#[derive(Iden, Clone, Copy)]
#[iden = "st"]
struct St;

/// Alias `part`: one part's read, joined with the other part's.
#[derive(Iden, Clone, Copy)]
#[iden = "part"]
struct Part;

/// Alias `edge_candidates`: the instants each part could place a page's edge
/// at.
#[derive(Iden, Clone, Copy)]
#[iden = "edge_candidates"]
struct EdgeCandidates;

/// Alias `edge_row`: the instant at the edge of a stream's page.
#[derive(Iden, Clone, Copy)]
#[iden = "edge_row"]
struct EdgeRow;

/// Alias `stream_edge`: the edge of a stream's page and whether the stream
/// runs on beyond it.
#[derive(Iden, Clone, Copy)]
#[iden = "stream_edge"]
struct StreamEdge;

/// Alias `page`: the instances one stream's page keeps.
#[derive(Iden, Clone, Copy)]
#[iden = "page"]
struct Page;

/// Alias `latest`: the most recent instances of every part before the last
/// `n` of them are taken.
#[derive(Iden, Clone, Copy)]
#[iden = "latest"]
struct Latest;

/// Column `edge`: the instant on the axis of a page's last counted instance.
#[derive(Iden, Clone, Copy)]
#[iden = "edge"]
struct Edge;

/// Column `stream_continues`: whether the stream holds instances in the window
/// beyond its page.
#[derive(Iden, Clone, Copy)]
#[iden = "stream_continues"]
struct StreamContinues;

/// The direction a page walks a stream in (clause 6.3.10).
#[derive(Clone, Copy)]
pub(crate) enum Walk {
    /// Without `lastN`: from the least recent instant.
    Forward,
    /// With a `lastN` above the broker's maximum: from the most recent instant.
    Backward,
}

impl Walk {
    /// The axis order the walk reads a stream in.
    const fn order(self) -> Order {
        match self {
            Walk::Forward => Order::Asc,
            Walk::Backward => Order::Desc,
        }
    }
}

/// Whether the window can hold instances with no instant on the axis.
#[derive(Clone, Copy)]
enum UndatedInstances {
    /// An unbounded window on an axis an instance may lack.
    Admitted,
    /// A window bounded on the axis, or the `deletedAt` axis that only a
    /// deletion carries and that selects those alone (clause 5.2.21).
    Excluded,
}

/// Whether the window `compiled` reads can hold instances with no instant on
/// its axis.
const fn undated_instances(compiled: &CompiledTemporalQuery) -> UndatedInstances {
    match (compiled.window.interval, compiled.window.timeproperty) {
        (TimeInterval::Unbounded, TimeProp::Observed | TimeProp::Created | TimeProp::Modified) => UndatedInstances::Admitted,
        (TimeInterval::Unbounded, TimeProp::Deleted) | (TimeInterval::Before(_) | TimeInterval::After(_) | TimeInterval::Between { .. }, _) => {
            UndatedInstances::Excluded
        }
    }
}

/// One part of the history of the stream under `st`, read under `ah`.
fn under_ah(part: SelectStatement) -> TableRef {
    TableRef::SubQuery(Box::new(part), Ah.into_iden())
}

/// `read` of one part, its rows under `part` so it can join another read.
fn wrapped(read: SelectStatement) -> SelectStatement {
    let mut wrapped = Query::select();
    wrapped.expr(Expr::col((Part, Asterisk))).from_subquery(read, Part);
    wrapped
}

/// `read` of every part of the stream under `st`, the reads joined.
fn over_parts(branch: &HistoryBranch, read: impl Fn(TableRef) -> SelectStatement) -> SelectStatement {
    let parts = stream_attribute_parts(branch, &HistoryStream::under(St));
    let mut joined = wrapped(read(under_ah(parts.recorded)));
    if let Some(pending) = parts.pending {
        joined.union(UnionType::All, wrapped(read(under_ah(pending))));
    }
    joined
}

/// Whether `read` of any part of the stream under `st` returns a row.
fn in_any_part(branch: &HistoryBranch, read: impl Fn(TableRef) -> SelectStatement) -> SimpleExpr {
    let parts = stream_attribute_parts(branch, &HistoryStream::under(St));
    let recorded = Expr::exists(read(under_ah(parts.recorded)));
    match parts.pending {
        Some(pending) => recorded.or(Expr::exists(read(under_ah(pending)))),
        None => recorded,
    }
}

/// The instances of `part` that `condition` admits.
fn admitted(part: TableRef, condition: &Condition) -> SelectStatement {
    let mut select = Query::select();
    // clone: every read of the stream keeps the same represented instances.
    select.from(part).cond_where(condition.clone());
    select
}

/// The edge of the page of `size` the stream under `st` is walked to, and
/// whether the stream runs on beyond it; no row when the stream's dated
/// instances fill no page.
///
/// A forward page leaves the instances on the inclusive lower bound of
/// `after` and `between` (clause 4.11) uncounted: they are the group the
/// previous page ended on, and counting them could stall a resumed walk on
/// that one instant.
fn stream_edge(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, condition: &Condition, walk: Walk, size: u32) -> SelectStatement {
    let axis = axis_column(compiled.window.timeproperty);
    let candidates = over_parts(branch, |part| {
        let mut candidate = admitted(part, condition);
        candidate.expr_as(Expr::col((Ah, axis)), Edge).and_where(Expr::col((Ah, axis)).is_not_null());
        match (walk, compiled.window.interval) {
            (Walk::Forward, TimeInterval::After(lower) | TimeInterval::Between { from: lower, .. }) => {
                candidate.and_where(Expr::col((Ah, axis)).gt(lower));
            }
            (Walk::Forward, TimeInterval::Unbounded | TimeInterval::Before(_)) | (Walk::Backward, _) => {}
        }
        candidate.order_by((Ah, axis), walk.order()).limit(u64::from(size));
        candidate
    });
    let mut edge_row = Query::select();
    edge_row
        .expr(Expr::col((EdgeCandidates, Edge)))
        .from_subquery(candidates, EdgeCandidates)
        .order_by((EdgeCandidates, Edge), walk.order())
        .offset(u64::from(size.saturating_sub(1)))
        .limit(1);

    let edge = Expr::col((EdgeRow, Edge));
    let beyond = in_any_part(branch, |part| {
        let mut beyond = admitted(part, condition);
        beyond.expr(int_literal(1)).and_where(match walk {
            // clone: the edge bounds every part's instances and is projected.
            Walk::Forward => Expr::col((Ah, axis)).gt(edge.clone()),
            Walk::Backward => Expr::col((Ah, axis)).lt(edge.clone()),
        });
        beyond
    });
    let continues = match undated_instances(compiled) {
        UndatedInstances::Admitted => beyond.or(in_any_part(branch, |part| {
            let mut undated = admitted(part, condition);
            undated.expr(int_literal(1)).and_where(Expr::col((Ah, axis)).is_null());
            undated
        })),
        UndatedInstances::Excluded => beyond,
    };

    let mut select = Query::select();
    select.expr_as(edge, Edge).expr_as(continues, StreamContinues).from_subquery(edge_row, EdgeRow);
    select
}

/// The instances the page of the stream under `st` keeps: every dated
/// instance up to the edge `stream_edge` holds, and the undated ones when the
/// page holds every dated instance.
///
/// The read ends in `OFFSET 0`, so PostgreSQL keeps it a subquery rather than
/// pulling it up into the outer join. Pulled up, its bound on the edge becomes
/// a join filter, and a planner that estimates a stream to hold a few rows in
/// the window (most streams of a broker beside a few dense ones) reads the
/// stream's rows first and finds the edge again for every one of them.
fn kept_instances(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, condition: &Condition, walk: Walk) -> SelectStatement {
    let axis = axis_column(compiled.window.timeproperty);
    let edge = Expr::col((StreamEdge, Edge));
    let mut kept = over_parts(branch, |part| {
        let mut dated = admitted(part, condition);
        project_base_columns(&mut dated);
        dated.and_where(match walk {
            // clone: the edge bounds every part's instances.
            Walk::Forward => Expr::col((Ah, axis)).lte(or_end_of_time(edge.clone())),
            Walk::Backward => Expr::col((Ah, axis)).gte(or_start_of_time(edge.clone())),
        });
        dated
    });
    match undated_instances(compiled) {
        UndatedInstances::Admitted => {
            kept.union(
                UnionType::All,
                over_parts(branch, |part| {
                    let mut undated = admitted(part, condition);
                    project_base_columns(&mut undated);
                    // clone: the edge gates every part's undated instances.
                    undated.and_where(Expr::col((Ah, axis)).is_null()).and_where(edge.clone().is_null());
                    undated
                }),
            );
        }
        UndatedInstances::Excluded => {}
    }
    kept.offset(0);
    kept
}

/// The Entity's streams under `st`, each beside the name of its attribute,
/// which every instance of the stream carries.
fn entity_streams(select: &mut SelectStatement, branch: &HistoryBranch, of: &HistoryOf) {
    select.from(stream_rows(branch, of, St)).join_as(
        JoinType::Join,
        AttributeNames::Table,
        An,
        Expr::col((An, AttributeNames::AttributeNameKey)).equals((St, HistoryGrain::AttributeNameKey)),
    );
}

/// Order the page rows under `page` as the reconstruction reads them: by
/// attribute, then on the axis, ties broken on the hypertable's unique key.
fn in_rendering_order(select: &mut SelectStatement, axis: AttributeHistory) {
    select
        .order_by((Page, Alias::new("attribute_name")), Order::Asc)
        .order_by((Page, axis), Order::Asc)
        .order_by((Page, AttributeHistory::RecordedAt), Order::Asc)
        .order_by((Page, AttributeHistory::WriteId), Order::Asc)
        .order_by((Page, AttributeHistory::WriteOrdinal), Order::Asc);
}

/// Clause 6.3.10: the page of `size` instances of every stream the Entity
/// `of` names holds, walked in the direction `walk`, each row carrying whether
/// its stream runs on beyond the page.
pub(crate) fn walked_history(
    branch: &HistoryBranch,
    of: &HistoryOf,
    compiled: &CompiledTemporalQuery,
    condition: &Condition,
    walk: Walk,
    size: u32,
) -> SelectStatement {
    let mut select = Query::select();
    select
        .expr(Expr::col((Page, Asterisk)))
        .expr_as(Expr::col((StreamEdge, StreamContinues)), StreamContinues);
    entity_streams(&mut select, branch, of);
    select
        .join_lateral(
            JoinType::LeftJoin,
            stream_edge(branch, compiled, condition, walk, size),
            StreamEdge,
            SimpleExpr::from(true),
        )
        .join_lateral(JoinType::Join, kept_instances(branch, compiled, condition, walk), Page, SimpleExpr::from(true));
    in_rendering_order(&mut select, axis_column(compiled.window.timeproperty));
    select
}

/// Order `select` most recent first over the columns under `alias`, ties
/// broken on the hypertable's unique key so two instances sharing an instant
/// are kept or dropped the same way on every execution. An instance with no
/// instant sorts first under the descending order, as it would in a single
/// ranking over the window; `axis` is `None` for a read of those alone.
fn most_recent_first(select: &mut SelectStatement, alias: impl IntoIden, axis: Option<AttributeHistory>) {
    let alias = alias.into_iden();
    if let Some(axis) = axis {
        // clone: every ordering key names the alias.
        select.order_by((alias.clone(), axis), Order::Desc);
    }
    select
        .order_by((alias.clone(), AttributeHistory::RecordedAt), Order::Desc)
        .order_by((alias.clone(), AttributeHistory::WriteId), Order::Desc)
        .order_by((alias, AttributeHistory::WriteOrdinal), Order::Desc);
}

/// The last `n` instances of the stream under `st`.
fn latest_instances(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, condition: &Condition, n: NonZeroU32) -> SelectStatement {
    let axis = axis_column(compiled.window.timeproperty);
    let limit = u64::from(n.get());
    let mut candidates = over_parts(branch, |part| {
        let mut dated = admitted(part, condition);
        project_base_columns(&mut dated);
        dated.and_where(Expr::col((Ah, axis)).is_not_null());
        most_recent_first(&mut dated, Ah, Some(axis));
        dated.limit(limit);
        dated
    });
    match undated_instances(compiled) {
        UndatedInstances::Admitted => {
            candidates.union(
                UnionType::All,
                over_parts(branch, |part| {
                    let mut undated = admitted(part, condition);
                    project_base_columns(&mut undated);
                    undated.and_where(Expr::col((Ah, axis)).is_null());
                    most_recent_first(&mut undated, Ah, None);
                    undated.limit(limit);
                    undated
                }),
            );
        }
        UndatedInstances::Excluded => {}
    }
    let mut latest = Query::select();
    latest.expr(Expr::col((Latest, Asterisk))).from_subquery(candidates, Latest);
    most_recent_first(&mut latest, Latest, Some(axis));
    latest.limit(limit);
    latest
}

/// Clauses 5.7.3.4 and 5.7.4.4: a `lastN` the broker answers in full is
/// exactly the last `n` instances of every stream the Entity `of` names.
pub(crate) fn last_n_history(
    branch: &HistoryBranch,
    of: &HistoryOf,
    compiled: &CompiledTemporalQuery,
    condition: &Condition,
    n: NonZeroU32,
) -> SelectStatement {
    let mut select = Query::select();
    select.expr(Expr::col((Page, Asterisk)));
    entity_streams(&mut select, branch, of);
    select.join_lateral(JoinType::Join, latest_instances(branch, compiled, condition, n), Page, SimpleExpr::from(true));
    in_rendering_order(&mut select, axis_column(compiled.window.timeproperty));
    select
}
