//! The ngsi-ld tree, its `snapshot` area: one test per scenario of the broker's NGSI-LD behaviour, each a sequence of
//! requests and the requirements on their answers, in the shared default tenant unless the test names a private one.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod clone;
mod create;
mod delete;
mod notification;
mod purge;
mod query;
mod status;
mod update;
