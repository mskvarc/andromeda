//! The complete input one Create Context Source Registration writes (ETSI GS CIM 009 clause 5.9.2).

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    non_empty_text::NonEmptyText,
    scope::path::ScopePath,
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
use indexmap::IndexMap;
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use typed_builder::TypedBuilder;

/// Parameters for creating a new context source registration.
#[derive(Debug, TypedBuilder)]
pub struct CreateCsrParams {
    /// Human-readable name of the registration.
    #[builder(default = Default::default())]
    pub registration_name: Option<NonEmptyText>,
    /// Free-text description of the registration.
    #[builder(default = Default::default())]
    pub description: Option<NonEmptyText>,
    /// Registration information entries describing provided entities/attributes.
    pub information: Vec<RegistrationInfo>,
    /// The `datasetId`s of the attribute instances the source provides.
    #[builder(default = Default::default())]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Tenant identifier for multi-tenancy.
    #[builder(default = Default::default())]
    pub tenant: Option<TenantName>,
    /// Interval the source's observations span.
    #[builder(default = Default::default())]
    pub observation_interval: Option<TimeInterval>,
    /// Interval the source's management data spans.
    #[builder(default = Default::default())]
    pub management_interval: Option<TimeInterval>,
    /// The `location` geometry.
    #[builder(default = Default::default())]
    pub location: Option<Geometry>,
    /// The `observationSpace` geometry.
    #[builder(default = Default::default())]
    pub observation_space: Option<Geometry>,
    /// The `operationSpace` geometry.
    #[builder(default = Default::default())]
    pub operation_space: Option<Geometry>,
    /// Expiration timestamp for the registration.
    #[builder(default = Default::default())]
    pub expires_at: Option<DateTime<Utc>>,
    /// Base endpoint of the context source, as the producer wrote it.
    pub endpoint: DereferenceableUri,
    /// Preferred MIME type for responses from the context source.
    #[builder(default = Default::default())]
    pub accept: Option<MediaTypeBuf>,
    /// Forwarding mode for distributed operations.
    #[builder(default = Default::default())]
    pub mode: ForwardingMode,
    /// NGSI-LD operations supported by the context source; the clause 4.20
    /// `federationOps` default is already materialised here.
    pub operations: Vec<CsrOperation>,
    /// Scope values for the registration.
    #[builder(default = Default::default())]
    pub scope: Option<Vec<ScopePath>>,
    /// ISO 8601 duration between context source refreshes.
    #[builder(default = Default::default())]
    pub refresh_rate: Option<Iso8601Duration>,
    /// Management information for the registration.
    #[builder(default = Default::default())]
    pub management: Option<RegistrationManagementInfo>,
    /// Additional context source metadata entries.
    #[builder(default = Default::default())]
    pub context_source_info: Option<Vec<KeyValuePair>>,
    /// Unique pseudonym for Via-based loop detection.
    #[builder(default = Default::default())]
    pub context_source_alias: Option<ContextSourcePseudonym>,
    /// The Context Source Properties preserved from the original request, by
    /// the expanded name they are stored under.
    #[builder(default = Default::default())]
    pub additional_properties: IndexMap<AttributeName, JsonLiteral>,
}
