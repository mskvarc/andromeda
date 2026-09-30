//! Three-way merge of existing registration state and a partial update into a
//! fully-resolved set of update values.
//!
//! Pure service-layer logic: no I/O, no SQL. The repository applies the
//! produced [`ResolvedCsrUpdate`](crate::resolved_update::ResolvedCsrUpdate) as
//! an unconditional UPDATE.

use crate::{
    csr_member::CsrMember,
    error::RegistrationError,
    mode_rules::default_operations,
    nullable_member::NullableMember,
    registration_validation_reason::RegistrationValidationReason,
    store::update_fragment::CsrUpdateFragment,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    context_source_pseudonym::ContextSourcePseudonym,
    dereferenceable_uri::DereferenceableUri,
    non_empty_text::NonEmptyText,
    scope::path::ScopePath,
    tenant::TenantName,
};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use serde_json::Value;
use typed_builder::TypedBuilder;

/// Fully-resolved CSR update values for a single row. Every field carries
/// the final value that will be written, with merge semantics already
/// applied.
#[derive(Debug, TypedBuilder)]
pub struct ResolvedCsrUpdate {
    /// New `registration_name` value (nullable).
    pub registration_name: Option<NonEmptyText>,
    /// New `description` value (nullable).
    pub description: Option<NonEmptyText>,
    /// New `information` array, serialised as JSON.
    pub information: Vec<RegistrationInfo>,
    /// New `datasetId` list.
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// New `tenant` value.
    pub tenant: Option<TenantName>,
    /// New `observation_interval` object.
    pub observation_interval: Option<TimeInterval>,
    /// New `management_interval` object.
    pub management_interval: Option<TimeInterval>,
    /// New `expires_at` timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// New `endpoint` (required).
    pub endpoint: DereferenceableUri,
    /// New `accept` MIME type.
    pub accept: Option<MediaTypeBuf>,
    /// New forwarding mode.
    pub mode: ForwardingMode,
    /// New supported operations list; the clause 4.20 `federationOps` default
    /// is already materialised here.
    pub operations: Vec<CsrOperation>,
    /// New scope strings.
    pub scope: Option<Vec<ScopePath>>,
    /// New refresh-rate duration string.
    pub refresh_rate: Option<Iso8601Duration>,
    /// New management object.
    pub management: Option<RegistrationManagementInfo>,
    /// New context-source-info object.
    pub context_source_info: Option<Vec<KeyValuePair>>,
    /// New JSON-LD context identifier.
    pub jsonld_context: Option<DereferenceableUri>,
    /// New Via alias pseudonym.
    pub context_source_alias: Option<ContextSourcePseudonym>,
    /// New non-standard-properties blob (after shallow merge).
    pub additional_properties: Option<Value>,
    /// New `location` geometry.
    pub location: Option<Geometry>,
    /// New `observation_space` geometry.
    pub observation_space: Option<Geometry>,
    /// New `operation_space` geometry.
    pub operation_space: Option<Geometry>,
}

/// Resolve one nullable geometry member (clause 5.5.8).
///
/// An omitted member keeps whatever the stored registration holds, which is
/// still `GeoJSON` at that point and is decoded on its way into the resolved
/// value set. `member` names the payload member in the failure message.
fn resolve_geometry(fragment: &NullableMember<Geometry>, stored: Option<&Value>, member: CsrMember) -> Result<Option<Geometry>, RegistrationError> {
    match fragment {
        NullableMember::Absent => stored
            .map(Geometry::from_geojson_value)
            .transpose()
            .map_err(|source| RegistrationError::Validation(RegistrationValidationReason::ExistingGeometry { member, source })),
        NullableMember::Cleared => Ok(None),
        // clone: own fragment side; the fragment is borrowed
        NullableMember::Set(geometry) => Ok(Some(geometry.clone())),
    }
}

