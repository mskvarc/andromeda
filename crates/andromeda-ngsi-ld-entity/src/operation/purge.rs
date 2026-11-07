//! Purge Entity attributes (clause 5.6.21), whole or scoped to a `datasetId`.

use crate::{
    entity_state::EntityState,
    operation::{
        attribute_write::{InstanceRemovalOutcome, remove_instance},
        selection::DatasetSelection,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_vocabulary::expanded::properties;

/// Which members of an entity a purge removes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributePurge<'a> {
    /// The listed members (clause 5.6.21 `drop`).
    Drop(&'a [AttributeName]),
    /// Every member except the listed ones (clause 5.6.21 `keep`).
    Keep(&'a [AttributeName]),
}

impl AttributePurge<'_> {
    /// Whether the purge removes the member `name`.
    fn removes(self, name: &str) -> bool {
        match self {
            AttributePurge::Drop(listed) => listed.iter().any(|listed| listed.as_str() == name),
            AttributePurge::Keep(listed) => !listed.iter().any(|listed| listed.as_str() == name),
        }
    }
}

/// Whether a purge removed anything from the entity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PurgeOutcome {
    /// At least one member or instance was removed.
    Purged,
    /// The entity held nothing the purge selects.
    Untouched,
}

impl EntityState {
    /// Remove the members `purge` selects, every instance of each (clause
    /// 5.6.21).
    ///
    /// The members are the attributes and the two entity members a client
    /// names by expanded IRI as it names an attribute: `scope` (clause 4.18)
    /// and the entity `expiresAt` (clause 4.22).
    pub fn purge_attributes(&mut self, purge: AttributePurge<'_>) -> PurgeOutcome {
        let mut outcome = PurgeOutcome::Untouched;
        if self.scope().is_some() && purge.removes(properties::SCOPE.as_str()) {
            self.set_scope(None);
            outcome = PurgeOutcome::Purged;
        }
        if self.expires_at().is_some() && purge.removes(properties::EXPIRES_AT.as_str()) {
            self.set_expires_at(None);
            outcome = PurgeOutcome::Purged;
        }
        let before = self.attributes().len();
        self.attributes_mut().retain(|name, _| !purge.removes(name.as_str()));
        if self.attributes().len() < before {
            outcome = PurgeOutcome::Purged;
        }
        outcome
    }

    /// Remove the instances `selection` names from each attribute `purge`
    /// selects (the dataset-scoped purge extension of clause 5.6.21).
    ///
    /// Only attributes are told apart by `datasetId` (clause 4.5.5); `scope`
    /// and the entity `expiresAt` hold no instances a `datasetId` selects.
    /// [`DatasetSelection::All`] removes every instance, which is
    /// [`EntityState::purge_attributes`] restricted to attributes.
    pub fn purge_dataset_instances(&mut self, purge: AttributePurge<'_>, selection: DatasetSelection<'_>) -> PurgeOutcome {
        let dataset_id: Option<&Identifier> = match selection {
            DatasetSelection::All => {
                let before = self.attributes().len();
                self.attributes_mut().retain(|name, _| !purge.removes(name.as_str()));
                return if self.attributes().len() < before {
                    PurgeOutcome::Purged
                } else {
                    PurgeOutcome::Untouched
                };
            }
            DatasetSelection::Single(dataset_id) => Some(dataset_id),
            DatasetSelection::Default => None,
        };
        let targets: Vec<AttributeName> = self
            .attributes()
            .iter()
            .filter(|(name, instances)| purge.removes(name.as_str()) && instances.contains(dataset_id))
            .map(|(name, _)| name.clone()) // clone: the removal outlives the borrow the scan holds.
            .collect();
        targets.iter().fold(PurgeOutcome::Untouched, |outcome, name| {
            match remove_instance(self.attributes_mut(), name, dataset_id) {
                InstanceRemovalOutcome::Removed => PurgeOutcome::Purged,
                InstanceRemovalOutcome::Absent => outcome,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_state::EntityState,
        operation::{
            purge::{AttributePurge, PurgeOutcome},
            selection::DatasetSelection,
            speed_entity::{SPEED, dataset, date_time, entity, instances, name, number, property, values, with_three_instances},
        },
        scopes::Scopes,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_vocabulary::expanded::properties;

    fn scoped_speed_entity() -> EntityState {
        let mut target = entity(vec![
            (SPEED, instances(vec![(None, property(number(1)))])),
            ("https://example.com/brand", instances(vec![(None, property(number(2)))])),
        ]);
        target.set_scope(Some(Scopes::one("/A".parse().expect("a valid scope path"))));
        target.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));
        target
    }

    /// Clause 5.6.21 `drop`: only the listed attributes go.
    #[test]
    fn dropping_attributes_removes_only_the_listed_ones() {
        let mut target = scoped_speed_entity();
        pretty_assertions::assert_eq!(target.purge_attributes(AttributePurge::Drop(&[name(SPEED)])), PurgeOutcome::Purged);
        assert!(!target.attributes().contains(SPEED));
        pretty_assertions::assert_eq!(values(&target, "https://example.com/brand"), vec![2]);
        assert!(target.scope().is_some());
    }

    /// Clause 5.6.21 `keep`: every member but the listed ones goes, the
    /// entity `scope` and `expiresAt` included unless listed.
    #[test]
    fn keeping_attributes_removes_every_other_member() {
        let mut target = scoped_speed_entity();
        let kept = [name(SPEED), name(properties::SCOPE.as_str())];
        pretty_assertions::assert_eq!(target.purge_attributes(AttributePurge::Keep(&kept)), PurgeOutcome::Purged);
        pretty_assertions::assert_eq!(target.attributes().names().map(AttributeName::as_str).collect::<Vec<_>>(), vec![SPEED]);
        assert!(target.scope().is_some());
        pretty_assertions::assert_eq!(target.expires_at(), None);
    }

    #[test]
    fn dropping_attributes_the_entity_lacks_leaves_it_untouched() {
        let mut target = scoped_speed_entity();
        pretty_assertions::assert_eq!(
            target.purge_attributes(AttributePurge::Drop(&[name("https://example.com/other")])),
            PurgeOutcome::Untouched
        );
    }

    /// The dataset-scoped purge removes only the selected instance of each
    /// selected attribute.
    #[test]
    fn a_dataset_purge_removes_only_the_selected_instances() {
        let mut target = with_three_instances();
        let outcome = target.purge_dataset_instances(
            AttributePurge::Drop(&[name(SPEED)]),
            DatasetSelection::Single(&dataset("urn:ngsi-ld:Dataset:B")),
        );
        pretty_assertions::assert_eq!(outcome, PurgeOutcome::Purged);
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10, 30]);
    }

    /// Clause 4.18 gives `scope` no instances, so a `datasetId` selector never
    /// reaches it, not even the default one under `keep`.
    #[test]
    fn a_default_dataset_purge_never_reaches_the_scope() {
        let mut target = scoped_speed_entity();
        let outcome = target.purge_dataset_instances(AttributePurge::Keep(&[name("https://example.com/brand")]), DatasetSelection::Default);
        pretty_assertions::assert_eq!(outcome, PurgeOutcome::Purged);
        assert!(!target.attributes().contains(SPEED));
        assert!(target.scope().is_some());
        assert!(target.expires_at().is_some());
    }

    #[test]
    fn a_dataset_purge_matching_no_instance_leaves_the_entity_untouched() {
        let mut target = with_three_instances();
        let outcome = target.purge_dataset_instances(
            AttributePurge::Drop(&[name(SPEED)]),
            DatasetSelection::Single(&dataset("urn:ngsi-ld:Dataset:Z")),
        );
        pretty_assertions::assert_eq!(outcome, PurgeOutcome::Untouched);
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10, 20, 30]);
    }
}
