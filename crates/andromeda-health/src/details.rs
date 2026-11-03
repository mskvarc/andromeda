//! What a component reports about itself, one variant per component.

use crate::{background_task_details::BackgroundTaskDetails, mqtt_details::MqttDetails, postgresql_details::PostgreSqlDetails};
use serde::{Deserialize, Serialize};

/// The details of one component's health.
///
/// Untagged on the wire: the component is already named by the key the details
/// sit under, and each variant's members are distinct, so the shape alone tells
/// the variants apart.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ComponentDetails {
    /// What the background tasks report.
    BackgroundTasks(BackgroundTaskDetails),
    /// What the MQTT transport reports.
    Mqtt(MqttDetails),
    /// What the PostgreSQL pool reports.
    PostgreSql(PostgreSqlDetails),
}
