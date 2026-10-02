//! What one write did to one Entity, as the clause 5.2.12 notification
//! triggers read it.
//!
//! Clause 5.8.6 notifies a subscriber "whenever there is a change in the
//! watched Attributes", and the message "shall include all the subscribed
//! Entities that changed". A change is what one write did to one Entity -- a
//! batch operation treats every entity instance "as if [it] had all arrived in
//! separate requests" (clause 5.5.11.0) -- so it is recorded once, as the
//! Entity's lifecycle transition together with every Attribute instance the
//! write created, updated or deleted. The notification triggers of Table
//! 5.2.12-1 then decide *whether* a subscription is owed a notification for the
//! change, never how many: one change is one notification per subscription,
//! however many instances it touched and however many of the subscription's
//! triggers it raised.

use crate::notification::NotificationTrigger;
use andromeda_ngsi_ld_entity::reads::attribute_change::{AttributeChange, AttributeChangeKind};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// What the write did to the Entity as a whole.
///
/// Not a specification token: the kebab-case spelling is the one Andromeda
/// persists a deferred change under.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntityLifecycle {
    /// The Entity did not exist before the write.
    Created,
    /// The Entity existed before the write and still exists after it.
    Modified,
    /// The Entity existed before the write and does not exist after it.
    Deleted,
}

/// The clause 5.2.12 trigger one attribute-instance transition raises:
/// `attributeCreated`, `attributeUpdated` or `attributeDeleted` (Table
/// 5.2.12-1).
#[must_use]
pub const fn attribute_trigger(kind: AttributeChangeKind) -> NotificationTrigger {
    match kind {
        AttributeChangeKind::Created => NotificationTrigger::AttributeCreated,
        AttributeChangeKind::Updated => NotificationTrigger::AttributeUpdated,
        AttributeChangeKind::Deleted => NotificationTrigger::AttributeDeleted,
    }
}

/// One write's change to one Entity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntityChange {
    /// The Entity's lifecycle transition.
    pub lifecycle: EntityLifecycle,
    /// Every Attribute instance the write created, updated or deleted, in the
    /// order the classification produced them.
    pub instances: Vec<AttributeChange>,
}

impl EntityChange {
    /// A change carrying `instances` through `lifecycle`.
    #[must_use]
    pub const fn new(lifecycle: EntityLifecycle, instances: Vec<AttributeChange>) -> EntityChange {
        EntityChange { lifecycle, instances }
    }

    /// The entity-level trigger the lifecycle raises, if any.
    ///
    /// Table 5.2.12-1 makes `entityUpdated` "equivalent to the combination
    /// `attributeCreated`, `attributeUpdated` and `attributeDeleted`", which the
    /// subscription side expands, so a modification raises no entity-level
    /// trigger of its own: its instances raise the attribute-level ones.
    #[must_use]
    pub const fn lifecycle_trigger(&self) -> Option<NotificationTrigger> {
        match self.lifecycle {
            EntityLifecycle::Created => Some(NotificationTrigger::EntityCreated),
            EntityLifecycle::Deleted => Some(NotificationTrigger::EntityDeleted),
            EntityLifecycle::Modified => None,
        }
    }

    /// Every clause 5.2.12 trigger the change raises, each named once.
    #[must_use]
    pub fn triggers(&self) -> Vec<NotificationTrigger> {
        let mut triggers: Vec<NotificationTrigger> = self.lifecycle_trigger().into_iter().collect();
        for trigger in self.instances.iter().map(|instance| attribute_trigger(instance.kind)) {
            if !triggers.contains(&trigger) {
                triggers.push(trigger);
            }
        }
        triggers
    }

