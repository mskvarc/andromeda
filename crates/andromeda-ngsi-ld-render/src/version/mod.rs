//! Payload adaptation for NGSI-LD version conformance (clause 4.3.6.8).
//!
//! Two callers ask for it. A client sending `Prefer: ngsi-ld=<version>` gets a
//! 203 Non-Authoritative Information and a payload whose members and datatypes
//! were removed or reformatted down to the version it named; and a Subscription
//! carrying an `ngsildConformance` gets the same operation applied to the
//! Entities of every notification it is sent (clause 5.8.6).
//!
//! [`fallback`] owns the rule tables: which version introduced each Entity
//! member, attribute family and instance member, and so what a target older
//! than that leaves out or rewrites. [`adaptation`] says what the caller is
//! told about whether anything changed. Entity data written from its typed form
//! asks the rule table member by member as it is written
//! ([`crate::entity_body`]); a payload that is still a rendered document -- a
//! temporal answer, a forwarded body -- is read by the readings below, which
//! ask the same table.
//!
//! Those readings split along two axes clause 4.3.6.8 leaves implicit. The
//! representation the payload is already rendered in decides where an
//! attribute's family and value are read from: [`reified_attribute`] for the
//! normalized and concise renderings of clauses 4.5.1 and 4.5.2.3,
//! [`simplified_attribute`] for the lossy rendering of clause 4.5.4,
//! [`simplified_temporal_attribute`] for the clause 4.5.9 one. Which NGSI-LD
//! Element the payload is ([`adapted_payload`]) decides how two of the rows
//! read, because an attribute's array is the clause 4.5.7 sequence of instances
//! in time rather than the clause 4.5.5 set separated by `datasetId`.
//! [`renderer`] is what a caller of the readings drives.

pub mod adaptation;
pub mod adapted_payload;
pub mod fallback;
pub mod reified_attribute;
pub mod renderer;
pub mod simplified_attribute;
pub mod simplified_temporal_attribute;
