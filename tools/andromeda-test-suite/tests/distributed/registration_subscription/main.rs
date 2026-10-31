//! The distributed tree, its `registration_subscription` area: one test per scenario of brokers working together, a
//! front broker and the peers its registrations name, each a sequence of requests and the requirements on their
//! answers, against the seven brokers of the multi stack.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod create;
mod delete;
mod notification;
mod query;
mod retrieve;
mod update;
