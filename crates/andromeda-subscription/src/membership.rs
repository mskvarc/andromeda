//! The reverse map from a subscription to the index buckets it occupies.
//!
//! Removal walks only these buckets instead of every bucket in the index, so
//! evicting one subscription costs its own fan-out rather than the whole
//! index size.

use crate::selector_routing::SelectorRouting;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;

/// Every bucket one indexed subscription was placed in.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Membership {
    /// Selector placements by type and by id, and whether the wildcard bucket
    /// holds the subscription.
    pub selectors: SelectorRouting,
    /// Trigger buckets the subscription occupies.
    pub triggers: Vec<NotificationTrigger>,
    /// Watched-attribute buckets the subscription occupies; empty when it
    /// watches every attribute or none.
    pub watched_attributes: Vec<AttributeName>,
    /// Whether the subscription is in the watch-all bucket, meaning it carries
    /// no `watchedAttributes` restriction.
    pub watch_all: bool,
}

impl Membership {
    /// The type buckets the subscription occupies, each once.
    pub fn type_buckets(&self) -> impl Iterator<Item = &EntityTypeName> {
        self.selectors.by_type.iter().map(|(name, _)| name)
    }

    /// The id buckets the subscription occupies, each once.
    pub fn id_buckets(&self) -> impl Iterator<Item = &Uri> {
        self.selectors.by_entity_id.iter().map(|(id, _)| id)
    }
}
