//! Payload adaptation for NGSI-LD version conformance (clause 4.3.6.8).
//!
//! Three callers ask for it. A client sending `Prefer: ngsi-ld=<version>` gets a
//! 203 Non-Authoritative Information and a payload whose members and datatypes
//! were removed or reformatted down to the version it named; a Subscription
//! carrying an `ngsildConformance` gets the same operation applied to the
//! Entities of every notification it is sent (clause 5.8.6); and a
//! registration's `ngsildConformance` has the Entity data of every write
//! forwarded to its source amended the same way (clause 4.3.6.6).
//!
//! [`fallback`] owns the rule tables: which version introduced each Entity
//! member, attribute family and instance member, and so what a target older
//! than that leaves out or rewrites. [`adaptation`] says what the caller is
//! told about whether anything changed. Which NGSI-LD Element the payload is
//! ([`adapted_payload`]) decides how two of the rows read, because an
//! attribute's array is the clause 4.5.7 sequence of instances in time rather
//! than the clause 4.5.5 set separated by `datasetId`. [`deletion`] says how a
//! forwarded update writes a deletion for its target, or that it cannot. Every
//! payload is written
//! from its typed form ([`crate::entity_body`], [`crate::temporal`],
//! [`crate::forwarded_entity`]), which asks the rule table member by member as
//! it is written; no rendered document is read back.

pub mod adaptation;
pub mod adapted_payload;
pub mod deletion;
pub mod fallback;
