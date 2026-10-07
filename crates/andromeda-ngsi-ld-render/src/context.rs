//! Where a response's `@context` goes, and what it says.
//!
//! Clause 6.3.6 gives the JSON-LD `@context` of a response two carriers: an
//! `application/ld+json` body holds it as a member, and an `application/json`
//! body cannot, so it is advertised through a `Link` header instead. Which
//! carrier applies is decided by the media type and, for `GeoJSON`, by the
//! `Prefer: body=` token; a partial-success body carries no context at all.
//!
//! Neither decision is made here: the caller settles which contexts apply and which carrier is used, and hands the answer down as a [`ResponseContext`], so the renderer writes exactly what it is given.

use andromeda_json_literal::JsonLiteral;
use iri_rs::IriBuf;

/// A response's `@context`, in the one carrier the response uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseContext {
    /// Embedded as the `@context` member of the body: one context as a
    /// string, several as an array (clause 5.2.3, "flattened to a single
    /// string if necessary").
    Body(JsonLiteral),
    /// Advertised by the one JSON-LD context `Link` header the response
    /// carries (clause 6.3.6; JSON-LD 1.1 section 6.1 admits no second one).
    /// The target is absolute: a relative context link would resolve against
    /// whatever URL a later reader of the response happens to hold.
    Link(IriBuf),
    /// Carried nowhere: the body of an error or partial-success response uses
    /// "Only Fully Qualified Names [...] as there is no context present"
    /// (clause 6.3.6).
    Absent,
}
