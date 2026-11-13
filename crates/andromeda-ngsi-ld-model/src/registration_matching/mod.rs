//! Matching a specification of entities and attributes against a registration's
//! `information` (ETSI GS CIM 009 clause 5.12).
//!
//! Discovery (clause 5.10.2), context source registration subscriptions
//! (clause 5.11.2), distributed operations (clause 4.3.6) and the proxied
//! admission rules (clause 5.9.2.4) all ask whether a registration's
//! `RegistrationInfo` elements cover some entities and attributes. Clause 5.12
//! gives the one reading every one of them uses, and clauses 5.10.2.5 and
//! 5.11.7 return the registration trimmed to the matching elements. This module
//! is that reading: a concrete entity is the specification "any of its types,
//! exactly its identifier", so coverage of an entity and matching of a query
//! are one decision. A subscription copy placed at a context source is reduced
//! by the same reading (clause 5.8.1.4).

pub mod entity_specification;
pub mod match_specification;
pub mod selector_reduction;
