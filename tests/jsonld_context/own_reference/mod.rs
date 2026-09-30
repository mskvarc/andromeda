//! References to the tenant's own stored `@context`s.
//!
//! Clause 5.13.1 serves Hosted and `ImplicitlyCreated` `@context`s "on-demand"
//! from the broker's storage. A reference whose path is
//! `[<advertised prefix>]/ngsi-ld/v1/jsonldContexts/<JsonLdContext id>`, whose
//! query is empty or one `andromedaTenant` naming the tenant, and whose
//! identifier the tenant holds, is resolved from that storage whatever origin it
//! names: the broker never fetches its own document over HTTP and never records
//! it as a Cached copy. Any other reference is an ordinary URL (clause 4.14:
//! another tenant's entry is not this tenant's).

mod recognition;
mod stored_wrapper;

/// An origin no name service resolves (IETF RFC 6761 section 6.4): a reference
/// on it can only be resolved without a fetch.
const UNREACHABLE_ORIGIN: &str = "http://own-reference.invalid";
