//! The ETSI distributed-operations cases: a broker forwarding queries, retrievals and writes to the context sources
//! its registrations name, in the inclusive, exclusive, redirect and auxiliary modes (clauses 4.3.3 and 4.3.6), and
//! detecting a forwarding loop with the `Via` header.
//!
//! Each case serves its own context source and stubs its answers.

mod consumption;
mod fork_rule;
mod forwarding_source;
mod loop_detection;
mod provision;
mod redirected_request;
mod vehicle_payloads;
