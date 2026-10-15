//! The relations a temporal read takes a Temporal Evolution from.
//!
//! A branch records history in `temporal_entities`, `temporal_attribute_streams`,
//! `attribute_history` and `scope_history`, joined by surrogate keys. The live
//! branch also queues history it has accepted and not applied yet
//! (`temporal_append_intents`), and a read answers the same before and after
//! the drain applies it only when it reads both. A queued entry has no
//! surrogate key, so every source here is keyed by what both halves carry: the
//! Entity identifier, the attribute key and the `datasetId` ([`HistoryGrain`]).
//!
//! On the live branch each source is the recorded rows `UNION ALL` the queued
//! entries the application would write, judged in SQL by the rules the
//! application follows (`crate::history_source::pending`); on a Snapshot
//! branch, which has no queue, it is the recorded rows alone.
//!
//! A source read for one Entity takes that Entity inside every branch
//! ([`HistoryOf::Entity`]). PostgreSQL pushes a restriction on a derived
//! table into its branches, but not a join clause: a correlation written
//! beside the source would become one once the planner turned the enclosing
//! `EXISTS` into a join, and the union would be read whole.
//!
//! A source of recorded rows alone has no union to protect, and there the
//! Entity named inside costs the other way: a derived table referring to an
//! enclosing row keeps the `EXISTS` around it a subplan, so every candidate
//! Entity probes the source on its own even when a semi-join over the
//! source's few matching rows would answer at once. Such a source is a plain
//! join PostgreSQL pulls up, so the Entity is compared beside it
//! ([`HistoryBranch::entity_history`]) and the planner chooses between the
//! semi-join and the per-Entity probe by its estimates.

/// The rows of attribute history and the streams they extend.
pub mod attribute_rows;
/// The Temporal Evolutions and the types they are recorded under.
pub mod entity_rows;
/// The history columns an entry of a queued intent is recorded in.
pub mod entry_values;
/// The grain columns every source projects.
pub mod grain;
/// The queued entries the application would write, as source rows.
mod pending;
/// The rows of scope history.
pub mod scope_rows;

use crate::history_source::grain::HistoryGrain;
use sea_query::{DynIden, Expr, ExprTrait, IntoIden, SelectStatement, SimpleExpr};

/// Whether a branch's history sources read queued entries beside the
/// recorded rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PendingEntries {
    /// The live branch while its queue holds entries the drain has not applied.
    Unioned,
    /// A Snapshot branch, which records history in the write's own
    /// transaction and has no queue, or the live branch while its queue holds
    /// nothing.
    Absent,
}

/// The relations of one branch a temporal statement reads history from.
///
/// The live branch reads its queue only while the queue holds an intent: a
/// write acknowledged before the read began is either applied or visible to
/// the probe that decides it, so reading the recorded rows alone over an empty
/// queue loses nothing a client could see, and the union costs as much again
/// as the statement it extends.
#[derive(Clone, Debug)]
pub struct HistoryBranch {
    /// The schema the branch's relations are named in, `None` for the live
    /// branch's own unqualified names.
    schema: Option<DynIden>,
    /// Whether the queued entries are read beside the recorded rows.
    pending: PendingEntries,
}

impl HistoryBranch {
    /// The live branch, its queued entries read beside its recorded rows.
    #[must_use]
    pub const fn live() -> HistoryBranch {
        HistoryBranch {
            schema: None,
            pending: PendingEntries::Unioned,
        }
    }

    /// The live branch while its queue holds no intent: its recorded rows
    /// alone.
    #[must_use]
    pub const fn live_recorded() -> HistoryBranch {
        HistoryBranch {
            schema: None,
            pending: PendingEntries::Absent,
        }
    }

    /// The live branch with its queued entries, its relations named in
    /// `schema`: for a statement whose unqualified names resolve to relations
    /// other than the live branch's own.
    #[must_use]
    pub const fn live_in(schema: DynIden) -> HistoryBranch {
        HistoryBranch {
            schema: Some(schema),
            pending: PendingEntries::Unioned,
        }
    }

    /// The recorded relations `schema` names, read without a queue: a
    /// Snapshot branch's, which has none, or copies of a branch's relations.
    #[must_use]
    pub const fn recorded_in(schema: DynIden) -> HistoryBranch {
        HistoryBranch {
            schema: Some(schema),
            pending: PendingEntries::Absent,
        }
    }

