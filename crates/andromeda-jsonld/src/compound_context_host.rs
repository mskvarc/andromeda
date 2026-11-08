//! Hosting a compound `@context` so one `Link` header can name it.
//!
//! JSON-LD 1.1 section 6.1 lets an `application/json` document name exactly one
//! context: "A response MUST NOT contain more than one HTTP Link Header using
//! the <http://www.w3.org/ns/json-ld#context> link relation", and the JSON-LD 1.1
//! API rejects a document that does with `multiple context link headers`. A
//! response whose `@context` is not one string therefore cannot name it in a
//! `Link` header as it stands. ETSI GS CIM 009 v1.9.1 Annex C.8 names the
//! remedy, "a wrapper @context has to be created and hosted [...] The final aim
//! is that only one @context is referenced from the JSON-LD Link header", and
//! clause 5.13.1 names the stored entry that results: an `ImplicitlyCreated`
//! `@context`, "created by the broker as a result of a user request".
//!
//! [`CompoundContextHost`] is that capability, declared here because the
//! rendering asks for it and implemented where the stored `@context`s and the
//! URLs they are served under are known. This crate stores nothing and mints no
//! URL itself.

use andromeda_json_literal::{JsonLiteral, admission::JsonLiteralError};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use iri_rs::{InvalidIri, IriBuf};
use std::{fmt::Debug, future::Future, pin::Pin};
use thiserror::Error;

/// What the URL of a hosted wrapper is used for, which decides whether an
/// entry stored recently may be taken as still stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapperUse {
    /// Named in the `Link` header of one response. An entry this process
    /// stored recently is taken as still stored, so a request repeated a
    /// thousand times writes once.
    Response,
    /// Kept by a resource that outlives the request: a subscription's
    /// `jsonldContext` (clause 5.8.1.4), dereferenced whenever it notifies. The
    /// entry is confirmed in storage before the resource keeps its URL: a
    /// clause 5.13.5 deletion through another replica, or any other removal
    /// this process did not see, may have taken it since this process stored
    /// it.
    StoredReference,
}

/// The future a [`CompoundContextHost`] answers with.
pub type CompoundContextHostFuture<'a> = Pin<Box<dyn Future<Output = Result<IriBuf, ContextHostingError>> + Send + 'a>>;

/// Where a compound `@context` is stored so it can be served under one URL.
pub trait CompoundContextHost: Debug + Send + Sync {
    /// Store the document `{"@context": context}` and answer the absolute URL
    /// it is served under, for the `usage` the URL is put to.
    ///
    /// Hosting the same context twice answers the same URL whenever the
    /// implementation can share the entry, so a request repeated a thousand
    /// times names one stored document rather than a thousand.
    fn host<'a>(&'a self, context: &'a JsonLiteral, usage: WrapperUse) -> CompoundContextHostFuture<'a>;
}

/// A test double: asked to host, it reports that no host is attached rather than naming a context it never stored.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCompoundContextHost;

impl CompoundContextHost for NoCompoundContextHost {
    fn host<'a>(&'a self, _context: &'a JsonLiteral, _usage: WrapperUse) -> CompoundContextHostFuture<'a> {
        Box::pin(async { Err(ContextHostingError::NoHost) })
    }
}

/// Why a compound `@context` could not be hosted.
#[derive(Debug, Error)]
pub enum ContextHostingError {
    /// Nothing that can store a `@context` is attached where one was needed.
    #[error("no store for hosted @contexts is attached")]
    NoHost,
    /// The store of hosted `@context`s refused or failed the write.
    #[error("the hosted @context could not be stored: {0}")]
    Persistence(#[source] PersistenceFailure),
    /// The request named no authority and no `apiRoot` is configured, so no
    /// absolute URL can be minted for a wrapper; refused before anything is
    /// stored. An HTTP/1.1 request that names no `Host` is malformed (IETF RFC
    /// 9112 section 3.2).
    #[error("the request names no authority to give a hosted @context a URL on")]
    NoRequestAuthority,
    /// The URL minted for a stored wrapper is not an IRI, which no HTTP URL
    /// fails to be: a broken invariant, never a client mistake.
    #[error("the URL minted for a hosted @context is not an IRI: {0}")]
    MintedUrlNotIri(#[source] InvalidIri<String>),
    /// The document `{"@context": context}` could not be written, which an
    /// admitted context never fails to be: a broken invariant, never a client
    /// mistake.
    #[error("the hosted @context document could not be written: {0}")]
    Unwritable(#[source] JsonLiteralError),
}

#[cfg(test)]
mod tests {
    use crate::{
        compound_context_host::{CompoundContextHost, ContextHostingError, NoCompoundContextHost, WrapperUse},
        test_documents::literal,
    };
    use serde_json::json;

    #[tokio::test]
    async fn a_host_that_stores_nothing_reports_that_no_host_is_attached() {
        let context = literal(&json!(["https://example.com/a.jsonld", "https://example.com/b.jsonld"]));

        assert!(matches!(
            NoCompoundContextHost.host(&context, WrapperUse::Response).await,
            Err(ContextHostingError::NoHost)
        ));
    }
}