/// Apply a partial `CsrUpdateFragment` on top of an existing CSR, producing
/// a fully-resolved set of column values for a repository UPDATE.
///
/// Every `.clone()` below is a fragment-side borrow → owned value for the
/// resolved output: the fragment is passed by `&`, the existing row is owned
/// and consumed, and the producer needs to yield a single owned
/// [`ResolvedCsrUpdate`] either way. There is no `&`/`Cow` alternative
/// because the repository takes `&ResolvedCsrUpdate` and the struct fields
/// are `Option<T>`/`String`/`Value` without lifetime parameters.
///
/// # Errors
///
/// Returns [`RegistrationError::Validation`] when the stored registration
/// carries a `location`, `observationSpace` or `operationSpace` the fragment
/// leaves alone and that value is not decodable `GeoJSON`.
pub fn apply_fragment(existing: ContextSourceRegistration, fragment: &CsrUpdateFragment) -> Result<ResolvedCsrUpdate, RegistrationError> {
    let registration_name = fragment.registration_name.resolve(existing.registration_name);
    let description = fragment.description.resolve(existing.description);
    let information = fragment.information.clone().unwrap_or(existing.coverage.information); // clone: own the borrowed update fragment when it supplied information.
    let dataset_id = fragment.dataset_id.resolve(existing.coverage.dataset_id);
    let tenant = fragment.tenant.resolve(existing.tenant);
    let observation_interval = fragment.observation_interval.resolve(existing.coverage.observation_interval);
    let management_interval = fragment.management_interval.resolve(existing.coverage.management_interval);
    let expires_at = fragment.expires_at.resolve(existing.lifecycle.expires_at);
    let endpoint = fragment.endpoint.clone().unwrap_or(existing.access.endpoint); // clone: own the borrowed update fragment when it supplied the URL.
    let accept = fragment.accept.resolve(existing.access.accept);
    let mode = fragment.mode.unwrap_or_else(|| existing.access.mode.unwrap_or_default());
    // Clause 4.20: the merged registration falls back to its mode's default set
    // whenever it names no explicit subset.
    let operations = fragment
        .operations
        .resolve(existing.access.operations)
        .unwrap_or_else(|| default_operations(mode).to_vec());
    let scope = fragment.scope.resolve(existing.coverage.scope);
    let refresh_rate = fragment.refresh_rate.resolve(existing.access.refresh_rate);
    let management = fragment.management.resolve(existing.access.management);
    let context_source_info = fragment.context_source_info.resolve(existing.access.context_source_info);
    let jsonld_context = fragment.jsonld_context.resolve(existing.access.jsonld_context);
    let context_source_alias = fragment.context_source_alias.resolve(existing.access.context_source_alias);

    let additional_properties = match (existing.additional_properties, &fragment.additional_properties) {
        (Some(existing_val), Some(new_val)) => {
            let mut merged = existing_val;
            if let (Some(m), Some(n)) = (merged.as_object_mut(), new_val.as_object()) {
                for (k, v) in n {
                    m.insert(k.clone(), v.clone()); // clone: owned key/value for merged map insertion
                }
            }
            Some(merged)
        }
        (None, Some(new_val)) => Some(new_val.clone()), // clone: own fragment Value
        (Some(existing_val), None) => Some(existing_val),
        (None, None) => None,
    };

    let location = resolve_geometry(&fragment.location, existing.coverage.location.as_ref(), CsrMember::Location)?;
    let observation_space = resolve_geometry(
        &fragment.observation_space,
        existing.coverage.observation_space.as_ref(),
        CsrMember::ObservationSpace,
    )?;
    let operation_space = resolve_geometry(&fragment.operation_space, existing.coverage.operation_space.as_ref(), CsrMember::OperationSpace)?;

    Ok(ResolvedCsrUpdate {
        registration_name,
        description,
        information,
        dataset_id,
        tenant,
        observation_interval,
        management_interval,
        expires_at,
        endpoint,
        accept,
        mode,
        operations,
        scope,
        refresh_rate,
        management,
        context_source_info,
        jsonld_context,
        context_source_alias,
        additional_properties,
        location,
        observation_space,
        operation_space,
    })
}
