//! The columns of a registration row as the values a create or a replacement
//! writes, in the order the `context_source_registrations` statements list
//! them.

use crate::{
    csource_registration::registration_statements::{
        ManagementColumns,
        context_source_info_columns,
        dataset_selectors,
        geometry_writes,
        interval_columns,
        mode_code,
        operation_codes,
        stored_scopes,
    },
    store_fault::persistence_error,
};
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
    registration::{forwarding_mode::ForwardingMode, operation::CsrOperation},
    registration_management::RegistrationManagementInfo,
    time_interval::TimeInterval,
};
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, portable_pattern::RenderingRevision};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{cast::cast, external_arguments::ExternalArguments, pg_type::PostgresType};
use andromeda_registration::{error::RegistrationError, resolved_update::ResolvedCsrUpdate, store::create_params::CreateCsrParams};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use iso8601_duration::Duration as Iso8601Duration;
use mediatype::MediaTypeBuf;
use sea_query::{Expr, SimpleExpr};
use sqlx::types::Json;

/// What a create and a replacement both write of a registration, borrowed from
/// the parameters of either.
pub(super) struct RegistrationFields<'a> {
    /// `registrationName`.
    pub(super) registration_name: Option<&'a NonEmptyText>,
    /// `description`.
    pub(super) description: Option<&'a NonEmptyText>,
    /// `datasetId`.
    pub(super) dataset_id: Option<&'a [DatasetSelector]>,
    /// `tenant`.
    pub(super) tenant: Option<&'a TenantName>,
    /// `observationInterval`.
    pub(super) observation_interval: Option<&'a TimeInterval>,
    /// `managementInterval`.
    pub(super) management_interval: Option<&'a TimeInterval>,
    /// `location`.
    pub(super) location: Option<&'a Geometry>,
    /// `observationSpace`.
    pub(super) observation_space: Option<&'a Geometry>,
    /// `operationSpace`.
    pub(super) operation_space: Option<&'a Geometry>,
    /// `expiresAt`.
    pub(super) expires_at: Option<DateTime<Utc>>,
    /// `endpoint`.
    pub(super) endpoint: &'a DereferenceableUri,
    /// `accept`.
    pub(super) accept: Option<&'a MediaTypeBuf>,
    /// `mode`.
    pub(super) mode: ForwardingMode,
    /// `operations`.
    pub(super) operations: &'a [CsrOperation],
    /// `scope`.
    pub(super) scope: Option<&'a [ScopePath]>,
    /// `refreshRate`.
    pub(super) refresh_rate: Option<&'a Iso8601Duration>,
    /// `management`.
    pub(super) management: Option<&'a RegistrationManagementInfo>,
    /// `contextSourceInfo`.
    pub(super) context_source_info: Option<&'a [KeyValuePair]>,
    /// `contextSourceAlias`.
    pub(super) context_source_alias: Option<&'a ContextSourcePseudonym>,
    /// The Context Source Properties.
    pub(super) additional_properties: &'a IndexMap<AttributeName, JsonLiteral>,
}

impl<'a> From<&'a CreateCsrParams> for RegistrationFields<'a> {
    fn from(params: &'a CreateCsrParams) -> RegistrationFields<'a> {
        RegistrationFields {
            registration_name: params.registration_name.as_ref(),
            description: params.description.as_ref(),
            dataset_id: params.dataset_id.as_deref(),
            tenant: params.tenant.as_ref(),
            observation_interval: params.observation_interval.as_ref(),
            management_interval: params.management_interval.as_ref(),
            location: params.location.as_ref(),
            observation_space: params.observation_space.as_ref(),
            operation_space: params.operation_space.as_ref(),
            expires_at: params.expires_at,
            endpoint: &params.endpoint,
            accept: params.accept.as_ref(),
            mode: params.mode,
            operations: &params.operations,
            scope: params.scope.as_deref(),
            refresh_rate: params.refresh_rate.as_ref(),
            management: params.management.as_ref(),
            context_source_info: params.context_source_info.as_deref(),
            context_source_alias: params.context_source_alias.as_ref(),
            additional_properties: &params.additional_properties,
        }
    }
}

impl<'a> From<&'a ResolvedCsrUpdate> for RegistrationFields<'a> {
    fn from(values: &'a ResolvedCsrUpdate) -> RegistrationFields<'a> {
        RegistrationFields {
            registration_name: values.registration_name.as_ref(),
            description: values.description.as_ref(),
            dataset_id: values.dataset_id.as_deref(),
            tenant: values.tenant.as_ref(),
            observation_interval: values.observation_interval.as_ref(),
            management_interval: values.management_interval.as_ref(),
            location: values.location.as_ref(),
            observation_space: values.observation_space.as_ref(),
            operation_space: values.operation_space.as_ref(),
            expires_at: values.expires_at,
            endpoint: &values.endpoint,
            accept: values.accept.as_ref(),
            mode: values.mode,
            operations: &values.operations,
            scope: values.scope.as_deref(),
            refresh_rate: values.refresh_rate.as_ref(),
            management: values.management.as_ref(),
            context_source_info: values.context_source_info.as_deref(),
            context_source_alias: values.context_source_alias.as_ref(),
            additional_properties: &values.additional_properties,
        }
    }
}

/// The `additional_properties` column value: the Context Source Properties of
/// a registration, or `NULL` for a registration that has none.
fn stored_properties(properties: &IndexMap<AttributeName, JsonLiteral>) -> Option<Json<&IndexMap<AttributeName, JsonLiteral>>> {
    (!properties.is_empty()).then_some(Json(properties))
}

