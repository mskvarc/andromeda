//! NGSI-LD Subscription types (ETSI GS CIM 009 clause 5.2.8).

use crate::{
    response_format::{JoinMode, ResponseFormat},
    time_format,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// Notification trigger type (ETSI GS CIM 009 clause 5.2.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationTrigger {
    EntityCreated,
    EntityUpdated,
    EntityDeleted,
    AttributeCreated,
    AttributeUpdated,
    AttributeDeleted,
}

impl fmt::Display for NotificationTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EntityCreated => f.write_str("entityCreated"),
            Self::EntityUpdated => f.write_str("entityUpdated"),
            Self::EntityDeleted => f.write_str("entityDeleted"),
            Self::AttributeCreated => f.write_str("attributeCreated"),
            Self::AttributeUpdated => f.write_str("attributeUpdated"),
            Self::AttributeDeleted => f.write_str("attributeDeleted"),
        }
    }
}

impl std::str::FromStr for NotificationTrigger {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "entityCreated" => Ok(Self::EntityCreated),
            "entityUpdated" => Ok(Self::EntityUpdated),
            "entityDeleted" => Ok(Self::EntityDeleted),
            "attributeCreated" => Ok(Self::AttributeCreated),
            "attributeUpdated" => Ok(Self::AttributeUpdated),
            "attributeDeleted" => Ok(Self::AttributeDeleted),
            _ => Err(format!("invalid notification trigger: {}", s)),
        }
    }
}

/// Subscription lifecycle status (ETSI GS CIM 009 clause 5.2.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubscriptionStatus {
    Active,
    Paused,
    Expired,
}

impl fmt::Display for SubscriptionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Active => f.write_str("active"),
            Self::Paused => f.write_str("paused"),
            Self::Expired => f.write_str("expired"),
        }
    }
}

impl std::str::FromStr for SubscriptionStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "expired" => Ok(Self::Expired),
            _ => Err(format!("invalid subscription status: {}", s)),
        }
    }
}

/// Notification delivery status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotificationStatus {
    Ok,
    Failed,
}

impl fmt::Display for NotificationStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ok => f.write_str("ok"),
            Self::Failed => f.write_str("failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: String,

    #[serde(rename = "type")]
    pub type_: String,

    #[serde(skip_serializing_if = "Option::is_none", rename = "subscriptionName")]
    pub subscription_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntitySelector>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "watchedAttributes")]
    pub watched_attributes: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "notificationTrigger")]
    pub notification_trigger: Option<Vec<NotificationTrigger>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "timeInterval")]
    pub time_interval: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "geoQ")]
    pub geo_q: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "scopeQ")]
    pub scope_q: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub csf: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "datasetId")]
    pub dataset_id: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "isActive")]
    pub is_active: Option<bool>,

    pub notification: NotificationParams,

    #[serde(skip_serializing_if = "Option::is_none", rename = "expiresAt", with = "time_format::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttling: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "temporalQ")]
    pub temporal_q: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "jsonldContext")]
    pub jsonld_context: Option<String>,

    /// When true, subscription only matches locally stored entities (§5.5.13).
    #[serde(skip_serializing_if = "Option::is_none", rename = "localOnly")]
    pub local_only: Option<bool>,

    pub status: SubscriptionStatus,

    #[serde(rename = "createdAt", with = "time_format::rfc3339")]
    pub created_at: DateTime<Utc>,

    #[serde(rename = "modifiedAt", with = "time_format::rfc3339")]
    pub modified_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionFragment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "type")]
    pub type_: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "subscriptionName")]
    pub subscription_name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntitySelector>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "watchedAttributes")]
    pub watched_attributes: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "notificationTrigger")]
    pub notification_trigger: Option<Vec<NotificationTrigger>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "timeInterval")]
    pub time_interval: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "geoQ")]
    pub geo_q: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "scopeQ")]
    pub scope_q: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub csf: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "datasetId")]
    pub dataset_id: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "isActive")]
    pub is_active: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification: Option<NotificationParams>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "expiresAt", with = "time_format::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttling: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "jsonldContext")]
    pub jsonld_context: Option<String>,
}

/// Shared entity selector used by both Subscription and ContextSourceRegistration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySelector {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "idPattern")]
    pub id_pattern: Option<String>,

    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<ResponseFormat>,

    pub endpoint: Endpoint,

    #[serde(skip_serializing_if = "Option::is_none", rename = "sysAttrs")]
    pub sys_attrs: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "showChanges")]
    pub show_changes: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pick: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub omit: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub join: Option<JoinMode>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "joinLevel")]
    pub join_level: Option<u32>,

    #[serde(
        skip_serializing_if = "Option::is_none",
        rename = "lastNotification",
        with = "time_format::rfc3339_option",
        default
    )]
    pub last_notification: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "lastSuccess", with = "time_format::rfc3339_option", default)]
    pub last_success: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "lastFailure", with = "time_format::rfc3339_option", default)]
    pub last_failure: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "timesSent")]
    pub times_sent: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<NotificationStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub uri: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub accept: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "receiverInfo")]
    pub receiver_info: Option<Vec<Value>>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "notifierInfo")]
    pub notifier_info: Option<Vec<Value>>,
}
