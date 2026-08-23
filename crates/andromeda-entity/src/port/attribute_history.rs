//! Historical dataset identifiers needed by attribute deletion.

use crate::error::EntityError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use futures::future::BoxFuture;

/// Reads the dataset identifiers for which an entity attribute has history.
pub trait AttributeDatasetHistory: Send + Sync + 'static {
    /// Return historical dataset identifiers, using `None` for the default dataset.
    fn dataset_identifiers<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        entity: &'life2 Uri,
        attribute: &'life3 AttributeName,
    ) -> BoxFuture<'async_trait, Result<Vec<Option<Uri>>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;
}
