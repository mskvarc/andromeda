//! Helpers for driving and observing the transaction-pooling test tier.
//!
//! Each submodule owns one concern: [`backend_sampler`] samples `PostgreSQL`
//! client-backend counts over time, [`pooler_console`] drives the `PgBouncer`
//! admin console, [`metrics_scrape`] reads a broker's Prometheus exposition,
//! [`tenant_load`] provisions tenants and generates bounded load with latency
//! percentiles, and [`tenant_cleanup`] evicts runtimes and drops tenant
//! databases between runs.

pub mod backend_sampler;
pub mod endpoints;
pub mod metrics_scrape;
pub mod pooler_console;
pub mod tenant_cleanup;
pub mod tenant_load;
