//! Clause 4.5.5.2, "Processing of Conflicting Transient Entities".
//!
//! Several holders can hold the same Entity and disagree about when it ceases
//! to exist. Clause 4.5.5.3 resolves that disagreement destructively: "If an
//! `expiresAt` attribute is missing from at least one version of the Entity
//! received from registered Context Sources, the `expiresAt` attribute is
//! removed from the Entity." Clause 4.5.5.2 is what stops the transience a
//! holder did report from being lost with it: before the versions are folded
//! together, each version that carries an entity-level `expiresAt` has that
//! instant pushed down onto its own Attributes, so the data that holder
//! contributed still stops being served when the holder said it would.
//!
//! The pass therefore runs while the versions are still separate. Once
//! [`EntityState::merge_version`] has folded a version in, the entity-level
//! value it left behind is the reconciled one and no longer belongs to any
//! single holder.

use crate::{date_time_text::DateTimeText, entity_state::EntityState};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// What the versions of one Entity seen so far report for the entity-level
/// `expiresAt`.
///
/// Clause 4.5.5.2 applies only when the value "is not duplicated across all
/// versions of the Entity received across all registered Context Sources", so
/// the only distinction that matters is whether every version reported the
/// same thing -- a version reporting nothing included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpiryAgreement {
    /// Every version seen so far reported this same value.
    Unanimous(Option<DateTime<Utc>>),
    /// Two versions reported different values, so the mechanism applies.
    Divided,
}

impl ExpiryAgreement {
    /// Fold one more version's entity-level `expiresAt` into the agreement.
    fn observing(self, reported: Option<DateTime<Utc>>) -> ExpiryAgreement {
        match self {
            ExpiryAgreement::Unanimous(seen) if seen == reported => ExpiryAgreement::Unanimous(seen),
            ExpiryAgreement::Unanimous(_) | ExpiryAgreement::Divided => ExpiryAgreement::Divided,
        }
    }
}

/// Differentiate the conflicting transient versions among `versions` (clause
/// 4.5.5.2).
///
/// `versions` are every party to the conflict, whatever entity each is a
/// version of: the copies this broker holds and the versions the non-auxiliary
/// holders answered with. Clause 4.3.6.2 keeps an auxiliary holder out -- its
/// data "is only included if it is supplementary to the context data otherwise
/// available to the Context Broker" -- so the caller leaves its versions out.
pub fn differentiate_transient_versions<'v>(versions: impl IntoIterator<Item = &'v mut EntityState>) {
    let mut versions: Vec<&'v mut EntityState> = versions.into_iter().collect();
    let mut agreement: HashMap<Identifier, ExpiryAgreement> = HashMap::new();
    for version in &versions {
        let reported = version.expires_at().map(DateTimeText::instant);
        let folded = agreement
            .get(version.id())
            .map_or(ExpiryAgreement::Unanimous(reported), |seen| seen.observing(reported));
        // clone: the agreement outlives the borrow of each version it folds
        agreement.insert(version.id().clone(), folded);
    }
    for version in &mut versions {
        if agreement.get(version.id()) == Some(&ExpiryAgreement::Divided) {
            constrain_attributes_to_entity_expiry(version);
        }
    }
}

/// Give every Attribute of one version an `expiresAt` no later than that
/// version's own entity-level one (clause 4.5.5.2).
///
/// "If no `expiresAt` attribute is present as a property of an Attribute, a
/// non-reified `expiresAt` attribute is added as a property of that Attribute
/// corresponding to the `expiresAt` found on the Entity", and "if the value on
/// the Attribute is further in the future than the `expiresAt` value found on
/// the Entity, the value of the `expiresAt` on the Attribute is overwritten to
/// corresponding to the earlier `DateTime`". An Attribute that already expires
/// no later than the Entity is left exactly as the holder sent it.
fn constrain_attributes_to_entity_expiry(version: &mut EntityState) {
    let Some(entity_expiry) = version.expires_at().cloned() else {
        return;
    };
    let late: Vec<_> = version
        .attributes()
        .iter()
        .filter(|(_, instances)| {
            instances
                .iter()
                .any(|(_, instance)| instance.expires_at().is_none_or(|own| own.instant() > entity_expiry.instant()))
        })
        .map(|(name, _)| name.clone()) // clone: the names outlive the borrow the scan holds
        .collect();
    for name in late {
        if let Some(instances) = version.attributes_mut().get_mut(&name) {
            for (_, instance) in instances.iter_mut() {
                if instance.expires_at().is_none_or(|own| own.instant() > entity_expiry.instant()) {
                    // clone: every constrained instance owns its expiry
                    instance.set_expires_at(Some(entity_expiry.clone()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_state::EntityState,
        operation::{
            speed_entity::{SPEED, date_time, entity, instances, number, property},
            transient_versions::differentiate_transient_versions,
        },
    };
    use pretty_assertions::assert_eq;

    fn version(expires_at: Option<&str>, attribute_expiry: Option<&str>) -> EntityState {
        let mut instance = property(number(1));
        instance.set_expires_at(attribute_expiry.map(date_time));
        let mut state = entity(vec![(SPEED, instances(vec![(None, instance)]))]);
        state.set_expires_at(expires_at.map(date_time));
        state
    }

    fn attribute_expiry(state: &EntityState) -> Option<String> {
        let (_, instance) = state.attributes().get(SPEED).expect("speed").iter().next().expect("one instance");
        instance.expires_at().map(|expiry| expiry.as_str().to_owned())
    }

    /// Clause 4.5.5.2: two versions disagreeing on the entity-level expiry
    /// each push their own onto their Attributes.
    #[test]
    fn divided_versions_push_their_expiry_onto_their_attributes() {
        let mut held = version(Some("2030-01-01T00:00:00Z"), None);
        let mut received = version(None, None);
        differentiate_transient_versions([&mut held, &mut received]);
        assert_eq!(attribute_expiry(&held), Some("2030-01-01T00:00:00Z".to_owned()));
        assert_eq!(attribute_expiry(&received), None);
    }

    /// An expiry duplicated across every version is no conflict.
    #[test]
    fn unanimous_versions_are_left_alone() {
        let mut held = version(Some("2030-01-01T00:00:00Z"), None);
        let mut received = version(Some("2030-01-01T00:00:00Z"), None);
        differentiate_transient_versions([&mut held, &mut received]);
        assert_eq!(attribute_expiry(&held), None);
    }

    /// An Attribute already expiring earlier keeps its own expiry; a later one
    /// is brought forward.
    #[test]
    fn only_a_later_attribute_expiry_is_overwritten() {
        let mut earlier = version(Some("2030-01-01T00:00:00Z"), Some("2029-01-01T00:00:00Z"));
        let mut later = version(Some("2031-01-01T00:00:00Z"), Some("2035-01-01T00:00:00Z"));
        differentiate_transient_versions([&mut earlier, &mut later]);
        assert_eq!(attribute_expiry(&earlier), Some("2029-01-01T00:00:00Z".to_owned()));
        assert_eq!(attribute_expiry(&later), Some("2031-01-01T00:00:00Z".to_owned()));
    }
}
