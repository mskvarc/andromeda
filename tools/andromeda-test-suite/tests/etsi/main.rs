//! The ETSI NGSI-LD conformance suite (the Robot Framework suite for ETSI GS CIM 009) against the base stack, one
//! test per ETSI case, each named after its case identifier.
//!
//! Each test stores a vocabulary of its own (see `andromeda_test_suite::vocabulary`) where the case references the
//! suite's `@context`, and namespaces every identifier, so every test runs in the shared default tenant beside every
//! other.

mod common_behaviours;
mod context_information;
mod context_source;
mod distributed_operations;
mod jsonld_context;
mod suite_keywords;
mod suite_payloads;
