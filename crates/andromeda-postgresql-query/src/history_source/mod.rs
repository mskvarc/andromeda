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

use sea_query::{DynIden, ExprTrait, SelectStatement, SimpleExpr};

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
