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
    non_empty_text::NonEmptyText,
    tenant::TenantName,
};
use andromeda_ngsi_ld_model::registration::{context_source_property_document::ContextSourcePropertyMembers, operation::CsrOperation};
use andromeda_ngsi_ld_query::portable_pattern::RenderingRevision;
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{cast::cast, external_arguments::ExternalArguments, pg_type::PostgresType};
use andromeda_registration::{error::RegistrationError, resolved_update::ResolvedCsrUpdate, store::create_params::CreateCsrParams};
use indexmap::IndexMap;
use mediatype::MediaTypeBuf;
use sea_query::{Expr, SimpleExpr};
use sqlx::types::Json;

/// What a create and a replacement both write of a registration, borrowed from
/// the parameters of either.
pub(super) struct RegistrationFields<'a> {
    /// `accept`, which Table 5.2.9-1 does not define and so is no Context
    /// Source Property.
    pub(super) accept: Option<&'a MediaTypeBuf>,
    /// Every other member, as the `csf` document reads them.
    pub(super) members: ContextSourcePropertyMembers<'a>,
}

/// The `operations` member as the store holds it: an empty list is the absent
/// member, stored `NULL` and read back as absent.
fn declared_operations(operations: &[CsrOperation]) -> Option<&[CsrOperation]> {
    (!operations.is_empty()).then_some(operations)
}

impl<'a> From<&'a CreateCsrParams> for RegistrationFields<'a> {
    fn from(params: &'a CreateCsrParams) -> RegistrationFields<'a> {
        RegistrationFields {
            accept: params.accept.as_ref(),
            members: ContextSourcePropertyMembers::builder()
                .endpoint(&params.endpoint)
                .context_source_info(params.context_source_info.as_deref())
                .information(&params.information)
                .context_source_alias(params.context_source_alias.as_ref())
                .description(params.description.as_ref())
                .dataset_id(params.dataset_id.as_deref())
                .expires_at(params.expires_at)
                .location(params.location.as_ref())
                .management(params.management.as_ref())
                .management_interval(params.management_interval.as_ref())
                .mode(params.mode)
                .observation_interval(params.observation_interval.as_ref())
                .observation_space(params.observation_space.as_ref())
                .operations(declared_operations(&params.operations))
                .operation_space(params.operation_space.as_ref())
                .refresh_rate(params.refresh_rate.as_ref())
                .registration_name(params.registration_name.as_ref())
                .scope(params.scope.as_deref())
                .tenant(params.tenant.as_ref())
                .added(&params.additional_properties)
                .build(),
        }
    }
}

