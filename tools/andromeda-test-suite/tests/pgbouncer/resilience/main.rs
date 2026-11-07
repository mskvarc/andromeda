//! The pgbouncer tree, its `resilience` area: a broker's requests while its pooler is reconnected, paused, saturated
//! or killed one tenant database at a time, each a sequence of requests and the requirements on their answers, against
//! the brokers of the pgbouncer stack.

mod churn;
mod outage;
mod saturation;
