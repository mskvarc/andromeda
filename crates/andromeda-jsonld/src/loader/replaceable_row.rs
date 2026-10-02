//! A durable context cache holding one broker-stored row whose document a
//! test can replace, as another broker process sharing the database could.

use crate::{
    context_cache::{CachedContext, ContextCache, ContextCacheError, StoredDocumentSource},
    loader::cache_lifetime::{CacheLifetime, ResidualLifetime},
};
use iri_rs::IriBuf;
use serde_json::Value;
use std::sync::RwLock;

/// One stored row, answered for every URL it is asked about.
#[derive(Debug)]
pub(crate) struct ReplaceableRow {
    /// The document the row holds now.
    payload: RwLock<Value>,
}

impl ReplaceableRow {
    /// A row holding `payload`.
    pub(crate) const fn new(payload: Value) -> ReplaceableRow {
        ReplaceableRow { payload: RwLock::new(payload) }
    }

    /// Replace the document the row holds.
    pub(crate) fn replace(&self, payload: Value) {
        *self.payload.write().expect("the row lock") = payload;
    }
}

#[async_trait::async_trait]
impl ContextCache for ReplaceableRow {
    async fn get(&self, _url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError> {
        Ok(Some(CachedContext {
            payload: self.payload.read().expect("the row lock").clone(), // clone: the row keeps its document for the next read
            residual: ResidualLifetime::Unbounded,
            canonical_url: None,
            source: StoredDocumentSource::Broker,
        }))
    }

    async fn put(&self, _url: &IriBuf, _document_url: &IriBuf, _payload: &Value, _lifetime: CacheLifetime) -> Result<(), ContextCacheError> {
        Ok(())
    }
}
