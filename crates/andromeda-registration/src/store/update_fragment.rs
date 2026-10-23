//! The typed fragment one Update Context Source Registration carries (ETSI GS CIM 009 clauses 5.9.3 and 5.5.8).

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    duration::Iso8601Duration,
    non_empty_text::NonEmptyText,
    scope::path::ScopePath,
    tenant::TenantName,
};
use andromeda_ngsi_ld_model::{
    clearable::Clearable,
    key_value_pair::KeyValuePair,
    registration::{forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use mediatype::MediaTypeBuf;
use typed_builder::TypedBuilder;

/// Typed fragment for CSR updates.
///
/// Every member clause 5.5.8 lets a fragment delete is a [`Clearable`]:
/// omitted keeps the stored value, the NGSI-LD Null deletes it and a value
/// replaces it. The members the clause gives no deletion semantics carry a
/// plain [`Option`], where `None` simply means the fragment left them alone.
#[derive(Debug, Default, TypedBuilder)]
pub struct CsrUpdateFragment {
    /// Updated registration name.
    #[builder(default = Default::default())]
    pub registration_name: Clearable<NonEmptyText>,
    /// Updated description.
    #[builder(default = Default::default())]
    pub description: Clearable<NonEmptyText>,
    /// Updated registration information entries.
    #[builder(default = Default::default())]
    pub information: Option<Vec<RegistrationInfo>>,
    /// Updated dataset identifiers.
    #[builder(default = Default::default())]
    pub dataset_id: Clearable<Vec<DatasetSelector>>,
    /// Updated tenant identifier.
    #[builder(default = Default::default())]
    pub tenant: Clearable<TenantName>,
    /// Updated observation interval.
    #[builder(default = Default::default())]
    pub observation_interval: Clearable<TimeInterval>,
    /// Updated management interval.
    #[builder(default = Default::default())]
    pub management_interval: Clearable<TimeInterval>,
    /// Updated `location` geometry.
    #[builder(default = Default::default())]
    pub location: Clearable<Geometry>,
    /// Updated `observationSpace` geometry.
    #[builder(default = Default::default())]
    pub observation_space: Clearable<Geometry>,
    /// Updated `operationSpace` geometry.
    #[builder(default = Default::default())]
    pub operation_space: Clearable<Geometry>,
    /// Updated expiration timestamp.
    #[builder(default = Default::default())]
    pub expires_at: Clearable<DateTime<Utc>>,
    /// Updated endpoint, as the producer wrote it.
    #[builder(default = Default::default())]
    pub endpoint: Option<DereferenceableUri>,
    /// Updated accepted MIME type.
    #[builder(default = Default::default())]
    pub accept: Clearable<MediaTypeBuf>,
    /// Updated forwarding mode.
    #[builder(default = Default::default())]
    pub mode: Option<ForwardingMode>,
    /// Updated supported operations.
    #[builder(default = Default::default())]
    pub operations: Clearable<Vec<CsrOperation>>,
    /// Updated scope values.
    #[builder(default = Default::default())]
    pub scope: Clearable<Vec<ScopePath>>,
    /// Updated refresh rate duration.
    #[builder(default = Default::default())]
    pub refresh_rate: Clearable<Iso8601Duration>,
    /// Updated management information.
    #[builder(default = Default::default())]
    pub management: Clearable<RegistrationManagementInfo>,
    /// Updated context source metadata entries.
    #[builder(default = Default::default())]
    pub context_source_info: Clearable<Vec<KeyValuePair>>,
    /// Updated context source alias pseudonym.
    #[builder(default = Default::default())]
    pub context_source_alias: Clearable<ContextSourcePseudonym>,
    /// The Context Source Properties the fragment writes, by the expanded name
    /// they are stored under; each replaces the stored property of that name,
    /// or deletes it when it is the NGSI-LD Null.
    #[builder(default = Default::default())]
    pub additional_properties: IndexMap<AttributeName, JsonLiteral>,
}

impl CsrUpdateFragment {
    /// Whether the fragment carries no member to apply, as the empty Fragment
    /// `{}` does: clause 5.5.8 acts only for each member of the Fragment, so
    /// such an update modifies nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let CsrUpdateFragment {
            registration_name,
            description,
            information,
            dataset_id,
            tenant,
            observation_interval,
            management_interval,
            location,
            observation_space,
            operation_space,
            expires_at,
            endpoint,
            accept,
            mode,
            operations,
            scope,
            refresh_rate,
            management,
            context_source_info,
            context_source_alias,
            additional_properties,
        } = self;
        registration_name.is_absent()
            && description.is_absent()
            && information.is_none()
            && dataset_id.is_absent()
            && tenant.is_absent()
            && observation_interval.is_absent()
            && management_interval.is_absent()
            && location.is_absent()
            && observation_space.is_absent()
            && operation_space.is_absent()
            && expires_at.is_absent()
            && endpoint.is_none()
            && accept.is_absent()
            && mode.is_none()
            && operations.is_absent()
            && scope.is_absent()
            && refresh_rate.is_absent()
            && management.is_absent()
            && context_source_info.is_absent()
            && context_source_alias.is_absent()
            && additional_properties.is_empty()
    }
}
