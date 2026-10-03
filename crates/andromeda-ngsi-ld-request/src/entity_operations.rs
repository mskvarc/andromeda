//! Query parameter types for batch entity operation endpoints.

use crate::{
    case_insensitive_bool::deserialize_option_bool_case_insensitive,
    error::ParameterError,
    language_filter::deserialize_optional_language_filter,
    options::{EntityRepresentationOptions, OverwritePolicy, UpsertMode},
    pagination::parameters::OffsetPageParameters,
    query::{
        deserialization::{deserialize_optional_dataset_selectors, deserialize_optional_projection_terms},
        entity::parameters::EntityQueryParameters,
    },
    representation_parameter::{WrittenValue, supported_option},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::grammar::RawProjectionTerm};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use serde::Deserialize;
use std::num::NonZeroU32;

/// Query parameters for `POST /entityOperations/upsert`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertParams {
    /// Whether existing entities are replaced or updated.
    pub options: Option<WrittenValue<UpsertMode>>,
}

/// Query parameters for `PATCH /entityOperations/update`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateParams {
    /// Whether existing attributes may be overwritten.
    pub options: Option<WrittenValue<OverwritePolicy>>,
}

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
/// Table 5.2.23-1 gives the `Query` data type no pagination member, no
/// representation member and no local-scope flag, while Table 6.2-1 lists
/// `POST /entityOperations/query` among the HTTP operations of clause 5.7.2 —
/// one of the operations clause 4.12 lists. Clauses 6.3.10 and 6.3.13
/// therefore require `limit`, `offset` and `count` on this binding as much as
/// on the GET one, and they can only reach it through the URL.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostQueryParams {
    /// The page asked for (clause 6.3.10 `limit` and `offset`). The binding
    /// carries no dereferenceable pointer, so it declares no `cursor`.
    #[serde(flatten)]
    pub page: OffsetPageParameters,
    /// Whether the response reports the total number of matches
    /// (clause 6.3.13 `count`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub count: Option<bool>,
    /// Typed representation and system-attribute options (`options`).
    pub options: Option<WrittenValue<EntityRepresentationOptions>>,
    /// The representation the response uses (clause 4.5.1 `format`).
    pub format: Option<WrittenValue<ResponseFormat>>,
    /// Clause-4.15 natural-language selection preferences.
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
    /// How linked entities reach the response (clause 4.5.23 `join`).
    pub join: Option<JoinMode>,
    /// How many links deep the traversal goes (`joinLevel`).
    #[serde(rename = "joinLevel")]
    pub join_level: Option<NonZeroU32>,
    /// The `GeoProperty` a `GeoJSON` response uses as its geometry (clause 6.3.15 `geometryProperty`).
    #[serde(rename = "geometryProperty")]
    pub geometry_property: Option<AttributeName>,
    /// Dataset instances to select, comma-separated (`datasetId`).
    #[serde(rename = "datasetId")]
    #[serde(default, deserialize_with = "deserialize_optional_dataset_selectors")]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// Members the response is narrowed to (clause 4.21 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Members the response drops (clause 4.21 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub omit: Option<Vec<RawProjectionTerm>>,
    /// Whether the operation is confined to this broker (clause 5.10 `local`).
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}

impl PostQueryParams {
    /// Fold the URL parameters over the query the clause 5.2.23 body produced.
    ///
    /// The same parameter may be written in the URL or in the canonical body.
    /// The specification does not say which wins, so this is broker
    /// convention: the URL is the more specific of the two and takes
    /// precedence, but only where it actually carries a value -- an absent
    /// query-string parameter leaves the body's member standing rather than
    /// erasing it.
    ///
    /// The page and `count` are the exception, and nothing is overwritten by
    /// them: Table 5.2.23-1 defines no member for either, so the body
    /// conversion leaves both absent and the URL is the only place clauses
    /// 6.3.10 and 6.3.13 can put them.
    pub fn apply_to(self, query: &mut EntityQueryParameters) {
        query.page = self.page.into();
        query.count = self.count;
        query.options = self.options.or(query.options.take());
        query.local = self.local.or(query.local);
        query.pick = self.pick.or(query.pick.take());
        query.omit = self.omit.or(query.omit.take());
        query.dataset_id = self.dataset_id.or(query.dataset_id.take());
        query.format = self.format.or(query.format.take());
        query.join = self.join.or(query.join);
        query.join_level = self.join_level.or(query.join_level);
        query.geometry_property = self.geometry_property.or(query.geometry_property.take());
        query.lang = self.lang.or(query.lang.take());
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity_operations::PostQueryParams, pagination::parameters::PageParameters, query::entity::parameters::EntityQueryParameters};
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
            }
        );
        pretty_assertions::assert_eq!(query.count, Some(true));
    }

    /// An absent URL parameter leaves the body's member standing.
    #[test]
    fn an_absent_url_parameter_leaves_the_body_member_standing() {
        let mut query = body_query();
        query.local = Some(true);
        let url: PostQueryParams = serde_urlencoded::from_str("").expect("valid URL parameters");
        url.apply_to(&mut query);
        pretty_assertions::assert_eq!(query.local, Some(true));
        pretty_assertions::assert_eq!(query.page, PageParameters::default());
    }
}
