//! The andromeda tree, its `management` area: one test per scenario of what this broker does beyond the NGSI-LD
//! specification or chooses where the specification leaves it open, each a sequence of requests and the requirements on
//! their answers, against the brokers of the andromeda stack.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod tenant;
mod version;
