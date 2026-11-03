//! What the background tasks report: the ones that stopped for good.

use andromeda_ngsi_ld_lexicon::{constant_string, tenant::TenantName};
use serde::{Deserialize, Serialize};

constant_string!(UnnamedDefaultTenant, "@default");

/// The name a background task is registered under.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct BackgroundTaskName(String);

impl BackgroundTaskName {
    /// The registered name of a task.
    #[must_use]
    pub const fn new(name: String) -> BackgroundTaskName {
        BackgroundTaskName(name)
    }
}

/// The tenant a stopped task served.
///
/// A named tenant is reported by its name and the unnamed default tenant as
/// `@default`, the value its metric series carry. No tenant name can spell it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum TaskTenant {
    /// The unnamed default tenant.
    UnnamedDefault(UnnamedDefaultTenant),
    /// A named tenant.
    Named(TenantName),
}

/// What a stopped task's exhaustion does to its scope, for the criticalities
/// that leave the tenant serving.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StoppedTaskCriticality {
    /// The failure is reported and nothing else changes.
    Observed,
    /// The failure marks the tenant, or the broker, degraded.
    TenantDegrading,
}

/// How the failure that stopped a task was classified.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum StoppedTaskFailureClass {
    /// A retry could have succeeded, but the restart policy ran out.
    Transient,
    /// A retry needs an external state change first.
    Permanent,
    /// The failure contradicts an internal correctness invariant.
    InvariantViolation,
}

/// One task that stopped for good.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoppedTask {
    /// The task's registered name.
    pub name: BackgroundTaskName,
    /// The tenant the task served; absent for a task of the whole broker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<TaskTenant>,
    /// What the task's exhaustion does to its scope.
    pub criticality: StoppedTaskCriticality,
    /// How the failure that stopped the task was classified.
    pub failure_class: StoppedTaskFailureClass,
    /// The failure that stopped the task, as the task reported it.
    pub failure_summary: String,
}

/// What the background tasks report: every task that stopped for good with a
/// criticality that leaves its scope serving.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundTaskDetails {
    /// The stopped tasks; empty while every task runs.
    pub stopped_tasks: Vec<StoppedTask>,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::background_task_details::{
        BackgroundTaskDetails,
        BackgroundTaskName,
        StoppedTask,
        StoppedTaskCriticality,
        StoppedTaskFailureClass,
        TaskTenant,
        UnnamedDefaultTenant,
    };
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use serde_json::json;

    fn stopped(tenant: Option<TaskTenant>) -> StoppedTask {
        StoppedTask {
            name: BackgroundTaskName::new("notification-delivery".to_owned()),
            tenant,
            criticality: StoppedTaskCriticality::TenantDegrading,
            failure_class: StoppedTaskFailureClass::InvariantViolation,
            failure_summary: "task stopped".to_owned(),
        }
    }

    #[test]
    fn a_stopped_task_of_the_unnamed_default_tenant_names_it_at_default() {
        pretty_assertions::assert_eq!(
            serde_json::to_value(stopped(Some(TaskTenant::UnnamedDefault(UnnamedDefaultTenant)))).expect("serialises"),
            json!({
                "name": "notification-delivery",
                "tenant": "@default",
                "criticality": "tenant-degrading",
                "failureClass": "invariant-violation",
                "failureSummary": "task stopped"
            })
        );
    }

    #[test]
    fn a_stopped_task_of_a_named_tenant_names_the_tenant() {
        let tenant = TenantName::try_from("acme".to_owned()).expect("a valid tenant name");
        let rendered = serde_json::to_value(stopped(Some(TaskTenant::Named(tenant)))).expect("serialises");
        pretty_assertions::assert_eq!(rendered["tenant"], json!("acme"));
    }

    #[test]
    fn a_stopped_task_of_the_whole_broker_omits_the_tenant_member() {
        let rendered = serde_json::to_value(stopped(None)).expect("serialises");
        assert!(rendered.get("tenant").is_none());
    }

    #[test]
    fn each_tenant_spelling_reads_back_as_the_variant_it_came_from() {
        let default: TaskTenant = serde_json::from_value(json!("@default")).expect("parses");
        pretty_assertions::assert_eq!(default, TaskTenant::UnnamedDefault(UnnamedDefaultTenant));
        let named: TaskTenant = serde_json::from_value(json!("acme")).expect("parses");
        assert!(matches!(named, TaskTenant::Named(_)));
    }

    #[test]
    fn the_details_round_trip_through_serde() {
        let details = BackgroundTaskDetails {
            stopped_tasks: vec![stopped(None)],
        };
        let json = serde_json::to_string(&details).expect("serialises");
        pretty_assertions::assert_eq!(serde_json::from_str::<BackgroundTaskDetails>(&json).expect("parses"), details);
    }
}
