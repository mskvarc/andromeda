//! The distributed tree, its `temporal` area: one test per scenario of brokers working together, a front broker and the
//! peers its registrations name, each a sequence of requests and the requirements on their answers, against the seven
//! brokers of the multi stack.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod add_attribute;
mod delete;
mod delete_attribute;
mod delete_instance;
mod modify_instance;
