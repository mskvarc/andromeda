//! The entities and attributes a subscription reacts to.

use crate::{entity_selector::EntitySelector, notification::NotificationTrigger};
use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
use andromeda_ngsi_ld_query::name_stage::Expanded;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// The entities and attributes whose changes a subscription reacts to
/// (ETSI GS CIM 009 clause 5.2.12, `entities` / `watchedAttributes` /
/// `notificationTrigger`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
#[builder(field_defaults(default = Default::default()))]
pub struct SubscriptionSelection {
    /// Entity selectors defining which entities trigger notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entities: Option<Vec<EntitySelector<Expanded>>>,
    /// Attribute names to watch for changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub watched_attributes: Option<Vec<AttributeIri>>,
    /// Conditions that trigger a notification (clause 5.2.12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notification_trigger: Option<Vec<NotificationTrigger>>,
}
