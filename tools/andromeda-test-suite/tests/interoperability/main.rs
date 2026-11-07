//! The ETSI interoperability suites (`IOP_TP`: Create Entity, Retrieve Entity and Query Entities across registrations
//! of every mode) against the interoperability stack of five brokers, each case in a default-context and a
//! user-context variant.
//!
//! The brokers are named b1 to b5 as the ETSI documentation names them. Registrations are global to a broker, so each
//! case isolates itself by an entity type and entity identifiers of its own namespace, and every registration it
//! creates names that type.

#[macro_use]
mod scenario;
mod composition;
mod consumption;
mod node;
mod parking;
mod payload;
mod provision;
mod topology;
