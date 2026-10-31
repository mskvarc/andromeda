//! The pgbouncer tree, its `tenancy` area: one test per scenario of a broker serving every request through a
//! transaction-pooling `PgBouncer`, each a sequence of requests and the requirements on their answers, against the
//! broker of the pgbouncer stack.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod provisioning;
