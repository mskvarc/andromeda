//! Bounded Entity Map expiry policy.

use crate::{error::EntityMapError, store::EntityMapStore};
use chrono::{DateTime, Utc};
use std::sync::Arc;

/// Independently scheduled deletion of Entity Maps past their `expiresAt`.
pub struct EntityMapExpiry {
    /// The Entity Map persistence port.
    store: Arc<dyn EntityMapStore>,
}

impl EntityMapExpiry {
    /// Construct expiry over the Entity Map persistence port.
    pub const fn new(store: Arc<dyn EntityMapStore>) -> EntityMapExpiry {
        EntityMapExpiry { store }
    }

    /// Delete Entity Maps due at the explicit execution cutoff, returning how many.
    ///
    /// # Errors
    ///
    /// [`EntityMapError::Persistence`] when the store cannot delete the maps
    /// that are due.
    pub async fn execute(&self, cutoff: DateTime<Utc>) -> Result<u64, EntityMapError> {
        self.store.delete_expired(cutoff).await
    }
}
