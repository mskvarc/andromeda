//! The distributed tree, its `distributed` area: one test per scenario of brokers working together, a front broker and
//! the peers its registrations name, each a sequence of requests and the requirements on their answers, against the
//! seven brokers of the multi stack.

mod batch;
mod discovery;
mod error;
mod forwarding_mode;
mod query;
mod retrieve;
mod temporal;
mod write;
