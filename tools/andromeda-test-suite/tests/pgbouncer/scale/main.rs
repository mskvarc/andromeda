//! The pgbouncer tree, its `scale` area: how many tenants a broker behind a transaction pooler serves, keeps resident
//! and holds backends for, each test against brokers of the pgbouncer stack no other test uses.

#![deny(missing_docs, unreachable_pub, unused_qualifications)]
#![deny(clippy::missing_docs_in_private_items)]
#![forbid(unsafe_code)]

mod backends;
mod provisioning;
mod residency;
