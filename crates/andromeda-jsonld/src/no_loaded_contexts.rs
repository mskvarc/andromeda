//! A [`ContextTailLookup`] that holds no document, for tests that present a
//! context without a tenant's loader.

use crate::{
    context_tail::ContextTail,
    error::JsonLdError,
    hosted_context_recognizer::{HostedContextRecognizer, NoHostedContexts},
    presented_context::ContextTailLookup,
};
use iri_rs::IriBuf;
use std::future::{Future, ready};

/// A lookup that holds no document.
///
/// Every document is reported as not ending with the core context, which
/// appends it.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLoadedContexts;

impl ContextTailLookup for NoLoadedContexts {
    fn tail_of(&self, _url: &IriBuf) -> impl Future<Output = Result<ContextTail, JsonLdError>> + Send {
        ready(Ok(ContextTail::Unresolvable))
    }

    fn hosted_contexts(&self) -> &dyn HostedContextRecognizer {
        &NoHostedContexts
    }
}
