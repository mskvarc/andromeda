//! The relations a temporal read takes a Temporal Evolution from.
//!
//! A branch records history in the identity rows of `entities` holding a
//! Temporal Evolution, `temporal_attribute_streams`, `attribute_history` and
//! `scope_history`, joined by surrogate keys. The live
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
//! A source read for the Evolution an enclosing row names
//! ([`HistoryOf::Evolution`]) takes it inside as well. Over recorded rows
//! alone it compares the surrogate key every recorded stream and Scope event
//! carries and reads no catalogue row, sparing each probe a lookup of the
//! identifier; where the branch reads its queue it compares the identifier,
//! which a queued entry carries and a surrogate key cannot name.
//!
//! A source of recorded rows alone has no union to protect, and there the
//! Entity named inside costs the other way: a derived table referring to an
//! enclosing row keeps the `EXISTS` around it a subplan, so every candidate
//! Entity probes the source on its own even when a semi-join over the
//! source's few matching rows would answer at once. Such a source is a plain
//! join PostgreSQL pulls up, so the Entity is compared beside it
//! ([`HistoryBranch::entity_history`]) and the planner chooses between the
//! semi-join and the per-Entity probe by its estimates. Compared by the
//! identifier, the source joins the identity rows to turn each stream's key
//! into it, and its matches merge with a page's walk in identifier order, so
//! the page stops at its last Evolution. A statement reading every match, such
//! as a count, compares the surrogate key every recorded stream carries
//! instead and joins no catalogue row ([`BesideComparison`]).

/// The types whose Temporal Evolutions hold streams of an attribute.
pub mod attribute_holders;
/// The rows of attribute history and the streams they extend.
pub mod attribute_rows;
/// The column a Temporal Property is held in and the bounds a stream read on
/// it carries.
pub mod axis;
/// The Temporal Evolutions and the types they are recorded under.
pub mod entity_rows;
/// The history columns an entry of a queued intent is recorded in.
pub mod entry_values;
/// The grain columns every source projects.
pub mod grain;
/// The holders of the attributes a temporal selection by attribute alone names.
pub mod holder_rows;
/// The queued entries that replace a recorded instance.
pub mod instance_replacement;
/// The queued entries the application would write, as source rows.
mod pending;
/// The rows of scope history.
pub mod scope_rows;
/// The attribute history of one stream, read in parts.
pub mod stream_parts;

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

    /// The surrogate key a source of this branch narrows to in place of the
    /// identifier: the key of the Evolution `of` names, over recorded rows
    /// alone; `None` where the branch reads its queue or `of` names no
    /// Evolution by key.
    pub(crate) const fn evolution_key<'o>(&self, of: &'o HistoryOf) -> Option<&'o SimpleExpr> {
        match (self.pending, of) {
            (PendingEntries::Absent, HistoryOf::Evolution(evolution)) => Some(&evolution.key),
            (PendingEntries::Absent, HistoryOf::EveryEntity | HistoryOf::Entity(_))
            | (PendingEntries::Unioned, HistoryOf::EveryEntity | HistoryOf::Entity(_) | HistoryOf::Evolution(_)) => None,
        }
    }

    /// How a source of this branch is read for the Temporal Evolution
    /// `evolution` names: taken inside every branch of a source that unions
    /// queued entries, by its identifier; compared beside a source of
    /// recorded rows alone, by what `compared` names.
    #[must_use]
    pub fn entity_history(&self, evolution: EvolutionColumns, compared: BesideComparison) -> EntityHistory {
        match (self.pending, compared) {
            (PendingEntries::Unioned, BesideComparison::Identifier | BesideComparison::Key) => EntityHistory::Inside(HistoryOf::Entity(evolution.id)),
            (PendingEntries::Absent, BesideComparison::Identifier) => EntityHistory::BesideById(evolution.id),
            (PendingEntries::Absent, BesideComparison::Key) => EntityHistory::BesideByKey(evolution.key),
        }
    }
}

/// What a source of recorded rows alone is compared beside by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BesideComparison {
    /// The Entity identifier, joined from the identity rows: the matches
    /// carry the order a page walks its Evolutions in.
    Identifier,
    /// The Evolution's surrogate key, which every recorded stream carries,
    /// so no catalogue row is joined: for a statement reading every match.
    Key,
}

/// The columns of an enclosing row naming a Temporal Evolution.
#[derive(Clone, Debug)]
pub struct EvolutionColumns {
    /// The Entity identifier.
    pub id: SimpleExpr,
    /// The surrogate key of the recorded Evolution.
    pub key: SimpleExpr,
}

/// How a source is read for one Temporal Evolution.
#[derive(Clone, Debug)]
pub enum EntityHistory {
    /// The source unions queued entries and takes the Evolution inside
    /// every branch.
    Inside(HistoryOf),
    /// The source holds every recorded row, each naming its Evolution by
    /// [`HistoryGrain::EntityId`], which is compared beside it to the
    /// expression.
    BesideById(SimpleExpr),
    /// The source holds every recorded row, each naming its Evolution by
    /// [`HistoryGrain::EntityKey`], which is compared beside it to
    /// the expression.
    BesideByKey(SimpleExpr),
}

