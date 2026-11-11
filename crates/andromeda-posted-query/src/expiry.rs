//! Deletion of posted queries past their expiry.

use crate::{error::PostedQueryError, store::PostedQueryStore};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Independently scheduled deletion of posted queries past their expiry.
pub struct PostedQueryExpiry {
    /// The posted-query persistence port.
    store: Arc<dyn PostedQueryStore>,
}

impl PostedQueryExpiry {
    /// Construct expiry over the posted-query persistence port.
    pub const fn new(store: Arc<dyn PostedQueryStore>) -> PostedQueryExpiry {
        PostedQueryExpiry { store }
    }

    /// Delete the posted queries due at the explicit execution cutoff,
    /// returning how many.
    ///
    /// # Errors
    ///
    /// [`PostedQueryError::Persistence`] when the store cannot delete the
    /// queries that are due.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<u64, PostedQueryError> {
        self.store.delete_expired(cutoff).await
    }
}
