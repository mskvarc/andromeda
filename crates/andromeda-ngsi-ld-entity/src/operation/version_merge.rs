//! Merging the versions of one Entity that several holders answered with
//! (clauses 4.3.6.2, 4.5.5.3, 4.18).
//!
//! Clause 4.5.5.3: instances of one attribute are told apart by `datasetId`
//! (the default instance being the one without), so instances with distinct
//! `datasetId`s coexist in the merged entity, while two instances sharing one
//! are in conflict and the most recent `observedAt` wins, then the most recent
//! `modifiedAt`. Where neither timestamp decides, the held instance stays:
//! clause 5.7.1.4 makes local data the base the remote data is merged onto.
//! Expired instances are discarded before the merge, by whoever read the
//! version.
//!
//! The merge works on expanded names, so two holders spelling one attribute
//! under different contexts merge into one attribute.

use crate::{attribute_instances::AttributeInstances, entity_state::EntityState, history::instance_recency::InstanceRecency, instance::Instance};
use andromeda_ngsi_ld_lexicon::time::{date_time::NgsiLdDateTime, system_instant::SystemInstant};
use std::{cmp::Ordering, sync::Arc};

/// How one version folds into the entity being assembled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergePolicy {
    /// Clause 4.5.5: instances merge per `datasetId`, the most recent winning.
    Instances,
    /// Clause 4.3.6.2 for an auxiliary registration: an attribute is added
    /// only when no other holder supplied it, and never overrides one.
    SupplementMissing,
}

impl EntityState {
    /// Fold `received`, another version of this entity, into it under
    /// `policy`.
    ///
    /// `id` and the types are this version's. Under the instance policy the
    /// entity-level `expiresAt` follows clause 4.5.5.3: it is kept only when
    /// both versions carry one, and then as the furthest in the future. Under
    /// the supplementing policy it is left alone, because clause 4.3.6.2 puts
    /// an auxiliary version outside that conflict. Clause 4.5.5.2 has already
    /// moved the transience of each version onto that version's own
    /// Attributes ([`differentiate_transient_versions`](crate::operation::transient_versions::differentiate_transient_versions)).
    pub fn merge_version(&mut self, received: EntityState, policy: MergePolicy) {
        let (_, _, scope, expires_at, attributes) = received.into_parts();
        for (name, instances) in attributes.iter_shared() {
            match (self.attributes_mut().get_mut(name), policy) {
                (Some(held), MergePolicy::Instances) => merge_instances(held, instances),
                // The attribute is present already, so an auxiliary holder adds
                // nothing to it.
                (Some(_), MergePolicy::SupplementMissing) => {}
                (None, MergePolicy::Instances | MergePolicy::SupplementMissing) => {
                    // clone: the shared instances are moved into the merged map
                    // by pointer, the received version is dropped after
                    self.attributes_mut().insert_shared(name.clone(), Arc::clone(instances));
                }
            }
        }
        // Clause 4.18: when representations of the same Entity are merged, "the
        // values of scope are merged". Clause 4.3.6.2 bounds that for an
        // auxiliary holder: it "never overrides data held directly within a
        // Context Broker", so it supplies the member only where nothing else
        // did.
        let merged_scope = match (self.scope().cloned(), scope) {
            (Some(mut held), Some(received)) => Some(match policy {
                MergePolicy::Instances => {
                    held.merge(received);
                    held
                }
                MergePolicy::SupplementMissing => held,
            }),
            (Some(held), None) => Some(held),
            (None, received) => received,
        };
        self.set_scope(merged_scope);
        let merged_expiry = match policy {
            // Clause 4.5.5.3: "If an expiresAt attribute is missing from at least
            // one version of the Entity received from registered Context Sources,
            // the expiresAt attribute is removed from the Entity. Otherwise, the
            // expiresAt attribute of the Entity is set to the DateTime
            // corresponding to the expiresAt DateTime which is furthest in the
            // future."
            MergePolicy::Instances => match (self.expires_at(), expires_at) {
                (Some(held), Some(received)) => Some(received.max(held)),
                (None, _) | (_, None) => None,
            },
            // Clause 4.3.6.2: an auxiliary version can neither raise the
            // Entity's expiry nor erase it by not reporting one.
            MergePolicy::SupplementMissing => self.expires_at(),
        };
        self.set_expires_at(merged_expiry);
    }
}

/// Fold the instances of one attribute another holder answered with into the
/// instances held for it (clause 4.5.5.3).
fn merge_instances(held: &mut AttributeInstances, received: &AttributeInstances) {
    for (dataset_id, candidate) in received.iter() {
        let wins = held
            .get(dataset_id)
            .is_none_or(|existing| recency(existing).order(&recency(candidate)) == Ordering::Less);
        if wins {
            // clone: the received version is shared and still referenced by its
            // holder's answer
            held.insert(dataset_id.cloned(), candidate.clone());
        }
    }
}

