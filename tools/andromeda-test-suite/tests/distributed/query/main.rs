//! The distributed tree, its `query` area: one test per scenario of brokers working together, a front broker and the
//! peers its registrations name, each a sequence of requests and the requirements on their answers, against the seven
//! brokers of the multi stack.

mod format;
mod geo;
mod pagination;
mod projection;
mod q_language;
