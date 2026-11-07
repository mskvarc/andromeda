//! The distributed tree, its `notification` area: one test per scenario of brokers working together, a front broker and
//! the peers its registrations name, each a sequence of requests and the requirements on their answers, against the
//! seven brokers of the multi stack.

mod delivery;
mod payload;
mod status;
mod triggering;
