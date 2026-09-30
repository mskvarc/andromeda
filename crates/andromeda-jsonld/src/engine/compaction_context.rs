//! A request `@context`, processed once into the active context compaction
//! reads.
//!
//! Processing a context is the expensive half of compaction: every term the
//! core context declares becomes a term definition, and the inverse context
//! compaction selects terms through is derived from them. None of that depends
//! on the document being compacted, so [`CompactionContext`] does it once and
//! every document of the operation is compacted against the result.

use crate::{
    context_graph::check_context_graph,
    context_spec::ContextSpec,
    error::JsonLdError,
    loader::{error::LoaderError, http::HttpContextLoader},
};
use iri_rs::IriBuf;
use jsonld::{
    Compact,
    CompactError,
    Context as ActiveContext,
    ExpandedDocument,
    Options,
    context_processing::{Process, ProcessedRef},
    syntax::{Context as JsonLdContext, TryFromJson},
};
use rdfx::{BlankIdBuf, vocabulary::no_vocabulary_mut};
use thiserror::Error;

/// A request `@context` and the active context it processed into.
#[derive(derive_more::Debug)]
pub(crate) struct CompactionContext {
    /// The `@context` as the request resolved it, which a compacted document
    /// that keeps its context carries verbatim.
    unprocessed: JsonLdContext,
    /// The active context compaction selects terms from.
    #[debug(skip)]
    processed: ActiveContext<IriBuf, BlankIdBuf>,
}

impl CompactionContext {
    /// Resolve and process the `@context` a spec selects.
    ///
    /// Mirrors what the processor does before compacting a document: the
    /// context is processed against an empty active context with no base IRI,
    /// which leaves a relative `@base` in the context itself as the only base
    /// compaction can relativize against.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::InlineContextInvalid`] when the resolved
    /// `@context` is not a JSON-LD context, and the processing failure, typed
    /// as the processor types it, when a context does not resolve or does not
    /// process.
    pub(crate) async fn prepare(spec: &ContextSpec, loader: &HttpContextLoader) -> Result<CompactionContext, CompactionContextError> {
        let unprocessed = match spec.to_context_value() {
            Some(value) => {
                check_context_graph(loader, &value).await.map_err(CompactionContextError::Invalid)?;
                JsonLdContext::try_from_json(&jsonld_syntax::Value::from_serde_json(value))
                    .map_err(|error| CompactionContextError::Invalid(JsonLdError::InlineContextInvalid(error)))?
            }
            None => JsonLdContext::default(),
        };
        let processed = unprocessed
            .process_full(
                no_vocabulary_mut(),
                &ActiveContext::new(None),
                loader,
                None,
                Options::<IriBuf>::default().context_processing_options(),
                (),
            )
            .await
            .map_err(|error| CompactionContextError::Processing(Box::new(CompactError::ContextProcessing(error))))?
            .into_processed();
        Ok(CompactionContext { unprocessed, processed })
    }

    /// Compact an expanded document against this context.
    ///
    /// The document is borrowed, so a caller whose compaction failed can try
    /// again against another context without having kept a copy of it.
    ///
    /// # Errors
    ///
    /// Returns the compaction failure, typed as the processor types it.
    pub(crate) async fn compact(
        &self,
        document: &ExpandedDocument<IriBuf, BlankIdBuf>,
        loader: &HttpContextLoader,
        embedding: ContextEmbedding,
    ) -> Result<jsonld_syntax::Value, Box<CompactError<LoaderError>>> {
        // The compaction algorithm writes whatever `@context` it is handed into
        // the output. A document that is about to lose it is handed the empty
        // one instead, which the algorithm leaves out, so the context is
        // neither copied into the output nor removed from it again.
        let empty = JsonLdContext::default();
        let embedded = match embedding {
            ContextEmbedding::Embed => &self.unprocessed,
            ContextEmbedding::Omit => &empty,
        };
        document
            .compact_full(
                no_vocabulary_mut(),
                ProcessedRef::new(embedded, &self.processed),
                loader,
                Options::<IriBuf>::default().compaction_options(),
            )
            .await
            .map_err(|error| Box::new(CompactError::Compaction(error)))
    }
}

/// Whether a compacted document carries the `@context` it was compacted with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextEmbedding {
    /// Write the `@context` into the compacted document.
    Embed,
    /// Leave it out.
    Omit,
}

/// Why a request `@context` could not be prepared for compaction.
#[derive(Debug, Error)]
pub(crate) enum CompactionContextError {
    /// The resolved `@context` is not a JSON-LD context at all, or its
    /// references form a cycle or nest too deep.
    #[error(transparent)]
    Invalid(JsonLdError),
    /// A context did not resolve, or did not process.
    #[error(transparent)]
    Processing(Box<CompactError<LoaderError>>),
}
