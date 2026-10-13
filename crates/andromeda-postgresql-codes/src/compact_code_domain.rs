//! The catalogue of every compact code domain a stored row can carry.
//!
//! A row carries several code columns, so a number that fails to decode is
//! reported together with the domain that refused it. Naming that domain by a
//! variant rather than by text keeps the report typed: the set of domains is
//! closed, and each one appears here exactly once, under the name of the type
//! that decodes it.

use strum::{Display, IntoStaticStr};

/// One compact code domain, named after the type that decodes it.
///
/// The variants mirror the registries declared with
/// [`compact_code_registry`](crate::compact_code_registry), which names its own
/// variant when it reports an unassigned number, plus the decoders declared
/// outside this crate that report through the same
/// [`UnknownCompactCode`](crate::registry::UnknownCompactCode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum CompactCodeDomain {
    /// [`AttributeChangeKindCode`](crate::attribute_change_kind::AttributeChangeKindCode).
    AttributeChangeKindCode,
    /// [`AttributeKind`](crate::attribute_kind::AttributeKind).
    AttributeKind,
    /// [`ChangeOriginCode`](crate::change_origin::ChangeOriginCode).
    ChangeOriginCode,
    /// [`CsrModeCode`](crate::csr_mode::CsrModeCode).
    CsrModeCode,
    /// [`CsrOperationCode`](crate::csr_operation::CsrOperationCode).
    CsrOperationCode,
    /// [`CsrStatusCode`](crate::csr_status::CsrStatusCode).
    CsrStatusCode,
    /// [`EntityLifecycleCode`](crate::entity_lifecycle::EntityLifecycleCode).
    EntityLifecycleCode,
    /// [`EntityTypeShapeCode`](crate::entity_type_shape::EntityTypeShapeCode).
    EntityTypeShapeCode,
    /// [`JsonldContextKindCode`](crate::jsonld_context_kind::JsonldContextKindCode).
    JsonldContextKindCode,
    /// [`MaintenanceActionResultCode`](crate::maintenance_action_result::MaintenanceActionResultCode).
    MaintenanceActionResultCode,
    /// [`MaintenanceDecisionCode`](crate::maintenance_decision::MaintenanceDecisionCode).
    MaintenanceDecisionCode,
    /// [`MaintenanceInspectionResultCode`](crate::maintenance_inspection_result::MaintenanceInspectionResultCode).
    MaintenanceInspectionResultCode,
    /// [`MaintenanceTargetCode`](crate::maintenance_target::MaintenanceTargetCode).
    MaintenanceTargetCode,
    /// [`NotificationTriggerCode`](crate::notification_trigger::NotificationTriggerCode).
    NotificationTriggerCode,
    /// The value shape a temporal aggregation projects for each row, decoded
    /// by the temporal query adapter.
    RecordedShape,
    /// [`ScopeValueKind`](crate::scope_value_kind::ScopeValueKind).
    ScopeValueKind,
    /// [`SnapshotStatusCode`](crate::snapshot_status::SnapshotStatusCode).
    SnapshotStatusCode,
    /// [`SubscriptionKindCode`](crate::subscription_kind::SubscriptionKindCode).
    SubscriptionKindCode,
    /// [`SubscriptionStatusCode`](crate::subscription_status::SubscriptionStatusCode).
    SubscriptionStatusCode,
    /// [`TemporalEntryKind`](crate::temporal_entry_kind::TemporalEntryKind).
    TemporalEntryKind,
    /// [`TenantStatusCode`](crate::tenant_status::TenantStatusCode).
    TenantStatusCode,
    /// [`ValueKind`](crate::value_kind::ValueKind).
    ValueKind,
}

#[cfg(test)]
mod tests {
    use crate::compact_code_domain::CompactCodeDomain;

    #[test]
    fn a_domain_renders_as_the_kebab_case_name_of_its_decoder() {
        pretty_assertions::assert_eq!(CompactCodeDomain::ValueKind.to_string(), "value-kind");
        pretty_assertions::assert_eq!(CompactCodeDomain::CsrOperationCode.to_string(), "csr-operation-code");
    }
}
