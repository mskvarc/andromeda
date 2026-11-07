//! The pgbouncer tree, its `scale` area: how many tenants a broker behind a transaction pooler serves, keeps resident
//! and holds backends for, each test against brokers of the pgbouncer stack no other test uses.

mod backends;
mod provisioning;
mod residency;
