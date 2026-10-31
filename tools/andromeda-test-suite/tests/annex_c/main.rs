//! The worked examples of NGSI-LD Annex C against the base stack, one test per example check.
//!
//! Each test serves the example's user `@context` from a server of its own and namespaces every identifier and
//! entity type, so every test runs in the shared default tenant beside every other.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod brand_fleet;
mod c_10;
mod c_11;
mod c_2_1;
mod c_2_2;
mod c_2_3;
mod c_2_4;
mod c_3;
mod c_4;
mod c_5_10;
mod c_5_11;
mod c_5_12;
mod c_5_13;
mod c_5_14;
mod c_5_15;
mod c_5_16;
mod c_5_2;
mod c_5_3;
mod c_5_4;
mod c_5_5;
mod c_5_6;
mod c_5_7;
mod c_5_8;
mod c_5_9;
mod c_6;
mod c_7;
mod c_8;
mod c_9;
mod discovery_fleet;
mod parking_example;
mod printed_document;
mod printed_vehicle;
mod served_context;
mod speed_evolution;
mod vehicle_example;
