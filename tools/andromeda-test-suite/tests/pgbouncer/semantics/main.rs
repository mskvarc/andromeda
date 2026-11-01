//! The pgbouncer tree, its `semantics` area: the correctness of a broker's transaction-pooled request path, each a
//! sequence of requests and the requirements on their answers, against the brokers of the pgbouncer stack.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod batch;
mod maintenance;
mod notification;
mod routing;
mod snapshot;
mod statement;
mod write;
