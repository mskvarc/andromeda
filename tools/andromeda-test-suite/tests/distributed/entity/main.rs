//! The distributed tree, its `entity` area: one test per scenario of brokers working together, a front broker and the
//! peers its registrations name, each a sequence of requests and the requirements on their answers, against the seven
//! brokers of the multi stack.

mod create;
mod delete;
mod merge;
mod purge;
mod replace;
mod retrieve;
