//! Inspection of a captured notification's `Notification` structure (clause
//! 5.2.25): the entities carried in `data` and the members every notification
//! must expose, shared across the MAP notification cases.

use crate::notification_adapter::CapturedNotification;
use serde_json::Value;

/// The entities carried in the notification `data` array.
pub fn notified_entities(notification: &CapturedNotification) -> Vec<Value> {
    notification.body["data"].as_array().cloned().unwrap_or_default()
}

/// The ids of the entities carried in the notification `data` array.
pub fn notified_entity_ids(notification: &CapturedNotification) -> Vec<String> {
    notified_entities(notification)
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect()
}

/// Whether the notification's `data` carries an entity with the given id.
pub fn notifies_entity(notification: &CapturedNotification, entity_id: &str) -> bool {
    notified_entity_ids(notification).iter().any(|id| id == entity_id)
}