    /// The schema the branch's relations are named in, `None` for unqualified
    /// names.
    #[must_use]
    pub const fn schema(&self) -> Option<&DynIden> {
        self.schema.as_ref()
    }

    /// Whether the queued entries are read beside the recorded rows.
    #[must_use]
    pub const fn pending(&self) -> PendingEntries {
        self.pending
    }

    /// How a source of this branch is read for the Entity `entity` names:
    /// taken inside every branch of a source that unions queued entries,
    /// compared beside a source of recorded rows alone.
    #[must_use]
    pub const fn entity_history(&self, entity: SimpleExpr) -> EntityHistory {
        match self.pending {
            PendingEntries::Unioned => EntityHistory {
                of: HistoryOf::Entity(entity),
                beside: None,
            },
            PendingEntries::Absent => EntityHistory {
                of: HistoryOf::EveryEntity,
                beside: Some(entity),
            },
        }
    }
}

/// A source read for one Entity: whose history the source reads, and the
/// Entity its rows are compared to beside it.
#[derive(Clone, Debug)]
pub struct EntityHistory {
    /// Whose history the source reads.
    pub of: HistoryOf,
    /// The Entity the source's [`grain::HistoryGrain::EntityId`] is compared
    /// to beside the source, `None` when the source takes it inside.
    pub beside: Option<SimpleExpr>,
}

/// Whose history a source reads.
#[derive(Clone, Debug)]
pub enum HistoryOf {
    /// Every Temporal Evolution of the branch.
    EveryEntity,
    /// The Temporal Evolution of the Entity this expression names: a
    /// constant, or a column of an enclosing or lateral row.
    Entity(SimpleExpr),
}

impl HistoryOf {
    /// Narrow one branch of a source to the Entity, the branch reading the
    /// Entity identifier from `entity_id`.
    pub(crate) fn narrow(&self, branch: &mut SelectStatement, entity_id: SimpleExpr) {
        match self {
            HistoryOf::EveryEntity => {}
            HistoryOf::Entity(id) => {
                // clone: every branch of the source names the Entity.
                branch.and_where(entity_id.eq(id.clone()));
            }
        }
    }
}

/// One attribute stream a source reads alone: a row of
/// [`attribute_rows::stream_rows`] under the alias it is read through, whose
/// grain and recorded stream key the source narrows to.
#[derive(Clone, Debug)]
pub struct HistoryStream {
    /// The alias the stream row is read under, usually a lateral reference.
    row: DynIden,
}

impl HistoryStream {
    /// The stream the row under `row` names.
    #[must_use]
    pub fn under(row: impl IntoIden) -> HistoryStream {
        HistoryStream { row: row.into_iden() }
    }

    /// One grain column of the stream row.
    pub(crate) fn column(&self, column: HistoryGrain) -> SimpleExpr {
        // clone: every column reference names the row's alias.
        Expr::col((self.row.clone(), column))
    }
}

#[cfg(test)]
mod tests {
    use crate::history_source::{HistoryBranch, HistoryOf};
    use sea_query::{Alias, Expr, IntoIden, PostgresQueryBuilder, Query};

    fn entity() -> sea_query::SimpleExpr {
        Expr::col((Alias::new("e"), Alias::new("id")))
    }

    fn rendered(expr: sea_query::SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_branch_reading_its_queue_takes_the_entity_inside_its_source() {
        let read = HistoryBranch::live().entity_history(entity());
        assert!(
            matches!(&read.of, HistoryOf::Entity(id) if rendered(id.clone()) == r#"SELECT "e"."id""#),
            "{read:?}"
        );
        assert!(read.beside.is_none(), "{read:?}");
    }

    #[test]
    fn a_branch_without_a_queue_compares_the_entity_beside_its_source() {
        for branch in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(Alias::new("snap_1").into_iden())] {
            let read = branch.entity_history(entity());
            assert!(matches!(read.of, HistoryOf::EveryEntity), "{read:?}");
            pretty_assertions::assert_eq!(read.beside.map(rendered).as_deref(), Some(r#"SELECT "e"."id""#));
        }
    }
}
