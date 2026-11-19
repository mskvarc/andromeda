//! The entity and attribute selection of a Subscription as a client reads it.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{entity_selector::EntitySelector, notification::NotificationTrigger};
use andromeda_ngsi_ld_query::name_stage::Written;
use serde::Serialize;

/// The `entities`, `watchedAttributes` and `notificationTrigger` members (clause 5.2.12), the names in the terms of the reader.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SelectionView<'a> {
    /// Entity selectors defining which entities trigger notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) entities: Option<&'a [EntitySelector<Written>]>,
    /// Attribute names to watch for changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) watched_attributes: Option<&'a [AttributeName]>,
    /// Conditions that trigger a notification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) notification_trigger: Option<&'a [NotificationTrigger]>,
}
