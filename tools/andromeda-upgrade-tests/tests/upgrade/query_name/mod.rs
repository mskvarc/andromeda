//! The names holding a colon a v1.0.0 tenant stored unquoted in query paths,
//! in its Subscriptions and its Snapshots, which the upgrade quotes so this
//! build reads them back as the filters v1.0.0 read.

mod colon_name_rewrite;
mod rewrite_cases;
mod rewrite_schema;
mod snapshot_names;
mod subscription_names;
