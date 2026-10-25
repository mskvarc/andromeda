//! An in-memory Context Registry store for tests that need no database.

use crate::{
    context_page::ContextPage,
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    fetched_context::FetchedContext,
    insertion::ContextInsertion,
    list_query::ContextListQuery,
    listed_contexts::ListedContexts,
    mutation::ContextMutation,
    service::ContextRegistryService,
    store::{ContextRegistryFuture, ContextRegistryStore},
    stored_context::StoredContext,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_cache::DownloadedContext, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use andromeda_ngsi_ld_query::page_reach::PageReach;
use std::sync::{Arc, RwLock};
use url::Url;

/// One stored row: its identifier, kind and document.
pub(super) struct Row {
    /// The row's local identifier, as written.
    pub(super) identifier: String,
    /// The row's lifecycle kind.
    pub(super) kind: ContextKind,
    /// The row's stored document.
    pub(super) payload: JsonLiteral,
}

/// A store holding its rows in memory, keyed by identifier alone.
#[derive(Default)]
pub(super) struct MemoryStore {
    rows: RwLock<Vec<Row>>,
}

impl MemoryStore {
    /// Hold `payload` under `identifier` as an entry of `kind`, as a client or
    /// an earlier broker run could have left it.
    pub(super) fn hold(&self, identifier: &JsonLdContextId, kind: ContextKind, payload: JsonLiteral) {
        self.rows.write().expect("the store lock").push(Row {
            identifier: identifier.to_string(),
            kind,
            payload,
        });
    }

    /// How many rows the store holds.
    pub(super) fn len(&self) -> usize {
        self.rows.read().expect("the store lock").len()
    }

    /// The row held under `identifier`, projected by `read`.
    pub(super) fn read<T>(&self, identifier: &str, read: impl FnOnce(&Row) -> T) -> Option<T> {
        self.rows
            .read()
            .expect("the store lock")
            .iter()
            .find(|row| row.identifier == identifier)
            .map(read)
    }
}

impl ContextRegistryStore for MemoryStore {
    fn insert<'a>(
        &'a self,
        identifier: &'a JsonLdContextId,
        payload: &'a JsonLiteral,
        _serving_url: &'a Url,
        kind: ContextCreationKind,
    ) -> ContextRegistryFuture<'a, ContextInsertion> {
        Box::pin(async move {
            if self.read(identifier.as_ref(), |_| ()).is_some() {
                return Ok(ContextInsertion::Occupied);
            }
            let kind = match kind {
                ContextCreationKind::Hosted => ContextKind::Hosted,
                ContextCreationKind::ImplicitlyCreated => ContextKind::ImplicitlyCreated,
            };
            self.hold(identifier, kind, payload.clone()); // clone: the store keeps its own copy of the document.
            Ok(ContextInsertion::Inserted)
        })
    }

    fn list<'a>(&'a self, _query: ContextListQuery<'a>) -> ContextRegistryFuture<'a, ContextPage> {
        Box::pin(async move {
            Ok(ContextPage {
                contexts: ListedContexts::Summary(Vec::new()),
                boundary: None,
                reach: PageReach::Exhausted,
            })
        })
    }

    fn get<'a>(&'a self, identifier: &'a Identifier, _detail_level: ContextDetailLevel) -> ContextRegistryFuture<'a, Option<FetchedContext>> {
        Box::pin(async move { Ok(self.read(identifier.as_str(), |row| FetchedContext::Payload(row.payload.clone()))) }) // clone: the read hands back an owned document.
    }

    fn record_access<'a>(&'a self, _identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move { Ok(ContextMutation::Applied) })
    }

    fn lookup<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, Option<StoredContext>> {
        Box::pin(async move {
            Ok(self.read(identifier.as_str(), |row| StoredContext {
                identifier: identifier.clone(), // clone: the lookup hands back an owned identifier.
                kind: row.kind.clone(),         // clone: the lookup hands back an owned kind.
                source_url: None,
            }))
        })
    }

    fn delete<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let mut rows = self.rows.write().expect("the store lock");
            let before = rows.len();
            rows.retain(|row| row.identifier != identifier.as_str());
            Ok(if rows.len() < before {
                ContextMutation::Applied
            } else {
                ContextMutation::Missing
            })
        })
    }

    fn replace_download<'a>(&'a self, _identifier: &'a Identifier, _download: &'a DownloadedContext) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move { Ok(ContextMutation::Missing) })
    }
}

/// A service over `store`, with an engine that never reaches the network.
pub(super) fn service(store: &Arc<MemoryStore>) -> ContextRegistryService {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds");
    ContextRegistryService::new(Arc::clone(store) as Arc<dyn ContextRegistryStore>, engine)
}
