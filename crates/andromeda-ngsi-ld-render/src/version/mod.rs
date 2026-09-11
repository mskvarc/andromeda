//! Payload adaptation for NGSI-LD version conformance (clause 4.3.6.8).
//!
//! Two callers ask for it. A client sending `Prefer: ngsi-ld=<version>` gets a
//! 203 Non-Authoritative Information and a payload whose members and datatypes
//! were removed or reformatted down to the version it named; and a Subscription
//! carrying an `ngsildConformance` gets the same operation applied to the
//! Entities of every notification it is sent (clause 5.8.6).
//!
//! The operation splits along the three axes clause 4.3.6.8 leaves implicit.
//! The first is the fallback itself: which version introduced each attribute
//! family ([`family_introduction`]), what Tables 4.3.6.8-1 to -3 remove or
//! reformat, and what the caller is told about whether anything changed
//! ([`adaptation`]). The second is the representation the payload is already
//! rendered in, which decides where an attribute's family and value are read
//! from: [`reified_attribute`] for the normalized and concise renderings of
//! clauses 4.5.1 and 4.5.2.3, [`simplified_attribute`] for the lossy rendering
//! of clause 4.5.4. The third is which NGSI-LD Element the payload is
//! ([`adapted_payload`]), because two of the rows read differently once an
//! attribute's array is the clause 4.5.7 sequence of instances in time rather
//! than the clause 4.5.5 set separated by `datasetId`. [`renderer`] is what a
//! caller drives, and the only module that knows all three axes.

pub mod adaptation;
pub mod adapted_payload;
pub mod family_introduction;
pub mod reified_attribute;
pub mod renderer;
pub mod simplified_attribute;
pub mod simplified_temporal_attribute;