/// The texts of an optional list, owned by the array value that carries them.
fn owned_texts(texts: Option<Vec<&str>>) -> Option<Vec<String>> {
    texts.map(|texts| texts.into_iter().map(str::to_owned).collect())
}

/// Every column `fields` writes, each with the value it takes, in the order
/// the registration statements list them.
///
/// `additional_properties` is a `jsonb` sea-query has no value for, so it
/// travels as an external argument pushed onto `external`.
///
/// # Errors
///
/// Returns [`RegistrationError`] when a geometry cannot be encoded as EWKB.
pub(super) fn registration_columns<'a>(
    fields: &RegistrationFields<'a>,
    pattern_forms_revision: Option<RenderingRevision>,
    external: &mut ExternalArguments<'a>,
) -> Result<Vec<(Csr, SimpleExpr)>, RegistrationError> {
    let scopes = stored_scopes(fields.scope);
    let (observation_start, observation_end) = interval_columns(fields.observation_interval);
    let (management_start, management_end) = interval_columns(fields.management_interval);
    let management = ManagementColumns::of(fields.management);
    let (info_keys, info_values) = context_source_info_columns(fields.context_source_info);
    let geometry = |written: Csr, repaired: Csr, geometry: Option<&Geometry>| {
        geometry_bytes(geometry.map(Geometry::form))
            .map(|ewkb| geometry_writes(written, repaired, ewkb))
            .map_err(|error| persistence_error::<RegistrationError>(error))
    };
    let [location, location_repaired] = geometry(Csr::Location, Csr::LocationValid, fields.location)?;
    let [observation_space, observation_space_repaired] = geometry(Csr::ObservationSpace, Csr::ObservationSpaceValid, fields.observation_space)?;
    let [operation_space, operation_space_repaired] = geometry(Csr::OperationSpace, Csr::OperationSpaceValid, fields.operation_space)?;
    Ok(vec![
        (Csr::RegistrationName, Expr::val(fields.registration_name.map(NonEmptyText::as_str))),
        (Csr::Description, Expr::val(fields.description.map(NonEmptyText::as_str))),
        (Csr::DatasetId, Expr::val(dataset_selectors(fields.dataset_id))),
        (Csr::Tenant, Expr::val(fields.tenant.map(TenantName::as_str))),
        (Csr::ObservationIntervalStart, Expr::val(observation_start)),
        (Csr::ObservationIntervalEnd, Expr::val(observation_end)),
        (Csr::ManagementIntervalStart, Expr::val(management_start)),
        (Csr::ManagementIntervalEnd, Expr::val(management_end)),
        location,
        location_repaired,
        observation_space,
        observation_space_repaired,
        operation_space,
        operation_space_repaired,
        (Csr::ExpiresAt, Expr::val(fields.expires_at)),
        (Csr::Endpoint, Expr::val(fields.endpoint.as_str())),
        (Csr::Accept, Expr::val(fields.accept.map(ToString::to_string))),
        (Csr::Mode, Expr::val(mode_code(fields.mode))),
        (Csr::Operations, Expr::val(operation_codes(fields.operations))),
        (Csr::ScopePaths, cast(Expr::val(scopes.label_paths), PostgresType::LtreeArray)),
        (Csr::RefreshRate, Expr::val(fields.refresh_rate.map(ToString::to_string))),
        (Csr::IsManagementDeclared, Expr::val(management.declared)),
        (Csr::ManagementCacheDuration, Expr::val(management.cache_duration)),
        (Csr::ManagementCooldown, Expr::val(management.cooldown)),
        (Csr::ManagementLocalOnly, Expr::val(management.local_only)),
        (Csr::ManagementTimeout, Expr::val(management.timeout)),
        (Csr::ContextSourceInfoKeys, Expr::val(owned_texts(info_keys))),
        (Csr::ContextSourceInfoValues, Expr::val(owned_texts(info_values))),
        (
            Csr::ContextSourceAlias,
            Expr::val(fields.context_source_alias.map(ContextSourcePseudonym::as_str)),
        ),
        (Csr::AdditionalProperties, external.push(stored_properties(fields.additional_properties))),
        (Csr::EntityIdPatternFormsRevision, Expr::val(pattern_forms_revision.map(RenderingRevision::get))),
        (Csr::VerbatimScopePaths, Expr::val(owned_texts(scopes.written))),
    ])
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::registration_columns::stored_properties;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use indexmap::IndexMap;

    /// A registration without Context Source Properties stores `NULL`, not an
    /// empty object, which the `q` pushdown reads as having none.
    #[test]
    fn a_registration_without_properties_stores_null() {
        assert!(stored_properties(&IndexMap::new()).is_none());
    }

    /// The properties are stored as the JSON object they were written as, in
    /// the order written.
    #[test]
    fn the_properties_are_stored_as_the_object_they_were_written_as() {
        let properties: IndexMap<AttributeName, JsonLiteral> = [
            (
                "https://example.org/zeta".parse().expect("a valid name"),
                JsonLiteral::admit("1").expect("a JSON literal"),
            ),
            (
                "https://example.org/alpha".parse().expect("a valid name"),
                JsonLiteral::admit(r#"{"type": "Property", "value": "x"}"#).expect("a JSON literal"),
            ),
        ]
        .into_iter()
        .collect();

        let stored = stored_properties(&properties).expect("properties are stored");

        pretty_assertions::assert_eq!(
            serde_json::to_string(&stored.0).expect("the properties write"),
            r#"{"https://example.org/zeta":1,"https://example.org/alpha":{"type":"Property","value":"x"}}"#
        );
    }
}