impl<'a> From<&'a ResolvedCsrUpdate> for RegistrationFields<'a> {
    fn from(values: &'a ResolvedCsrUpdate) -> RegistrationFields<'a> {
        RegistrationFields {
            accept: values.accept.as_ref(),
            members: ContextSourcePropertyMembers::builder()
                .endpoint(&values.endpoint)
                .context_source_info(values.context_source_info.as_deref())
                .information(&values.information)
                .context_source_alias(values.context_source_alias.as_ref())
                .description(values.description.as_ref())
                .dataset_id(values.dataset_id.as_deref())
                .expires_at(values.expires_at)
                .location(values.location.as_ref())
                .management(values.management.as_ref())
                .management_interval(values.management_interval.as_ref())
                .mode(values.mode)
                .observation_interval(values.observation_interval.as_ref())
                .observation_space(values.observation_space.as_ref())
                .operations(declared_operations(&values.operations))
                .operation_space(values.operation_space.as_ref())
                .refresh_rate(values.refresh_rate.as_ref())
                .registration_name(values.registration_name.as_ref())
                .scope(values.scope.as_deref())
                .tenant(values.tenant.as_ref())
                .added(&values.additional_properties)
                .build(),
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
/// `additional_properties` and `context_source_properties` are `jsonb`
/// sea-query has no value for, so they travel as external arguments pushed
/// onto `external`.
///
/// # Errors
///
/// Returns [`RegistrationError`] when a geometry cannot be encoded as EWKB or
/// the Context Source Property document cannot be written.
pub(super) fn registration_columns<'a>(
    fields: &RegistrationFields<'a>,
    pattern_forms_revision: Option<RenderingRevision>,
    external: &mut ExternalArguments<'a>,
) -> Result<Vec<(Csr, SimpleExpr)>, RegistrationError> {
    let members = &fields.members;
    let scopes = stored_scopes(members.scope);
    let (observation_start, observation_end) = interval_columns(members.observation_interval);
    let (management_start, management_end) = interval_columns(members.management_interval);
    let management = ManagementColumns::of(members.management);
    let (info_keys, info_values) = context_source_info_columns(members.context_source_info);
    let geometry = |written: Csr, repaired: Csr, geometry: Option<&Geometry>| {
        geometry_bytes(geometry.map(Geometry::form))
            .map(|ewkb| geometry_writes(written, repaired, ewkb))
            .map_err(|error| persistence_error::<RegistrationError>(error))
    };
    let [location, location_repaired] = geometry(Csr::Location, Csr::LocationValid, members.location)?;
    let [observation_space, observation_space_repaired] = geometry(Csr::ObservationSpace, Csr::ObservationSpaceValid, members.observation_space)?;
    let [operation_space, operation_space_repaired] = geometry(Csr::OperationSpace, Csr::OperationSpaceValid, members.operation_space)?;
    let property_document = members.document().map_err(|error| persistence_error::<RegistrationError>(error))?;
    Ok(vec![
        (Csr::RegistrationName, Expr::val(members.registration_name.map(NonEmptyText::as_str))),
        (Csr::Description, Expr::val(members.description.map(NonEmptyText::as_str))),
        (Csr::DatasetId, Expr::val(dataset_selectors(members.dataset_id))),
        (Csr::Tenant, Expr::val(members.tenant.map(TenantName::as_str))),
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
        (Csr::ExpiresAt, Expr::val(members.expires_at)),
        (Csr::Endpoint, Expr::val(members.endpoint.as_str())),
        (Csr::Accept, Expr::val(fields.accept.map(ToString::to_string))),
        (Csr::Mode, Expr::val(mode_code(members.mode))),
        (Csr::Operations, Expr::val(members.operations.and_then(operation_codes))),
        (Csr::ScopePaths, cast(Expr::val(scopes.label_paths), PostgresType::LtreeArray)),
        (Csr::RefreshRate, Expr::val(members.refresh_rate.map(ToString::to_string))),
        (Csr::IsManagementDeclared, Expr::val(management.declared)),
        (Csr::ManagementCacheDuration, Expr::val(management.cache_duration)),
        (Csr::ManagementCooldown, Expr::val(management.cooldown)),
        (Csr::ManagementLocalOnly, Expr::val(management.local_only)),
        (Csr::ManagementTimeout, Expr::val(management.timeout)),
        (Csr::ContextSourceInfoKeys, Expr::val(owned_texts(info_keys))),
        (Csr::ContextSourceInfoValues, Expr::val(owned_texts(info_values))),
        (
            Csr::ContextSourceAlias,
            Expr::val(members.context_source_alias.map(ContextSourcePseudonym::as_str)),
        ),
        (Csr::AdditionalProperties, external.push(stored_properties(members.added))),
        (Csr::ContextSourceProperties, external.push(Json(property_document.into_literal()))),
        (Csr::EntityIdPatternFormsRevision, Expr::val(pattern_forms_revision.map(RenderingRevision::get))),
        (Csr::VerbatimScopePaths, Expr::val(owned_texts(scopes.written))),
    ])
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::csource_registration::registration_columns::{RegistrationFields, registration_columns, stored_properties};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_postgresql_schema::csr::Csr;
    use andromeda_postgresql_seaquery::external_arguments::ExternalArguments;
    use andromeda_registration::{
        resolved_update::apply_fragment,
        store::{create_params::CreateCsrParams, update_fragment::CsrUpdateFragment},
    };
    use indexmap::IndexMap;
    use serde_json::json;

    fn stored() -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "registrationName": "vehicles",
            "information": [{"entities": [{"type": "https://example.org/Vehicle"}]}],
            "scope": ["/Madrid"],
            "location": {"type": "Point", "coordinates": [13.5, 52.5]},
            "observationInterval": {"startAt": "2024-01-01T00:00:00Z"},
            "endpoint": "https://source.example.com/",
            "mode": "exclusive",
            "operations": ["retrieveEntity"],
            "management": {"timeout": 3000},
            "expiresAt": "2100-01-01T00:00:00Z",
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z",
            "https://example.org/tier": "gold"
        }))
        .expect("a registration")
    }

    fn binds_the_document(fields: &RegistrationFields<'_>) -> bool {
        let mut external = ExternalArguments::default();
        registration_columns(fields, None, &mut external)
            .expect("the columns build")
            .iter()
            .any(|(column, _)| matches!(column, Csr::ContextSourceProperties))
    }

    /// Clause 4.9: the document a create or an update stores for discovery is
    /// the one a forwarded read and a subscription build from the registration
    /// as it reads back, so the three readers of `csf` agree.
    #[test]
    fn create_and_update_write_the_property_document() {
        let registration = stored();
        let read = registration.property_members().document().expect("the document builds").into_literal();
        let resolved = apply_fragment(registration, &CsrUpdateFragment::default());
        let updated = RegistrationFields::from(&resolved);
        let written = updated.members.document().expect("the document builds").into_literal();
        pretty_assertions::assert_eq!(written, read);
        assert!(binds_the_document(&updated));

        let params = CreateCsrParams::builder()
            .information(Vec::new())
            .endpoint("https://example.org/csource".parse().expect("a dereferenceable URI"))
            .operations(Vec::new())
            .build();
        assert!(binds_the_document(&RegistrationFields::from(&params)));
    }

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
