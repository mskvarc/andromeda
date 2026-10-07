//! Which identifier a stored `@context` is given (clauses 5.13.1, 5.13.2.4
//! and Table 6.30.2-1).
//!
//! Clause 5.13.1 leaves the format of a stored `@context`'s local identifier to
//! the broker. Andromeda names every entry `urn:ngsi-ld:JsonLdContext:<uuid>`:
//! a Hosted or Cached entry takes a random (version 4) UUID, and an
//! `ImplicitlyCreated` entry the version 5 UUID of its RFC 8785 canonical
//! content, so equal documents share one entry however they were written.

mod content_derived;
mod minting;
mod urn_spelling;

use crate::context_api::JSON_LD_CONTEXT_PREFIX;
use uuid::{Uuid, Version};

/// The UUID version a stored `@context` identifier carries as its local id.
fn uuid_version(identifier: &str) -> Option<Version> {
    let local_id = identifier
        .strip_prefix(JSON_LD_CONTEXT_PREFIX)
        .expect("a stored @context identifier carries the JsonLdContext prefix");
    Uuid::parse_str(local_id).expect("the local id is a hyphenated UUID").get_version()
}
