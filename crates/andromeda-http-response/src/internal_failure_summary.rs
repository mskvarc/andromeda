//! The client-safe operation summary disclosed in place of an internal failure's cause.

/// A compile-time, client-safe summary of an internal failure.
///
/// Wrapping `&'static str` is the security invariant: the disclosed text can only
/// be a literal a developer chose, never runtime data interpolated from the
/// failure's cause. An internal error's cause may carry datastore hostnames, SQL,
/// or filesystem paths that must never reach a client, so the type system forbids
/// building a summary out of one.
///
/// Clause 5.5.3 of ETSI GS CIM 009 v1.9.1 defines `ProblemDetails.detail`, by way
/// of RFC 7807 section 3.1, as "a human-readable explanation specific to this
/// occurrence"; a real operation summary honours that better than the bare token
/// this replaced, while the clause 5.5.2 category stays `InternalError`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, derive_more::Display)]
pub struct InternalFailureSummary(&'static str);

impl InternalFailureSummary {
    /// The broker failed to resolve one of its own internal identifiers.
    ///
    /// A phrase that recurs across crates gets a shared constant so the wording
    /// is written and vetted once. Subsystem-specific phrases are passed inline.
    pub const IDENTIFIER_RESOLUTION: InternalFailureSummary = InternalFailureSummary::new("the broker could not resolve an internal identifier");
    /// The negotiated response representation could not be rendered.
    pub const RESPONSE_RENDERING: InternalFailureSummary = InternalFailureSummary::new("failed to render the response representation");
    /// The response payload could not be encoded for transport.
    pub const RESPONSE_ENCODING: InternalFailureSummary = InternalFailureSummary::new("failed to encode the response payload");
    /// A request payload could not be decoded for a reason of the broker's own.
    pub const REQUEST_DECODING: InternalFailureSummary = InternalFailureSummary::new("failed to decode the request payload");
    /// A context source could not be reached, or its response could not be used.
    pub const CONTEXT_FEDERATION: InternalFailureSummary =
        InternalFailureSummary::new("a context source could not be reached or its response could not be used");

    /// Wrap a vetted, client-safe literal.
    #[must_use]
    pub const fn new(summary: &'static str) -> InternalFailureSummary {
        InternalFailureSummary(summary)
    }
}

#[cfg(test)]
mod tests {
    use crate::internal_failure_summary::InternalFailureSummary;

    #[test]
    fn summary_display_is_the_wrapped_literal() {
        pretty_assertions::assert_eq!(InternalFailureSummary::new("x").to_string(), "x");
        pretty_assertions::assert_eq!(
            InternalFailureSummary::IDENTIFIER_RESOLUTION.to_string(),
            "the broker could not resolve an internal identifier"
        );
        pretty_assertions::assert_eq!(
            InternalFailureSummary::RESPONSE_RENDERING.to_string(),
            "failed to render the response representation"
        );
        pretty_assertions::assert_eq!(InternalFailureSummary::RESPONSE_ENCODING.to_string(), "failed to encode the response payload");
        pretty_assertions::assert_eq!(
            InternalFailureSummary::CONTEXT_FEDERATION.to_string(),
            "a context source could not be reached or its response could not be used"
        );
    }
}
