//! Reading one local page of a snapshot entity capture (clause 5.16.1.4).
//!
//! A page is every stored state the query selects within one keyset window.
//! A state that no longer reads back ends the query: without it the capture
//! would not be the query's result.

use crate::{
    error::SnapshotError,
    executor::{SnapshotExecutor, execution::entity_query_failure, execution_failure::ExecutionFailure, page_outcome::PageOutcome, status::failure_detail},
};
use andromeda_entity::{
    port::{retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
    query::local_read::LocalRead,
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow};
use std::sync::Arc;
use tracing::warn;

impl SnapshotExecutor {
    /// One local page of the states `read` selects within `window`, or the
    /// result the query ends with when the page cannot be read whole.
    ///
    /// # Errors
    ///
    /// Returns a transient store failure the page met.
    pub(super) async fn local_page(
        &self,
        read: LocalRead<'_>,
        window: &PageWindow,
        snapshot_id: &Identifier,
        index: usize,
    ) -> Result<PageOutcome, SnapshotError> {
        let rows = match self
            .sources
            .entity_query
            .query_states(&self.sources.live_target, read, window, CountPolicy::Omit)
            .await
        {
            Ok((rows, _)) => rows,
            Err(error) => {
                warn!(snapshot_id = %snapshot_id, query_index = index, error = %error, "Snapshot entity query failed");
                return entity_query_failure(error).map(PageOutcome::Ended);
            }
        };
        Ok(match read_states(rows) {
            Ok(states) => PageOutcome::States(states),
            Err(id) => PageOutcome::Ended(failure_detail(ExecutionFailure::UnreadableEntity { id: &id }.problem())),
        })
    }
}

/// The states of one local page, or the first entity of it that does not read
/// back.
fn read_states(rows: Vec<StoredEntry<RetrievedEntity>>) -> Result<Vec<EntityState>, Identifier> {
    rows.into_iter()
        .map(|row| match row {
            // The page statement decoded each state into its own allocation,
            // so taking it out of its `Arc` never copies; the entity-level
            // system times read beside it are not part of a captured entity.
            StoredEntry::Read(row) => Ok(Arc::unwrap_or_clone(row.state)),
            StoredEntry::Unreadable(id) => Err(id),
        })
        .collect()
}
