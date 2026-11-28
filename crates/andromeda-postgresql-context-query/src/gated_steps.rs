//! The branches of a gated page as they are added in the order the page tries
//! them, the CTEs they read and the gates each later branch inherits from the
//! steps before it.

use crate::{
    entity_page::{GATED_STEP_COLUMN, GatedStep, StepColumn, TOTAL_COUNT_COLUMN},
    gated_page::{GatedColumns, LeadingRead, PageSource, cut},
};
use andromeda_ngsi_ld_query::count_policy::CountPolicy;
use andromeda_postgresql_query::{
    error::RenderError,
    one_time_gate::{CandidateBound, Candidates, IndexEntries, beyond, bounded, column_of, materialized, narrowed_to, within, wrapped},
    sort_key::KeyOrder,
};
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::aggregate::count_over_all;
use sea_query::{Alias, Asterisk, Condition, Expr, Iden, Query, SelectStatement, SimpleExpr, Value, WithClause};

/// The CTEs a gated page reads and the gates of its steps so far, as its
/// branches are added in the order the page tries them.
pub(crate) struct GatedSteps<'p, 'a> {
    /// The statement the page is cut from.
    source: &'p PageSource<'a>,
    /// What the page's rows carry.
    columns: GatedColumns,
    /// The candidates and walk matches the branches read.
    pub(crate) ctes: WithClause,
    /// One gate per step so far, each holding when that step fell short of
    /// the page, so each later step runs only when every earlier one did.
    pub(crate) fell_short: Vec<SimpleExpr>,
    /// How many index reads the page tries before the next, which names that
    /// read's CTE.
    reads: usize,
}

impl<'p, 'a> GatedSteps<'p, 'a> {
    /// The steps of a page of `source` whose rows carry what `columns` asks
    /// for, before any is added.
    pub(crate) fn new(source: &'p PageSource<'a>, columns: GatedColumns) -> GatedSteps<'p, 'a> {
        GatedSteps {
            source,
            columns,
            ctes: WithClause::new(),
            fell_short: Vec::new(),
            reads: 0,
        }
    }

    /// The branch that cuts the page from the candidates of `read`, open when
    /// every earlier step fell short and the entries `cte` counts of them, up
    /// to one past `bound`, number at most `bound`. The entries' CTE is added,
    /// and past the bound a gate every later step shares. The candidates are
    /// read only when the branch opens.
    pub(crate) fn leading(&mut self, cte: IndexEntries, read: LeadingRead, bound: u64) -> Result<SelectStatement, RenderError> {
        let keys = narrowed_to(self.source.rows.entity_key_column(), read.candidates);
        let narrowed = self
            .fell_short
            .iter()
            .cloned()
            .fold(read.narrowed.add(keys), Condition::add)
            .add(within(cte.iden(), bound));
        self.fell_short.push(beyond(cte.iden(), bound));
        let (indexed, _) = self.source.page(narrowed, self.source.window)?;
        let mut indexed = indexed.select;
        named(&mut indexed, self.columns, Total::Counted, GatedStep::Candidates);
        self.ctes.cte(materialized(bounded(read.entries, bound), cte.iden()));
        Ok(wrapped(indexed, "indexed"))
    }

    /// The branch that cuts the page from `candidates`, read up to `bound`,
    /// under `narrowed`, open when every earlier step fell short and the read
    /// stays within the bound. The read's CTE is added, and past its bound a
    /// gate every later step shares.
    pub(crate) fn indexed(&mut self, candidates: SelectStatement, narrowed: Condition, bound: CandidateBound) -> Result<SelectStatement, RenderError> {
        let cte = Candidates::at(self.reads);
        self.reads += 1;
        let keys = narrowed_to(self.source.rows.entity_key_column(), column_of(cte.iden(), Entities::EntityKey));
        let mut narrowed = self.fell_short.iter().cloned().fold(narrowed.add(keys), Condition::add);
        match bound {
            CandidateBound::AtMost(most) => {
                narrowed = narrowed.add(within(cte.iden(), most));
                self.fell_short.push(beyond(cte.iden(), most));
            }
            CandidateBound::Whole => {}
        }
        let (indexed, _) = self.source.page(narrowed, self.source.window)?;
        let mut indexed = indexed.select;
        named(&mut indexed, self.columns, Total::Counted, GatedStep::Candidates);
        self.ctes.cte(materialized(bound.read(candidates), cte.iden()));
        Ok(wrapped(indexed, "indexed"))
    }

    /// The branch `alias` that cuts the page from the walk matches `matches`
    /// holds, ordered as `order` orders the page, open when every earlier
    /// step fell short and each of `gates` holds, its rows naming `step`.
    pub(crate) fn walked(
        &self,
        matches: impl Iden + 'static,
        gates: Vec<SimpleExpr>,
        order: &[(Alias, KeyOrder)],
        step: GatedStep,
        alias: &str,
    ) -> SelectStatement {
        let mut answered = Query::select();
        answered.column(Asterisk);
        named(&mut answered, self.columns, Total::Absent, step);
        answered.from(matches);
        for gate in self.fell_short.iter().cloned().chain(gates) {
            answered.and_where(gate);
        }
        for (column, key_order) in order {
            key_order.order_by(&mut answered, Expr::col(column.clone()));
        }
        cut(&mut answered, self.source.window);
        wrapped(answered.take(), alias)
    }
}

/// Whether a gated branch's rows carry the total.
#[derive(Clone, Copy)]
pub(crate) enum Total {
    /// The branch read every candidate, so it counts its rows before the
    /// window cuts them.
    Counted,
    /// The branch saw part of the selection: a null `bigint`, so the union's
    /// column takes the type of the index branches' count.
    Absent,
}

/// Project on `branch` what `columns` asks the rows of a gated page to carry:
/// the total as `total` says the branch can, and `step` as the step that
/// answered.
pub(crate) fn named(branch: &mut SelectStatement, columns: GatedColumns, total: Total, step: GatedStep) {
    match (columns.total, total) {
        (CountPolicy::Emit, Total::Counted) => {
            branch.expr_as(count_over_all(), Alias::new(TOTAL_COUNT_COLUMN));
        }
        (CountPolicy::Emit, Total::Absent) => {
            branch.expr_as(Expr::val(Value::BigInt(None)), Alias::new(TOTAL_COUNT_COLUMN));
        }
        (CountPolicy::Omit, Total::Counted | Total::Absent) => {}
    }
    match columns.step {
        StepColumn::Named => {
            branch.expr_as(step_value(step), Alias::new(GATED_STEP_COLUMN));
        }
        StepColumn::Unnamed => {}
    }
}

/// The `smallint` a branch names itself by.
fn step_value(step: GatedStep) -> SimpleExpr {
    Expr::val(Value::SmallInt(Some(step as i16)))
}
