//! A clause 6.3.10 page of one Entity's history, or its `lastN`, read stream
//! by stream.
//!
//! Every part of every attribute stream is read ordered on the requested axis
//! and limited, so the stream-led axis index returns the page and stops: a
//! page costs the instances it keeps, not every instance in the window.
//!
//! A forward or backward page keeps the group of instances sharing the instant
//! at its edge whole, so a client resuming from the `Content-Range` end neither
//! skips nor splits it. The read finds the edge first, the instant of the
//! page's last counted instance, and then keeps every instance up to it. A
//! stream holding instances beyond the edge was cut, which the page reports
//! beside its rows as `stream_continues`. A read that pages no stream projects
//! the same column as `FALSE`, so every history row says whether its stream was
//! cut and a reader never infers it from a column that is missing.
//!
//! Every represented instance carries an instant on the requested axis, so the
//! stream-led index a part is ordered and bounded on serves every read here,
//! and a page, a walk over every page and the last `n` instances all hold the
//! same instances (clauses 5.7.3.4 and 6.3.10).

use crate::{
    alias::Ah,
    history::project_base_columns,
    stream_parts::{admitted, entity_streams, in_any_part, over_parts},
};
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, window::TimeInterval};
use andromeda_postgresql_query::history_source::{
    HistoryBranch,
    HistoryOf,
    axis::{axis_bound, axis_column, axis_order},
};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    literal::{false_literal, int_literal},
    open_instant::{or_end_of_time, or_start_of_time},
};
use sea_query::{Alias, Asterisk, Condition, Expr, ExprTrait, Iden, IntoIden, JoinType, Order, Query, SelectStatement, SimpleExpr};
use std::num::NonZeroU32;

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
pub(crate) struct StreamContinues;

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
    pub(crate) const fn order(self) -> Order {
        match self {
            Walk::Forward => Order::Asc,
            Walk::Backward => Order::Desc,
        }
    }
}

/// The edge of the page of `size` the stream under `st` is walked to, and
/// whether the stream runs on beyond it; no row when the stream's instances
/// fill no page.
///
/// A forward page leaves the instances on the inclusive lower bound of
/// `after` and `between` (clause 4.11) uncounted: they are the group the
/// previous page ended on, and counting them could stall a resumed walk on
/// that one instant.
fn stream_edge(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, condition: &Condition, walk: Walk, size: u32) -> SelectStatement {
    let timeproperty = compiled.window.timeproperty;
    let candidates = over_parts(branch, |part| {
        let mut candidate = admitted(part, condition);
        candidate.expr_as(Expr::col((Ah, axis_column(timeproperty))), Edge);
        match (walk, compiled.window.interval) {
            (Walk::Forward, TimeInterval::After(lower) | TimeInterval::Between { from: lower, .. }) => {
                candidate.and_where(axis_bound(Ah, timeproperty, |instant| instant.gt(lower)));
            }
            (Walk::Forward, TimeInterval::Unbounded | TimeInterval::Before(_)) | (Walk::Backward, _) => {}
        }
        candidate.order_by_expr(axis_order(Ah, timeproperty), walk.order()).limit(u64::from(size));
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
        beyond.expr(int_literal(1)).and_where(axis_bound(Ah, timeproperty, |instant| match walk {
            // clone: the edge bounds every part's instances and is projected.
            Walk::Forward => instant.gt(edge.clone()),
            Walk::Backward => instant.lt(edge.clone()),
        }));
        beyond
    });

    let mut select = Query::select();
    select.expr_as(edge, Edge).expr_as(beyond, StreamContinues).from_subquery(edge_row, EdgeRow);
    select
}

/// The instances the page of the stream under `st` keeps: every instance up
/// to the edge `stream_edge` holds, or every instance when the stream fills no
/// page.
///
/// The read ends in `OFFSET 0`, so PostgreSQL keeps it a subquery rather than
/// pulling it up into the outer join. Pulled up, its bound on the edge becomes
/// a join filter, and a planner that estimates a stream to hold a few rows in
/// the window (most streams of a broker beside a few dense ones) reads the
/// stream's rows first and finds the edge again for every one of them.
fn kept_instances(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, condition: &Condition, walk: Walk) -> SelectStatement {
    let edge = Expr::col((StreamEdge, Edge));
    let mut kept = over_parts(branch, |part| {
        let mut rows = admitted(part, condition);
        project_base_columns(&mut rows);
        rows.and_where(axis_bound(Ah, compiled.window.timeproperty, |instant| match walk {
            // clone: the edge bounds every part's instances.
            Walk::Forward => instant.lte(or_end_of_time(edge.clone())),
            Walk::Backward => instant.gte(or_start_of_time(edge.clone())),
        }));
        rows
    });
    kept.offset(0);
    kept
}

/// Project that `select` answers every stream whole: a read that pages no
/// stream cuts none.
pub(crate) fn project_whole_streams(select: &mut SelectStatement) {
    select.expr_as(false_literal(), StreamContinues);
}

/// Order the history rows under `rows` as the reconstruction reads them: by
/// attribute, then on the axis, ties broken on the hypertable's unique key.
pub(crate) fn in_rendering_order(select: &mut SelectStatement, rows: impl IntoIden, axis: AttributeHistory) {
    let rows = rows.into_iden();
    // clone: every ordering key names the rows' alias.
    select
        .order_by((rows.clone(), Alias::new("attribute_name")), Order::Asc)
        .order_by((rows.clone(), axis), Order::Asc)
        .order_by((rows.clone(), AttributeHistory::RecordedAt), Order::Asc)
        .order_by((rows.clone(), AttributeHistory::WriteId), Order::Asc)
        .order_by((rows, AttributeHistory::WriteOrdinal), Order::Asc);
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
    select
}

/// Order `select` most recent first on `instant` and then on the columns
/// under `alias`, ties broken on the hypertable's unique key so two instances
/// sharing an instant are kept or dropped the same way on every execution.
fn most_recent_first(select: &mut SelectStatement, alias: impl IntoIden, instant: SimpleExpr) {
    let alias = alias.into_iden();
    // clone: every ordering key names the alias.
    select
        .order_by_expr(instant, Order::Desc)
        .order_by((alias.clone(), AttributeHistory::RecordedAt), Order::Desc)
        .order_by((alias.clone(), AttributeHistory::WriteId), Order::Desc)
        .order_by((alias, AttributeHistory::WriteOrdinal), Order::Desc);
}

/// The last `n` instances of the stream under `st`, "the last n timestamps
/// (in descending ordering) of the temporal property" of clauses 5.7.3.4 and
/// 5.7.4.4, each with the columns `project` projects.
pub(crate) fn latest_instances(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    condition: &Condition,
    n: NonZeroU32,
    project: fn(&mut SelectStatement),
) -> SelectStatement {
    let timeproperty = compiled.window.timeproperty;
    let limit = u64::from(n.get());
    let candidates = over_parts(branch, |part| {
        let mut latest = admitted(part, condition);
        project(&mut latest);
        most_recent_first(&mut latest, Ah, axis_order(Ah, timeproperty));
        latest.limit(limit);
        latest
    });
    let mut latest = Query::select();
    latest.expr(Expr::col((Latest, Asterisk))).from_subquery(candidates, Latest);
    most_recent_first(&mut latest, Latest, Expr::col((Latest, axis_column(timeproperty))));
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
    project_whole_streams(&mut select);
    entity_streams(&mut select, branch, of);
    select.join_lateral(
        JoinType::Join,
        latest_instances(branch, compiled, condition, n, project_base_columns),
        Page,
        SimpleExpr::from(true),
    );
    select
}