    /// The names of the Attributes the change touched, each named once, in
    /// the order the instances carry them.
    #[must_use]
    pub fn attributes(&self) -> Vec<&AttributeName> {
        let mut names: Vec<&AttributeName> = Vec::with_capacity(self.instances.len());
        for name in self.instances.iter().map(|instance| &instance.key.attr) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// Whether the change deleted any Attribute instance.
    #[must_use]
    pub fn deletes_instances(&self) -> bool {
        self.instances.iter().any(|instance| instance.kind == AttributeChangeKind::Deleted)
    }

    /// Whether a subscription listening to `triggers` and watching `watched`
    /// is owed a notification for this change.
    ///
    /// Table 5.2.12-1: "The notification triggers listed indicate what kind of
    /// changes shall trigger a notification", and clause 5.8.1.4: "Watched
    /// Attributes are those that trigger a new notification when they are
    /// changed" -- absent, "any Attribute". The change is owed when its
    /// lifecycle raises a listed entity-level trigger over an attribute the
    /// subscription watches, or when one of its instances raises a listed
    /// attribute-level trigger on a watched attribute. An entity created or
    /// deleted with no attribute at all still raises its entity-level trigger
    /// for a subscription that watches every attribute.
    #[must_use]
    pub fn is_owed_to(&self, triggers: &[NotificationTrigger], watched: Option<&HashSet<AttributeName>>) -> bool {
        let is_watched = |attribute: &AttributeName| watched.is_none_or(|names| names.contains(attribute));
        let lifecycle_owed = self
            .lifecycle_trigger()
            .is_some_and(|trigger| triggers.contains(&trigger) && (watched.is_none() || self.instances.iter().any(|instance| is_watched(&instance.key.attr))));
        lifecycle_owed
            || self
                .instances
                .iter()
                .any(|instance| triggers.contains(&attribute_trigger(instance.kind)) && is_watched(&instance.key.attr))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_change::{EntityChange, EntityLifecycle, attribute_trigger},
        notification::{DEFAULT_NOTIFICATION_TRIGGERS, NotificationTrigger, effective_notification_triggers},
    };
    use andromeda_ngsi_ld_entity::reads::{
        attribute_change::{AttributeChange, AttributeChangeKind},
        attribute_instance_key::AttributeInstanceKey,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    fn watching(raw: &[&str]) -> HashSet<AttributeName> {
        raw.iter().map(|attribute| name(attribute)).collect()
    }

    fn instance(attribute: &str, dataset: Option<&str>, kind: AttributeChangeKind) -> AttributeChange {
        AttributeChange {
            key: AttributeInstanceKey {
                attr: name(attribute),
                dataset_id: dataset.map(|raw| raw.parse().expect("a valid dataset URI")),
            },
            kind,
        }
    }

    /// A Create Entity with `a`, `b` and `c`.
    fn created_abc() -> EntityChange {
        EntityChange::new(
            EntityLifecycle::Created,
            vec![
                instance("https://example.org/a", None, AttributeChangeKind::Created),
                instance("https://example.org/b", None, AttributeChangeKind::Created),
                instance("https://example.org/c", None, AttributeChangeKind::Created),
            ],
        )
    }

    /// A Replace that creates `a`, updates `b` and deletes `c`.
    fn replaced() -> EntityChange {
        EntityChange::new(
            EntityLifecycle::Modified,
            vec![
                instance("https://example.org/a", None, AttributeChangeKind::Created),
                instance("https://example.org/b", None, AttributeChangeKind::Updated),
                instance("https://example.org/c", None, AttributeChangeKind::Deleted),
            ],
        )
    }

    /// Table 5.2.12-1: each attribute-instance transition raises the trigger
    /// of its own kind.
    #[test]
    fn each_attribute_transition_raises_the_trigger_of_its_kind() {
        assert_eq!(attribute_trigger(AttributeChangeKind::Created), NotificationTrigger::AttributeCreated);
        assert_eq!(attribute_trigger(AttributeChangeKind::Updated), NotificationTrigger::AttributeUpdated);
        assert_eq!(attribute_trigger(AttributeChangeKind::Deleted), NotificationTrigger::AttributeDeleted);
    }

    /// The deferred-change queues persist the transition kind; its spelling is
    /// the one the model type it replaced persisted (`created`, `updated`,
    /// `deleted`), and the lifecycle keeps its own.
    #[test]
    fn the_persisted_spellings_of_a_change_are_unchanged() {
        for (kind, spelling) in [
            (AttributeChangeKind::Created, "\"created\""),
            (AttributeChangeKind::Updated, "\"updated\""),
            (AttributeChangeKind::Deleted, "\"deleted\""),
        ] {
            assert_eq!(serde_json::to_string(&kind).expect("serialises"), spelling);
            assert_eq!(serde_json::from_str::<AttributeChangeKind>(spelling).expect("deserialises"), kind);
        }
        for (lifecycle, spelling) in [
            (EntityLifecycle::Created, "\"created\""),
            (EntityLifecycle::Modified, "\"modified\""),
            (EntityLifecycle::Deleted, "\"deleted\""),
        ] {
            assert_eq!(serde_json::to_string(&lifecycle).expect("serialises"), spelling);
        }
    }

    #[test]
    fn a_creation_raises_its_entity_trigger_and_one_attribute_trigger() {
        assert_eq!(
            created_abc().triggers(),
            vec![NotificationTrigger::EntityCreated, NotificationTrigger::AttributeCreated]
        );
    }

    #[test]
    fn a_modification_raises_only_the_attribute_triggers_of_its_instances() {
        assert_eq!(
            replaced().triggers(),
            vec![
                NotificationTrigger::AttributeCreated,
                NotificationTrigger::AttributeUpdated,
                NotificationTrigger::AttributeDeleted
            ]
        );
        assert!(replaced().deletes_instances());
        assert!(!created_abc().deletes_instances());
    }

    #[test]
    fn the_attributes_are_named_once_in_instance_order() {
        let change = EntityChange::new(
            EntityLifecycle::Modified,
            vec![
                instance("https://example.org/b", None, AttributeChangeKind::Updated),
                instance("https://example.org/b", Some("urn:ngsi-ld:Dataset:x"), AttributeChangeKind::Updated),
                instance("https://example.org/a", None, AttributeChangeKind::Created),
            ],
        );
        assert_eq!(change.attributes(), vec![&name("https://example.org/b"), &name("https://example.org/a")]);
    }

    /// Table 5.2.12-1's default triggers and no watched attributes: a creation
    /// with three attributes is owed, and owed once.
    #[test]
    fn a_default_subscription_is_owed_a_creation() {
        assert!(created_abc().is_owed_to(&DEFAULT_NOTIFICATION_TRIGGERS, None));
    }

    #[test]
    fn an_entity_created_subscription_is_owed_a_creation_of_a_watched_attribute_only() {
        let triggers = [NotificationTrigger::EntityCreated];
        assert!(created_abc().is_owed_to(&triggers, None));
        assert!(created_abc().is_owed_to(&triggers, Some(&watching(&["https://example.org/b"]))));
        assert!(!created_abc().is_owed_to(&triggers, Some(&watching(&["https://example.org/z"]))));
    }

    /// An entity created with no attribute at all still raises `entityCreated`
    /// for a subscription that watches every attribute, and nothing for one
    /// that watches some.
    #[test]
    fn an_attribute_less_creation_is_owed_only_where_every_attribute_is_watched() {
        let bare = EntityChange::new(EntityLifecycle::Created, Vec::new());
        assert!(bare.is_owed_to(&[NotificationTrigger::EntityCreated], None));
        assert!(!bare.is_owed_to(&[NotificationTrigger::EntityCreated], Some(&watching(&["https://example.org/a"]))));
        assert!(!bare.is_owed_to(&DEFAULT_NOTIFICATION_TRIGGERS, None));
    }

    /// An attribute-level trigger is owed only through an instance of that
    /// kind on a watched attribute.
    #[test]
    fn an_attribute_trigger_is_owed_through_a_watched_instance_of_its_kind() {
        let deleted_only = [NotificationTrigger::AttributeDeleted];
        assert!(replaced().is_owed_to(&deleted_only, None));
        assert!(replaced().is_owed_to(&deleted_only, Some(&watching(&["https://example.org/c"]))));
        assert!(!replaced().is_owed_to(&deleted_only, Some(&watching(&["https://example.org/a"]))));
        assert!(!created_abc().is_owed_to(&deleted_only, None));
    }

    /// Table 5.2.12-1: `entityUpdated` is the three attribute triggers, which
    /// the subscription side expands; a modification is owed through them.
    #[test]
    fn an_entity_updated_subscription_is_owed_a_modification_through_its_expansion() {
        let expanded = effective_notification_triggers(Some(&[NotificationTrigger::EntityUpdated]));
        assert!(replaced().is_owed_to(&expanded, None));
        assert!(!created_abc().is_owed_to(&[NotificationTrigger::EntityDeleted], None));
    }
}
