//! The distributed tree, its `registration` area: one test per scenario of brokers working together, a front broker and
//! the peers its registrations name, each a sequence of requests and the requirements on their answers, against the
//! seven brokers of the multi stack.

mod create;
mod delete;
mod health;
mod matching;
mod query;
mod retrieve;
mod update;
