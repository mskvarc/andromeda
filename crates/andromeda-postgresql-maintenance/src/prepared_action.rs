//! Resolving what one pending action works on, and the readings taken
//! around it.
//!
//! A fixed target names its relation at compile time; a columnstore target
//! works on whichever of its chunks is the oldest partial one, read from the
//! catalogue in the visit that acts on it, never from configuration or a
//! request.

use crate::{
    action_measurement::{ActionBaseline, ActionComparison},
    branch::{Branch, BranchTarget},
    candidate::MaintenanceTarget,
    columnstore_partiality,
    error::MaintenanceError,
    inspection,
    policy::SafetyGates,
    safety_gate_inspection::{self, GateScope},
    statistics_freshness,
};
use andromeda_postgresql_seaquery::maintenance::{MaintenanceStatement, recompress_chunk};
use sqlx::PgConnection;

/// What one pending action runs with once its subject is resolved: the gates
/// measured on its relations, the reading taken before it, its statement, and
/// the relation it works on, for the log.
pub(crate) struct PreparedAction {
    /// The safety gates of the action's relations.
    pub(crate) gates: SafetyGates,
    /// The measure read before the action.
    pub(crate) baseline: ActionBaseline,
    /// The fixed statement the action runs.
    pub(crate) statement: MaintenanceStatement,
    /// The relation the statement works on.
    pub(crate) relation: String,
}

/// Resolve what a pending action works on, measure its gates and read its
/// baseline. `None` for a columnstore target none of whose chunks is partial
/// any more: nothing is left to recompress.
pub(crate) async fn prepare_action(connection: &mut PgConnection, row: BranchTarget) -> Result<Option<PreparedAction>, MaintenanceError> {
    let branch = row.branch();
    let prepared = match row.target() {
        MaintenanceTarget::Index(index) => PreparedAction {
            gates: safety_gate_inspection::safety_gates(connection, GateScope::Target(row)).await?,
            baseline: ActionBaseline::Index {
                index,
                before_bytes: inspection::index_bytes(connection, branch, index).await?,
            },
            statement: index.reindex_statement(branch),
            relation: branch.qualified(index.relation_name()),
        },
        MaintenanceTarget::Heap(heap) => PreparedAction {
            gates: safety_gate_inspection::safety_gates(connection, GateScope::Target(row)).await?,
            baseline: ActionBaseline::Heap {
                heap,
                before: inspection::heap_tuple_counts(connection, branch, heap).await?,
            },
            statement: heap.vacuum_statement(branch),
            relation: branch.qualified(heap.relation_name()),
        },
        MaintenanceTarget::Statistics(heap) => PreparedAction {
            gates: safety_gate_inspection::safety_gates(connection, GateScope::Target(row)).await?,
            baseline: ActionBaseline::Statistics {
                heap,
                before_modified: statistics_freshness::modified_since_analyze(connection, branch, heap).await?,
            },
            statement: heap.analyze_statement(branch),
            relation: branch.qualified(heap.relation_name()),
        },
        MaintenanceTarget::Columnstore(table) => {
            let Some((chunk, _)) = columnstore_partiality::oldest_partial_chunk(connection, table).await? else {
                return Ok(None);
            };
            PreparedAction {
                gates: safety_gate_inspection::safety_gates(connection, GateScope::Chunk { table, chunk: &chunk }).await?,
                baseline: ActionBaseline::Columnstore { chunk: chunk.oid },
                statement: recompress_chunk(chunk.oid),
                relation: chunk.relation,
            }
        }
    };
    Ok(Some(prepared))
}

/// Read the same measure in `branch` after the action and pair it with the
/// baseline.
pub(crate) async fn read_comparison(connection: &mut PgConnection, branch: Branch, baseline: ActionBaseline) -> Result<ActionComparison, MaintenanceError> {
    Ok(match baseline {
        ActionBaseline::Index { index, before_bytes } => ActionComparison::Index {
            before_bytes,
            after_bytes: inspection::index_bytes(connection, branch, index).await?,
        },
        ActionBaseline::Heap { heap, before } => ActionComparison::Heap {
            before,
            after: inspection::heap_tuple_counts(connection, branch, heap).await?,
        },
        ActionBaseline::Statistics { heap, before_modified } => ActionComparison::Statistics {
            before_modified,
            after_modified: statistics_freshness::modified_since_analyze(connection, branch, heap).await?,
        },
        ActionBaseline::Columnstore { chunk } => ActionComparison::Columnstore {
            after: columnstore_partiality::chunk_partiality(connection, chunk).await?,
        },
    })
}
