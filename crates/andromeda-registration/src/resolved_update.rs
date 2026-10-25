//! Three-way merge of existing registration state and a partial update into a
//! fully-resolved set of update values.
//!
//! Pure service-layer logic: no I/O, no SQL. The repository applies the
//! produced [`ResolvedCsrUpdate`](crate::resolved_update::ResolvedCsrUpdate) as
//! an unconditional UPDATE.

use crate::{mode_rules::default_operations, store::update_fragment::CsrUpdateFragment};
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
    key_value_pair::KeyValuePair,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode, info::RegistrationInfo, operation::CsrOperation},
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use mediatype::MediaTypeBuf;
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
    /// New Via alias pseudonym.
    pub context_source_alias: Option<ContextSourcePseudonym>,
    /// New Context Source Properties (after the shallow merge).
    pub additional_properties: IndexMap<AttributeName, JsonLiteral>,
    /// New `location` geometry.
    pub location: Option<Geometry>,
    /// New `observation_space` geometry.
    pub observation_space: Option<Geometry>,
    /// New `operation_space` geometry.
    pub operation_space: Option<Geometry>,
}

/// Apply a partial `CsrUpdateFragment` on top of an existing CSR, producing
/// a fully-resolved set of column values for a repository UPDATE.
///
/// Every `.clone()` below is a fragment-side borrow → owned value for the
/// resolved output: the fragment is passed by `&`, the existing row is owned
/// and consumed, and the producer needs to yield a single owned
/// [`ResolvedCsrUpdate`] either way. There is no `&`/`Cow` alternative
/// because the repository takes `&ResolvedCsrUpdate` and the struct fields
/// are `Option<T>`/`Vec<T>` without lifetime parameters.
#[must_use]
pub fn apply_fragment(existing: ContextSourceRegistration, fragment: &CsrUpdateFragment) -> ResolvedCsrUpdate {
    let registration_name = fragment.registration_name.resolved_over(existing.registration_name);
    let description = fragment.description.resolved_over(existing.description);
    let information = fragment.information.clone().unwrap_or(existing.coverage.information); // clone: own the borrowed update fragment when it supplied information.
    let dataset_id = fragment.dataset_id.resolved_over(existing.coverage.dataset_id);
    let tenant = fragment.tenant.resolved_over(existing.tenant);
    let observation_interval = fragment.observation_interval.resolved_over(existing.coverage.observation_interval);
    let management_interval = fragment.management_interval.resolved_over(existing.coverage.management_interval);
    let expires_at = fragment.expires_at.resolved_over(existing.lifecycle.expires_at);
    let endpoint = fragment.endpoint.clone().unwrap_or(existing.access.endpoint); // clone: own the borrowed update fragment when it supplied the URL.
    let accept = fragment.accept.resolved_over(existing.access.accept);
    let mode = fragment.mode.unwrap_or_else(|| existing.access.mode.unwrap_or_default());
    // Clause 4.20: the merged registration falls back to its mode's default set
    // whenever it names no explicit subset.
    let operations = fragment
        .operations
        .resolved_over(existing.access.operations)
        .unwrap_or_else(|| default_operations(mode).to_vec());
    let scope = fragment.scope.resolved_over(existing.coverage.scope);
    let refresh_rate = fragment.refresh_rate.resolved_over(existing.access.refresh_rate);
    let management = fragment.management.resolved_over(existing.access.management);
    let context_source_info = fragment.context_source_info.resolved_over(existing.access.context_source_info);
    let context_source_alias = fragment.context_source_alias.resolved_over(existing.access.context_source_alias);

    // A property the fragment names replaces the stored one in place, or is
    // deleted when the fragment wrote it as the NGSI-LD Null (clause 5.5.8); the
    // others are kept, so the shallow merge keeps every stored property's
    // position.
    let mut additional_properties = existing.additional_properties;
    for (name, value) in &fragment.additional_properties {
        if value.is_null_sentinel() {
            additional_properties.shift_remove(name);
        } else {
            additional_properties.insert(name.clone(), value.clone()); // clone: the fragment is borrowed and the resolved update owns what it writes
        }
    }

    let location = fragment.location.resolved_over(existing.coverage.location);
    let observation_space = fragment.observation_space.resolved_over(existing.coverage.observation_space);
    let operation_space = fragment.operation_space.resolved_over(existing.coverage.operation_space);

    ResolvedCsrUpdate {
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
        context_source_alias,
        additional_properties,
        location,
        observation_space,
        operation_space,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{resolved_update::apply_fragment, store::update_fragment::CsrUpdateFragment};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::{clearable::Clearable, registration::ContextSourceRegistration};
    use serde_json::json;

    fn stored() -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:merge",
            "type": "ContextSourceRegistration",
            "information": [{"propertyNames": ["speed"]}],
            "location": {"type": "Point", "coordinates": [1, 2]},
            "operationSpace": {"type": "Point", "coordinates": [3, 4]},
            "endpoint": "https://source.example.com/",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z",
            "alpha": 1,
            "beta": 2
        }))
        .expect("a stored registration")
    }

    fn property(name: &str, value: &str) -> (AttributeName, JsonLiteral) {
        (name.parse().expect("a valid name"), JsonLiteral::admit(value).expect("a JSON literal"))
    }

    /// Clause 5.5.8: the fragment's Context Source Properties are merged
    /// shallowly, each replacing the stored property of its name where it
    /// stands.
    #[test]
    fn a_fragment_property_replaces_the_stored_one_in_place_and_the_others_are_kept() {
        let fragment = CsrUpdateFragment {
            additional_properties: [property("alpha", r#"{"n": 10}"#), property("gamma", "3")].into_iter().collect(),
            ..CsrUpdateFragment::default()
        };

        let resolved = apply_fragment(stored(), &fragment);

        let merged: Vec<(&str, &str)> = resolved
            .additional_properties
            .iter()
            .map(|(name, value)| (name.as_str(), value.text()))
            .collect();
        pretty_assertions::assert_eq!(merged, vec![("alpha", r#"{"n":10}"#), ("beta", "2"), ("gamma", "3")]);
    }

    /// Clause 5.5.8: a Context Source Property written as the NGSI-LD Null
    /// deletes the stored one, and the Null is never stored as a value.
    #[test]
    fn a_fragment_property_written_as_the_ngsi_ld_null_deletes_the_stored_one() {
        let fragment = CsrUpdateFragment {
            additional_properties: [property("alpha", r#""urn:ngsi-ld:null""#), property("gamma", r#""urn:ngsi-ld:null""#)]
                .into_iter()
                .collect(),
            ..CsrUpdateFragment::default()
        };

        let resolved = apply_fragment(stored(), &fragment);

        let merged: Vec<&str> = resolved.additional_properties.keys().map(AttributeName::as_str).collect();
        pretty_assertions::assert_eq!(merged, vec!["beta"]);
    }

    /// Clause 5.5.8: an omitted geometry keeps the stored one, the NGSI-LD Null
    /// deletes it, and a value replaces it.
    #[test]
    fn a_geometry_is_kept_removed_or_replaced_as_the_fragment_says() {
        let replacement = serde_json::from_value(json!({"type": "Point", "coordinates": [9, 9]})).expect("a geometry");
        let fragment = CsrUpdateFragment {
            location: Clearable::Absent,
            observation_space: Clearable::NgsiLdNull,
            operation_space: Clearable::Set(replacement),
            ..CsrUpdateFragment::default()
        };

        let resolved = apply_fragment(stored(), &fragment);

        pretty_assertions::assert_eq!(
            resolved.location.map(|geometry| serde_json::to_value(geometry).expect("a geometry writes")),
            Some(json!({"type": "Point", "coordinates": [1, 2]}))
        );
        assert!(resolved.observation_space.is_none());
        pretty_assertions::assert_eq!(
            resolved
                .operation_space
                .map(|geometry| serde_json::to_value(geometry).expect("a geometry writes")),
            Some(json!({"type": "Point", "coordinates": [9, 9]}))
        );
    }
}
