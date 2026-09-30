//! The `@context` an `application/ld+json` body names (study group A).
//!
//! Clause 5.2.3 has an `application/ld+json` Entity, Context Source
//! Registration, Subscription or Notification carry "an array (flattened to a
//! single string if necessary), containing a user @context where present, and
//! the core @context", and clause 4.4 has the core `@context` treated as last
//! "so that [...] upstream JSON-LD processors can properly expand as NGSI-LD".
//! The rule (`docs/core-context-rendering.md`): the request's contexts in order,
//! every core reference removed, and the v1.9 core `@context` appended last
//! unless what remains provably leaves it in force last (Annex C.8); one
//! context as a plain string, several as an array.

mod body_named;
mod determinism;
mod geojson;
mod link_named;
mod processor_check;
mod rendered_resources;
mod repeated_context;