impl EntityHistory {
    /// The comparison of the source under `alias` to the Evolution beside it,
    /// `None` when the source takes the Evolution inside.
    #[must_use]
    pub fn compared_beside(&self, alias: &DynIden) -> Option<SimpleExpr> {
        match self {
            EntityHistory::Inside(_) => None,
            // clone: the identifier stays with the read, which every source of it shares.
            EntityHistory::BesideById(id) => Some(Expr::col((alias.clone(), HistoryGrain::EntityId)).eq(id.clone())),
            // clone: the key stays with the read, which every source of it shares.
            EntityHistory::BesideByKey(key) => Some(Expr::col((alias.clone(), HistoryGrain::EntityKey)).eq(key.clone())),
        }
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
    /// The Temporal Evolution an enclosing row names by both its identifier
    /// and its surrogate key: read by the key over recorded rows alone, by
    /// the identifier where the branch reads its queue.
    Evolution(EvolutionColumns),
}

impl HistoryOf {
    /// Narrow one branch of a source to the Entity by its identifier, the
    /// branch reading the Entity identifier from `entity_id`.
    pub(crate) fn narrow(&self, branch: &mut SelectStatement, entity_id: SimpleExpr) {
        match self {
            HistoryOf::EveryEntity => {}
            HistoryOf::Entity(id) | HistoryOf::Evolution(EvolutionColumns { id, .. }) => {
                // clone: every branch of the source names the Entity.
                branch.and_where(entity_id.eq(id.clone()));
            }
        }
    }

    /// The Evolution a row of the source this reads names, read under
    /// `alias`: by what the source projects, the surrogate key where it reads
    /// [`HistoryOf::Evolution`] and the identifier otherwise.
    pub(crate) fn same_evolution_as(&self, alias: impl IntoIden) -> HistoryOf {
        let alias = alias.into_iden();
        let id = Expr::col((alias.clone(), HistoryGrain::EntityId));
        match self {
            HistoryOf::EveryEntity | HistoryOf::Entity(_) => HistoryOf::Entity(id),
            HistoryOf::Evolution(_) => HistoryOf::Evolution(EvolutionColumns {
                id,
                key: Expr::col((alias, HistoryGrain::EntityKey)),
            }),
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
    /// The grain column the stream row names its Evolution by: the
    /// identifier, or the surrogate key a stream read by key carries.
    evolution: HistoryGrain,
}

impl HistoryStream {
    /// The stream the row under `row` names, the row naming its Evolution by
    /// identifier.
    #[must_use]
    pub fn under(row: impl IntoIden) -> HistoryStream {
        HistoryStream {
            row: row.into_iden(),
            evolution: HistoryGrain::EntityId,
        }
    }

    /// The stream the row under `row` names, the row read from the stream
    /// source of `branch` for `of`, which names the Evolution by key where it
    /// reads it by key.
    #[must_use]
    pub fn read_for(branch: &HistoryBranch, of: &HistoryOf, row: impl IntoIden) -> HistoryStream {
        HistoryStream {
            row: row.into_iden(),
            evolution: match branch.evolution_key(of) {
                Some(_) => HistoryGrain::EntityKey,
                None => HistoryGrain::EntityId,
            },
        }
    }

    /// The grain column the stream row names its Evolution by.
    pub(crate) const fn evolution(&self) -> HistoryGrain {
        self.evolution
    }

    /// One grain column of the stream row.
    pub(crate) fn column(&self, column: HistoryGrain) -> SimpleExpr {
        Expr::col((self.row.clone(), column))
    }
}

#[cfg(test)]
mod tests {
    use crate::history_source::{BesideComparison, EntityHistory, EvolutionColumns, HistoryBranch, HistoryOf};
    use sea_query::{Alias, Expr, IntoIden, PostgresQueryBuilder, Query};

    fn evolution() -> EvolutionColumns {
        EvolutionColumns {
            id: Expr::col((Alias::new("e"), Alias::new("id"))),
            key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
        }
    }

    fn rendered(expr: sea_query::SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_branch_reading_its_queue_takes_the_evolution_inside_its_source_by_identifier() {
        for compared in [BesideComparison::Identifier, BesideComparison::Key] {
            let read = HistoryBranch::live().entity_history(evolution(), compared);
            assert!(
                matches!(&read, EntityHistory::Inside(HistoryOf::Entity(id)) if rendered(id.clone()) == r#"SELECT "e"."id""#),
                "{read:?}"
            );
            assert!(read.compared_beside(&Alias::new("ah").into_iden()).is_none(), "{read:?}");
        }
    }

    #[test]
    fn a_branch_without_a_queue_compares_the_evolution_beside_its_source_by_what_is_asked() {
        for branch in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(Alias::new("snap_1").into_iden())] {
            for (compared, comparison) in [
                (BesideComparison::Identifier, r#"SELECT "ah"."entity_id" = "e"."id""#),
                (BesideComparison::Key, r#"SELECT "ah"."entity_key" = "e"."entity_key""#),
            ] {
                let read = branch.entity_history(evolution(), compared);
                pretty_assertions::assert_eq!(read.compared_beside(&Alias::new("ah").into_iden()).map(rendered).as_deref(), Some(comparison));
            }
        }
    }
}
