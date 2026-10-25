//! The `@context` a paged walk is read under (clause 5.5.9.1).
//!
//! Clause 5.5.9.1 requires the same `@context` throughout a pagination and
//! answers an attempt to use a different one with `BadRequestData`. Every page
//! pointer the broker emits, by offset or by cursor, therefore carries the
//! context of the request that produced it, and a request following one is
//! refused when its own context is another.

use iri_rs::IriBuf;
use thiserror::Error;

/// The query-string parameter a page pointer carries the walk's context in.
pub const PAGE_CONTEXT_PARAMETER: &str = "pageContext";

/// The context IRI a request named in its `Link` header, or the core context
/// when it named none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageContext(IriBuf);

impl PageContext {
    /// The context a request naming `context` pages under.
    #[must_use]
    pub const fn new(context: IriBuf) -> PageContext {
        PageContext(context)
    }

    /// The IRI a page pointer spells the context with.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// A `pageContext` value that is not an IRI.
///
/// The value is broker-emitted and followed unchanged, so one that does not
/// parse was corrupted or forged: a `BadRequestData` (clause 6.3.20).
#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid pageContext: not an IRI")]
pub struct PageContextError;

impl TryFrom<&str> for PageContext {
    type Error = PageContextError;

    fn try_from(text: &str) -> Result<PageContext, PageContextError> {
        IriBuf::new(text.to_owned()).map(PageContext).map_err(|_| PageContextError)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::pagination::page_context::PageContext;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_pointer_value_reads_back_as_the_context_it_spells() {
        let context = IriBuf::new("https://example.com/a.jsonld".to_owned()).expect("an IRI");
        assert_eq!(PageContext::try_from("https://example.com/a.jsonld"), Ok(PageContext::new(context)));
    }

    #[test]
    fn a_pointer_value_that_is_not_an_iri_is_refused() {
        assert!(PageContext::try_from("not an iri").is_err());
    }
}
