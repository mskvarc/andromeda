//! Query parameter types for batch entity operation endpoints.

use crate::{
    error::ParameterError,
    options::{EntityRepresentationOptions, OverwritePolicy, UpsertMode},
    pagination::parameters::OffsetPageParameters,
    query::entity::parameters::EntityQueryParameters,
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
    representation_parameter::{WrittenValue, supported_option},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde::Deserialize;

/// Query parameters for `POST /entityOperations/upsert`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertParams {
    /// Whether existing entities are replaced or updated.
    pub options: Option<WrittenValue<UpsertMode>>,
    /// Whether the operation is confined to this broker (clause 6.3.18 `local`).
    pub local: Option<bool>,
}

query_parameter_set!(UpsertParams { Options, Local });

/// Query parameters for `PATCH /entityOperations/update`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    /// Whether existing attributes may be overwritten.
    pub options: Option<WrittenValue<OverwritePolicy>>,
    /// Whether the operation is confined to this broker (clause 6.3.18 `local`).
    pub local: Option<bool>,
}

query_parameter_set!(UpdateParams { Options, Local });

impl UpsertParams {
    /// Whether an entity that already exists is replaced or updated.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] for a value clause 6.15.3.1
    /// does not define, which clause 6.3.20 answers with `InvalidRequest`.
    pub fn mode(&self) -> Result<UpsertMode, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }
}

impl UpdateParams {
    /// Whether the update may overwrite an attribute the entity already has.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] for a value other than
    /// `noOverwrite`, which clause 6.3.20 answers with `InvalidRequest`.
    pub fn overwrite_policy(&self) -> Result<OverwritePolicy, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }
}

/// Query parameters for `POST /entityOperations/query`.
///
/// Table 6.23.3.1-1 defines no URL parameter: the query is the clause 5.2.23
/// body. The URL carries only what that body has no member for and a clause
/// 6.3 subclause grants to "POST operations corresponding to Query
/// Entities": `format` and `options` (clauses 6.3.7 and 6.3.11), `limit`,
/// `offset` and `count` (clauses 6.3.10 and 6.3.13, through Table 6.2-1),
/// `geometryProperty` (clause 6.3.15) and `local` (clause 6.3.18). A member the
/// body defines (`pick`, `omit`, `datasetId`, `join`, `joinLevel`, `lang`, ...)
/// has its place in the body, so on the URL it is a parameter the operation
/// does not define (clause 6.3.20).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostQueryParams {
    /// The page asked for (clause 6.3.10 `limit` and `offset`). The binding
    /// carries no dereferenceable pointer, so it declares no `cursor`.
    #[serde(flatten)]
    pub page: OffsetPageParameters,
    /// Whether the response reports the total number of matches
    /// (clause 6.3.13 `count`).
    pub count: Option<bool>,
    /// Typed representation and system-attribute options (`options`).
    pub options: Option<WrittenValue<EntityRepresentationOptions>>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<WrittenValue<ResponseFormat>>,
    /// The `GeoProperty` a `GeoJSON` response uses as its geometry (clause 6.3.15 `geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<AttributeName>,
    /// Whether the operation is confined to this broker (clause 6.3.18 `local`).
    pub local: Option<bool>,
}

query_parameter_set!(
    PostQueryParams {
        Count,
        Options,
        Format,
        GeometryProperty,
        Local
    } + OffsetPageParameters
);

impl PostQueryParams {
    /// Set the URL parameters on the query the clause 5.2.23 body produced.
    ///
    /// Table 5.2.23-1 defines a member for none of them, so the body
    /// conversion leaves each absent and nothing of the body is overwritten.
    pub fn apply_to(self, query: &mut EntityQueryParameters) {
        query.page = self.page.into();
        query.count = self.count;
        query.options = self.options;
        query.format = self.format;
        query.geometry_property = self.geometry_property;
        query.local = self.local;
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        entity_operations::{PostQueryParams, UpdateParams, UpsertParams},
        pagination::parameters::PageParameters,
        query::entity::parameters::EntityQueryParameters,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::query::body::Query;
    use serde_json::json;

    fn body_query() -> EntityQueryParameters {
        let body: Query = serde_json::from_value(json!({"type": "Query", "entities": [{"type": "Vehicle"}]})).expect("a valid query body");
        EntityQueryParameters::try_from(body).expect("a convertible body")
    }

    /// Clause 6.3.10: the POST binding reads its page off the URL, the only
    /// place it can name one, and never a cursor.
    #[test]
    fn apply_url_parameters_moves_the_page() {
        let mut query = body_query();
        let url: PostQueryParams = serde_urlencoded::from_str("limit=3&offset=6&count=true").expect("valid URL parameters");
        url.apply_to(&mut query);
        pretty_assertions::assert_eq!(
            query.page,
            PageParameters {
                limit: Some(3),
                offset: Some(6),
                cursor: None,
                context: None,
            }
        );
        pretty_assertions::assert_eq!(query.count, Some(true));
    }

    /// Clauses 6.3.7, 6.3.15 and 6.3.18 grant `format`, `geometryProperty` and
    /// `local` to the POST binding, and the body has no member for them.
    #[test]
    fn the_granted_url_parameters_reach_the_query() {
        let mut query = body_query();
        let url: PostQueryParams =
            serde_urlencoded::from_str("format=keyValues&options=sysAttrs&geometryProperty=position&local=true").expect("valid URL parameters");
        url.apply_to(&mut query);
        assert!(query.format.is_some() && query.options.is_some());
        pretty_assertions::assert_eq!(query.geometry_property.as_ref().map(AttributeName::as_str), Some("position"));
        pretty_assertions::assert_eq!(query.local, Some(true));
    }

    /// Table 6.23.3.1-1 defines no URL parameter, and each of these has its
    /// member in the Table 5.2.23-1 body (clause 6.3.20).
    #[test]
    fn a_body_member_on_the_url_is_unknown() {
        for parameter in [
            "pick=speed",
            "omit=speed",
            "datasetId=@none",
            "join=flat",
            "joinLevel=2",
            "lang=en",
            "q=speed>1",
            "type=Vehicle",
            "attrs=speed",
            "cursor=abc",
        ] {
            let message = serde_urlencoded::from_str::<PostQueryParams>(parameter).expect_err(parameter).to_string();
            assert!(message.contains("unknown field"), "{parameter}: {message}");
        }
    }

    /// Clause 6.3.18 gives every `/entityOperations/` operation `local`.
    #[test]
    fn the_batch_upsert_and_update_read_local() {
        let upsert: UpsertParams = serde_urlencoded::from_str("options=update&local=true").expect("parses");
        pretty_assertions::assert_eq!(upsert.local, Some(true));
        let update: UpdateParams = serde_urlencoded::from_str("options=noOverwrite&local=true").expect("parses");
        pretty_assertions::assert_eq!(update.local, Some(true));
    }
}