/// The clause 4.5.5.3 instants one instance carries.
fn recency(instance: &Instance) -> InstanceRecency {
    InstanceRecency {
        observed_at: instance.observed_at().map(NgsiLdDateTime::get),
        modified_at: instance.modified_at().map(SystemInstant::get),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        instance::Instance,
        operation::{
            speed_entity::{SPEED, date_time, entity, instance_rows, instances, number, property, row},
            version_merge::MergePolicy,
        },
        scopes::Scopes,
    };
    use andromeda_ngsi_ld_lexicon::time::system_instant::SystemInstant;
    use pretty_assertions::assert_eq;

    const NAME: &str = "https://example.com/name";

    fn observed(value: i64, observed_at: Option<&str>, modified_at: Option<&str>) -> Instance {
        let mut instance = property(number(value));
        instance.set_observed_at(observed_at.map(date_time));
        instance.set_modified_at(modified_at.map(|raw| raw.parse::<SystemInstant>().expect("an instant")));
        instance
    }

    #[test]
    fn instances_of_different_datasets_coexist() {
        let mut local = entity(vec![(SPEED, instances(vec![(None, observed(1, None, None))]))]);
        let remote = entity(vec![(SPEED, instances(vec![(Some("urn:ds:a"), observed(2, None, None))]))]);
        local.merge_version(remote, MergePolicy::Instances);
        assert_eq!(instance_rows(local.attributes(), SPEED), vec![row(None, 1), row(Some("urn:ds:a"), 2)]);
    }

    #[test]
    fn the_most_recent_observation_wins() {
        let mut local = entity(vec![(SPEED, instances(vec![(None, observed(1, Some("2020-01-01T00:00:00Z"), None))]))]);
        let remote = entity(vec![(SPEED, instances(vec![(None, observed(2, Some("2021-01-01T00:00:00Z"), None))]))]);
        local.merge_version(remote, MergePolicy::Instances);
        assert_eq!(instance_rows(local.attributes(), SPEED), vec![row(None, 2)]);
    }

    /// Clause 4.5.5.3: only the remote instance carries `observedAt`, so it is
    /// the one with the most recent observation.
    #[test]
    fn an_observed_instance_wins_over_an_unobserved_one() {
        let mut local = entity(vec![(SPEED, instances(vec![(None, observed(1, None, Some("2021-01-01T00:00:00Z")))]))]);
        let remote = entity(vec![(SPEED, instances(vec![(None, observed(2, Some("2020-01-01T00:00:00Z"), None))]))]);
        local.merge_version(remote, MergePolicy::Instances);
        assert_eq!(instance_rows(local.attributes(), SPEED), vec![row(None, 2)]);
    }

    #[test]
    fn modified_at_breaks_an_observation_tie() {
        let at = Some("2020-01-01T00:00:00Z");
        let mut local = entity(vec![(SPEED, instances(vec![(None, observed(1, at, Some("2020-01-01T00:00:00Z")))]))]);
        let remote = entity(vec![(SPEED, instances(vec![(None, observed(2, at, Some("2020-02-01T00:00:00Z")))]))]);
        local.merge_version(remote, MergePolicy::Instances);
        assert_eq!(instance_rows(local.attributes(), SPEED), vec![row(None, 2)]);
    }

    #[test]
    fn a_full_tie_keeps_the_local_instance() {
        let mut local = entity(vec![(SPEED, instances(vec![(None, observed(1, None, None))]))]);
        let remote = entity(vec![(SPEED, instances(vec![(None, observed(2, None, None))]))]);
        local.merge_version(remote, MergePolicy::Instances);
        assert_eq!(instance_rows(local.attributes(), SPEED), vec![row(None, 1)]);
    }

    #[test]
    fn an_auxiliary_version_only_supplements() {
        let mut local = entity(vec![(SPEED, instances(vec![(None, observed(1, Some("2020-01-01T00:00:00Z"), None))]))]);
        let remote = entity(vec![
            (SPEED, instances(vec![(None, observed(2, Some("2021-01-01T00:00:00Z"), None))])),
            (NAME, instances(vec![(None, observed(3, None, None))])),
        ]);
        local.merge_version(remote, MergePolicy::SupplementMissing);
        assert_eq!(instance_rows(local.attributes(), SPEED), vec![row(None, 1)]);
        assert_eq!(instance_rows(local.attributes(), NAME), vec![row(None, 3)]);
    }

    #[test]
    fn scope_is_unioned() {
        let mut local = entity(Vec::new());
        local.set_scope(Some(Scopes::one("/a".parse().expect("a scope"))));
        let mut remote = entity(Vec::new());
        remote.set_scope(Some(Scopes::one("/b".parse().expect("a scope"))));
        local.merge_version(remote, MergePolicy::Instances);
        let scopes: Vec<String> = local.scope().expect("scopes").as_slice().iter().map(ToString::to_string).collect();
        assert_eq!(scopes, vec!["/a".to_owned(), "/b".to_owned()]);
    }

    #[test]
    fn expiry_is_the_furthest_when_every_version_has_one() {
        let mut local = entity(Vec::new());
        local.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));
        let mut remote = entity(Vec::new());
        remote.set_expires_at(Some(date_time("2031-01-01T00:00:00Z")));
        local.merge_version(remote, MergePolicy::Instances);
        assert_eq!(local.expires_at().map(|expiry| expiry.to_string()), Some("2031-01-01T00:00:00Z".to_owned()));
    }

    #[test]
    fn expiry_is_removed_when_a_version_has_none() {
        let mut local = entity(Vec::new());
        local.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));
        local.merge_version(entity(Vec::new()), MergePolicy::Instances);
        assert!(local.expires_at().is_none());
    }

    #[test]
    fn an_auxiliary_version_leaves_the_expiry_alone() {
        let mut local = entity(Vec::new());
        local.set_expires_at(Some(date_time("2030-01-01T00:00:00Z")));
        local.merge_version(entity(Vec::new()), MergePolicy::SupplementMissing);
        assert!(local.expires_at().is_some());
    }

    /// Both holders' answers expand to the same IRI, so the merge sees one
    /// attribute whatever each holder called it.
    #[test]
    fn two_spellings_of_one_attribute_merge() {
        let mut local = entity(vec![(SPEED, instances(vec![(None, observed(1, None, None))]))]);
        let remote = entity(vec![(SPEED, instances(vec![(Some("urn:ds:b"), observed(2, None, None))]))]);
        local.merge_version(remote, MergePolicy::Instances);
        assert_eq!(local.attributes().len(), 1);
    }
}
