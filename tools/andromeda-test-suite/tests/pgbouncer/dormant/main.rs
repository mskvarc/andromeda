//! The pgbouncer tree, its `dormant` area: the obligations of a tenant out of the runtime cache, evicted by the
//! pressure of other tenants, the answers it gives once loaded again, and the obligations of a suspended tenant, each a
//! sequence of requests and the requirements on their answers, against the brokers of the pgbouncer stack.

mod expiry;
mod notification;
mod query;
mod suspension;
mod wake;
