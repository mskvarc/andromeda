//! The typed fragment one Update Context Source Registration carries (ETSI GS CIM 009 clauses 5.9.3 and 5.5.8).

use crate::nullable_member::NullableMember;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    non_empty_text::NonEmptyText,
    scope::grammar::ScopeBuf,
    tenant::TenantName,
};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde_json::Value;
use typed_builder::TypedBuilder;

/// Typed fragment for CSR updates.
///
/// Every member clause 5.5.8 lets a fragment remove is a
/// [`NullableMember`]: omitted keeps the stored value, `null` clears it and a
/// value replaces it. The members the clause gives no removal semantics carry
/// a plain [`Option`], where `None` simply means the fragment left them alone.
#[derive(Debug, Default, TypedBuilder)]
pub struct CsrUpdateFragment {
    /// Updated registration name.
    #[builder(default = Default::default())]
    pub registration_name: NullableMember<NonEmptyText>,
    /// Updated description.
    #[builder(default = Default::default())]
    pub description: NullableMember<NonEmptyText>,
    /// Updated registration information entries.
    #[builder(default = Default::default())]
    pub information: Option<Vec<RegistrationInfo>>,
    /// Updated dataset identifiers.
    #[builder(default = Default::default())]
    pub dataset_id: NullableMember<Vec<DatasetSelector>>,
    /// Updated tenant identifier.
    #[builder(default = Default::default())]
    pub tenant: NullableMember<TenantName>,
    /// Updated observation interval.
    #[builder(default = Default::default())]
    pub observation_interval: NullableMember<TimeInterval>,
    /// Updated management interval.
    #[builder(default = Default::default())]
    pub management_interval: NullableMember<TimeInterval>,
    /// Updated `location` geometry.
    #[builder(default = Default::default())]
    pub location: NullableMember<Geometry>,
    /// Updated `observationSpace` geometry.
    #[builder(default = Default::default())]
    pub observation_space: NullableMember<Geometry>,
    /// Updated `operationSpace` geometry.
    #[builder(default = Default::default())]
    pub operation_space: NullableMember<Geometry>,
    /// Updated expiration timestamp.
    #[builder(default = Default::default())]
    pub expires_at: NullableMember<DateTime<Utc>>,
    /// Updated endpoint, as the producer wrote it.
    #[builder(default = Default::default())]
    pub endpoint: Option<DereferenceableUri>,
    /// Updated accepted MIME type.
    #[builder(default = Default::default())]
    pub accept: NullableMember<MediaTypeBuf>,
    /// Updated forwarding mode.
    #[builder(default = Default::default())]
    pub mode: Option<ForwardingMode>,
    /// Updated supported operations.
    #[builder(default = Default::default())]
    pub operations: NullableMember<Vec<CsrOperation>>,
    /// Updated scope values.
    #[builder(default = Default::default())]
    pub scope: NullableMember<Vec<ScopeBuf>>,
    /// Updated refresh rate duration.
    #[builder(default = Default::default())]
    pub refresh_rate: NullableMember<Iso8601Duration>,
    /// Updated management information.
    #[builder(default = Default::default())]
    pub management: NullableMember<RegistrationManagementInfo>,
    /// Updated context source metadata entries.
    #[builder(default = Default::default())]
    pub context_source_info: NullableMember<Vec<KeyValuePair>>,
    /// Updated JSON-LD context reference.
    #[builder(default = Default::default())]
    pub jsonld_context: NullableMember<DereferenceableUri>,
    /// Updated context source alias pseudonym.
    #[builder(default = Default::default())]
    pub context_source_alias: NullableMember<ContextSourcePseudonym>,
    /// Updated non-standard properties.
    #[builder(default = Default::default())]
    pub additional_properties: Option<Value>,
}
