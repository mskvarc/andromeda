//! The freeze every compact code registry declares, gathered for a check
//! against the seal ledger.
//!
//! A registry's own tests hold its frozen registrations unchanged, but cannot
//! tell whether the freeze covers what the last release shipped, because the
//! releases are recorded in the seal ledger rather than in the registry. This
//! module lists every registry's freeze under its domain so that one check
//! reading the ledger can hold all of them to the last sealed release.

use crate::{
    attribute_change_kind::AttributeChangeKindCode,
    attribute_kind::AttributeKind,
    change_origin::ChangeOriginCode,
    compact_code_domain::CompactCodeDomain,
    csr_mode::CsrModeCode,
    csr_operation::CsrOperationCode,
    csr_status::CsrStatusCode,
    entity_lifecycle::EntityLifecycleCode,
    entity_type_shape::EntityTypeShapeCode,
    jsonld_context_kind::JsonldContextKindCode,
    maintenance_action_result::MaintenanceActionResultCode,
    maintenance_decision::MaintenanceDecisionCode,
    maintenance_inspection_result::MaintenanceInspectionResultCode,
    maintenance_repack_phase::MaintenanceRepackPhaseCode,
    maintenance_target::MaintenanceTargetCode,
    notification_trigger::NotificationTriggerCode,
    posted_query_kind::PostedQueryKindCode,
    recorded_shape::RecordedShape,
    registry::frozen_prefix_digest,
    scope_value_kind::ScopeValueKind,
    snapshot_status::SnapshotStatusCode,
    subscription_kind::SubscriptionKindCode,
    subscription_status::SubscriptionStatusCode,
    temporal_entry_kind::TemporalEntryKind,
    temporal_intent_reach::TemporalIntentReach,
    tenant_status::TenantStatusCode,
    value_kind::ValueKind,
};
use strum::IntoEnumIterator;

/// Expands to the [`RegistryFreeze`] of the registry `$domain` names, matching
/// every listed registry under the [`CompactCodeDomain`] variant of the same
/// name. The match is exhaustive, so a domain without a listed registry does
/// not compile.
macro_rules! registry_freeze_of {
    ($domain:expr; $($registry:ident),+ $(,)?) => {
        match $domain {
            $(CompactCodeDomain::$registry => RegistryFreeze {
                domain: CompactCodeDomain::$registry,
                frozen_at: $registry::FROZEN_AT,
                frozen_prefix: $registry::FROZEN_PREFIX,
                registrations: $registry::REGISTRATIONS,
            },)+
        }
    };
}

/// The freeze one compact code registry declares.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryFreeze {
    /// The domain the registry decodes.
    domain: CompactCodeDomain,
    /// The stable release the freeze was last brought up to.
    frozen_at: semver::Version,
    /// How many leading registrations the freeze covers.
    frozen_prefix: usize,
    /// Every registration the registry assigns, in registration order.
    registrations: &'static [(i16, &'static str)],
}

impl RegistryFreeze {
    /// The freeze the registry of `domain` declares.
    #[must_use]
    pub const fn of(domain: CompactCodeDomain) -> RegistryFreeze {
        registry_freeze_of!(
            domain;
            AttributeChangeKindCode,
            AttributeKind,
            ChangeOriginCode,
            CsrModeCode,
            CsrOperationCode,
            CsrStatusCode,
            EntityLifecycleCode,
            EntityTypeShapeCode,
            JsonldContextKindCode,
            MaintenanceActionResultCode,
            MaintenanceDecisionCode,
            MaintenanceInspectionResultCode,
            MaintenanceRepackPhaseCode,
            MaintenanceTargetCode,
            NotificationTriggerCode,
            PostedQueryKindCode,
            RecordedShape,
            ScopeValueKind,
            SnapshotStatusCode,
            SubscriptionKindCode,
            SubscriptionStatusCode,
            TemporalEntryKind,
            TemporalIntentReach,
            TenantStatusCode,
            ValueKind,
        )
    }

    /// The freeze of every registry, one for each domain.
    pub fn every() -> impl Iterator<Item = RegistryFreeze> {
        CompactCodeDomain::iter().map(RegistryFreeze::of)
    }

    /// The domain the registry decodes.
    #[must_use]
    pub const fn domain(&self) -> CompactCodeDomain {
        self.domain
    }

    /// The stable release the freeze was last brought up to.
    #[must_use]
    pub const fn frozen_at(&self) -> &semver::Version {
        &self.frozen_at
    }

    /// How many leading registrations the freeze covers.
    #[must_use]
    pub const fn frozen_prefix(&self) -> usize {
        self.frozen_prefix
    }

    /// How many codes the registry assigns.
    #[must_use]
    pub const fn assigned(&self) -> usize {
        self.registrations.len()
    }

    /// The frozen digest of a freeze covering every code the registry
    /// assigns, which is what a release sealed now ships.
    #[must_use]
    pub const fn digest_of_every_code(&self) -> u64 {
        frozen_prefix_digest(self.registrations, self.registrations.len())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_kind::AttributeKind,
        compact_code_domain::CompactCodeDomain,
        registry::frozen_prefix_digest,
        registry_freeze::RegistryFreeze,
        value_kind::ValueKind,
    };
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn every_domain_has_one_freeze_declared_by_the_registry_named_after_it() {
        let domains: Vec<CompactCodeDomain> = RegistryFreeze::every().map(|freeze| freeze.domain()).collect();
        assert_eq!(domains, CompactCodeDomain::iter().collect::<Vec<_>>());
    }

    #[test]
    fn a_freeze_reads_the_constants_its_registry_declares() {
        let freeze = RegistryFreeze::of(CompactCodeDomain::ValueKind);
        assert_eq!(freeze.frozen_at(), &ValueKind::FROZEN_AT);
        assert_eq!(freeze.frozen_prefix(), ValueKind::FROZEN_PREFIX);
        assert_eq!(freeze.assigned(), ValueKind::ALL.len());
    }

    #[test]
    fn the_digest_of_every_code_covers_the_whole_registry() {
        let freeze = RegistryFreeze::of(CompactCodeDomain::AttributeKind);
        assert_eq!(
            freeze.digest_of_every_code(),
            frozen_prefix_digest(AttributeKind::REGISTRATIONS, AttributeKind::REGISTRATIONS.len())
        );
    }
}
