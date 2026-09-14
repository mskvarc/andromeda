//! Effects of an exhausted task on its owning runtime scope.

use strum::IntoStaticStr;

/// Operational effect applied after restart policy is exhausted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum TaskCriticality {
    /// Keep the owning scope serviceable while reporting failed health.
    Observed,
    /// Mark the tenant degraded without cancelling sibling tasks.
    TenantDegrading,
    /// Cancel every task belonging to the tenant.
    TenantFatal,
    /// Cancel process and tenant task scopes.
    ProcessFatal,
}
