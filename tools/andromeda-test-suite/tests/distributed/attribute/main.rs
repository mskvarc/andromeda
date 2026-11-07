//! The distributed tree, its `attribute` area: one test per scenario of brokers working together, a front broker and
//! the peers its registrations name, each a sequence of requests and the requirements on their answers, against the
//! seven brokers of the multi stack.

mod append;
mod delete;
mod partial_update;
mod replace;
mod update;
