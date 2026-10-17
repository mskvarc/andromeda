//! What a probe of one attribute stream's page of periods reads: an instance
//! of the stream found by one ordered, limited read of each of its parts on
//! the requested axis, so the stream-led axis index serves every probe.

use crate::{
    alias::Ah,
    axis::axis_column,
    stream_page::Walk,
    stream_parts::{admitted, over_parts},
};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::scalar_subquery::scalar_subquery;
use sea_query::{Condition, Expr, ExprTrait, Func, Iden, Query, SelectStatement, SimpleExpr};

/// Alias `found`: the instance a probe found.
#[derive(Iden, Clone, Copy)]
#[iden = "found"]
pub(crate) struct Found;

/// Column `instant`: the instant on the axis of the instance a probe found.
#[derive(Iden, Clone, Copy)]
#[iden = "instant"]
pub(crate) struct Instant;

/// What a probe of the stream under `st` reads: the represented instances of
/// its parts on the requested axis.
#[derive(Clone, Copy)]
pub(crate) struct StreamProbes<'q> {
    /// The branch the stream is read in.
    pub(crate) branch: &'q HistoryBranch,
    /// The query, which names the axis and the parts.
    pub(crate) compiled: &'q CompiledTemporalQuery,
    /// The represented instances.
    pub(crate) condition: &'q Condition,
}

impl StreamProbes<'_> {
    /// The axis column the probes read.
    const fn axis(&self) -> AttributeHistory {
        axis_column(self.compiled.window.timeproperty)
    }

    /// The instant of the instance `offset` places past the first one `walk`
    /// reaches among those `bound` admits, every part read in that order and
    /// limited on its own before the parts are joined; no row when there are
    /// not that many.
    pub(crate) fn instant(&self, walk: Walk, bound: Option<&SimpleExpr>, offset: u64) -> SelectStatement {
        let axis = self.axis();
        let candidates = over_parts(self.branch, self.compiled, |part| {
            let mut candidate = admitted(part, self.condition);
            candidate.expr_as(Expr::col((Ah, axis)), Instant).and_where(Expr::col((Ah, axis)).is_not_null());
            if let Some(bound) = bound {
                // clone: every part is bounded by the same instant.
                candidate.and_where(bound.clone());
            }
            candidate.order_by((Ah, axis), walk.order()).limit(offset + 1);
            candidate
        });
        let mut instant = Query::select();
        instant
            .expr(Expr::col((Found, Instant)))
            .from_subquery(candidates, Found)
            .order_by((Found, Instant), walk.order())
            .offset(offset)
            .limit(1);
        instant
    }

    /// The bound a probe walking `walk` from `from` reads beyond: forwards the
    /// instants at or after it, backwards the instants before it.
    pub(crate) fn beyond(&self, walk: Walk, from: SimpleExpr) -> SimpleExpr {
        let axis = Expr::col((Ah, self.axis()));
        match walk {
            Walk::Forward => axis.gte(from),
            Walk::Backward => axis.lt(from),
        }
    }

    /// The first instant `walk` reaches beyond `from`, NULL when there is
    /// none.
    ///
    /// The search runs first among the instants within one period of `from`
    /// and only then beyond: either way it finds the first instant past
    /// `from`, whatever bound splits the two searches. On an uncompressed
    /// chunk each search is one descent of the stream's axis index. A
    /// compressed chunk has no such index, but summarises each batch by its
    /// least and most recent instant, and a search bounded on both sides
    /// decompresses only the batches overlapping it, where one bounded on a
    /// single side would decompress every batch beyond `from`. `COALESCE`
    /// runs the search beyond only when the nearer one finds nothing.
    pub(crate) fn next_instant(&self, walk: Walk, from: &SimpleExpr, width: SimpleExpr) -> SelectStatement {
        let axis = || Expr::col((Ah, self.axis()));
        // clone: the bound one period on is read by both searches.
        let step = match walk {
            Walk::Forward => from.clone().add(width),
            Walk::Backward => from.clone().sub(width),
        };
        let near = match walk {
            // clone: the near search is bounded by both instants.
            Walk::Forward => axis().gte(from.clone()).and(axis().lt(step.clone())),
            Walk::Backward => axis().lt(from.clone()).and(axis().gte(step.clone())),
        };
        let near = scalar_subquery(self.instant(walk, Some(&near), 0));
        let far = scalar_subquery(self.instant(walk, Some(&self.beyond(walk, step)), 0));
        let mut next = Query::select();
        // `OFFSET 0` keeps the read a subquery: pulled up, each use of the
        // instant would run the searches again.
        next.expr_as(Func::coalesce([near, far]), Instant).offset(0);
        next
    }

    /// The instants on the kept side of `bound`, an edge as [`open_bound`]
    /// writes it: forwards those before it, backwards those at or after it.
    ///
    /// The comparison names the bound as a column, not the expression that
    /// opens it, so a compressed chunk compares it with each batch's least
    /// and most recent instant and decompresses only the batches it admits.
    pub(crate) fn kept_side(&self, walk: Walk, bound: SimpleExpr) -> SimpleExpr {
        let axis = Expr::col((Ah, self.axis()));
        match walk {
            Walk::Forward => axis.lt(bound),
            Walk::Backward => axis.gte(bound),
        }
    }
}
