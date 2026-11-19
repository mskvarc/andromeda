//! How reading or selecting one page of a snapshot entity capture ended.

use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_model::snapshot::ExecutionResultDetails;

/// One page of a capture: the states it holds, or the result the query ends
/// with because the page could not be taken whole.
pub(super) enum PageOutcome {
    /// The page's states.
    States(Vec<EntityState>),
    /// The query's result: the page could not be read or captured whole.
    Ended(ExecutionResultDetails),
}
