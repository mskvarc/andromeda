//! A registration advertising `queryBatch` but not `queryEntity` (or a client that queried via POST) is contacted through the batch query endpoint (clause 6.23). The query is the typed clause 5.2.23 [`Query`], built from the parameters the narrowing produced, which are already spelled in the registration's forwarding context (clause 4.3.6.6).

use crate::{error::FederationError, federation_invariant::FederationInvariant, forwarded_parameter::ForwardedParameter, forwarded_query::ForwardedQuery};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::query::{body::Query, entity_selector::QueryEntitySelector};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;

/// The clause 5.2.23 `Query` of a forwarded batch query: one entity selector
/// per covered identifier (each carrying the covered type), or a single
/// type-only selector when the query names no identifier, and the narrowed
/// attribute list when present. A query naming no type selects every type
/// (clause 4.17 wildcard), since a selector's type is mandatory (Table
/// 5.2.33-1).
///
/// # Errors
///
/// Returns [`FederationError::Query`] when the narrowed type selection does not
/// parse, and [`FederationInvariant::ForwardedName`] or
/// [`FederationInvariant::ForwardedIdentifier`] when a narrowed attribute or
/// identifier is none, which the narrowing never produces.
pub(crate) fn batch_query(query: &ForwardedQuery) -> Result<Query, FederationError> {
    let entity_type = match query.value_of(ForwardedParameter::Type) {
        Some(selection) => selection.parse::<TypeExprRaw>()?,
        None => TypeExprRaw::Wildcard,
    };
    let selectors = match query.value_of(ForwardedParameter::Id) {
        Some(ids) => ids
            .split(',')
            .map(|id| {
                let id = id.parse::<Identifier>().map_err(FederationInvariant::ForwardedIdentifier)?;
                Ok(QueryEntitySelector {
                    id: Some(vec![id]),
                    id_pattern: None,
                    type_: entity_type.clone(), // clone: one selector per identifier names the same type
                })
            })
            .collect::<Result<Vec<_>, FederationError>>()?,
        None => vec![QueryEntitySelector {
            id: None,
            id_pattern: None,
            type_: entity_type,
        }],
    };
    let attrs = match query.value_of(ForwardedParameter::Attrs) {
        Some(attrs) => Some(
            attrs
                .split(',')
                .map(|attr| {
                    attr.parse::<AttributeName>()
                        .map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))
                })
                .collect::<Result<Vec<_>, FederationError>>()?,
        ),
        None => None,
    };
    Ok(Query::builder().entities(Some(selectors)).attrs(attrs).build())
}

#[cfg(test)]
mod tests {
    use crate::{
        forwarded_batch_query::batch_query,
        forwarded_names::ForwardedNames,
        forwarded_query::{ForwardedQuery, client_query::ClientQuery},
        forwarding_context::Rewording,
        request::TemporalAnswer,
        split_entities::SplitEntities,
        term_resolver::TermResolver,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, entity_selector::EntitySelectorParams};
    use serde_json::{Value, json};

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": information,
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    /// The `Query` written for a forwarded query of `query_string` narrowed by
    /// a registration of `information`.
    async fn written(information: &Value, query_string: &str) -> Value {
        let by_type = registration(information);
        let params: EntityQueryParameters = serde_urlencoded::from_str(query_string).expect("valid query");
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let names = ForwardedNames::prepare(&engine, &resolver, &context, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        let forwarded = ForwardedQuery::for_registration(
            ClientQuery::EntityQuery(&params),
            &by_type,
            SplitEntities::Complete,
            None,
            TemporalAnswer::AsRequested,
            &names,
        )
        .await
        .expect("narrowing succeeds");
        let query = batch_query(forwarded.first().expect("covered")).expect("the query builds");
        serde_json::to_value(query).expect("the query writes")
    }

    /// Clause 6.23 requires the first entity selector to carry both the type and the id, so a narrowed id+type query builds exactly that.
    #[tokio::test]
    async fn a_narrowed_id_and_type_query_builds_a_single_selector_with_both() {
        let value = written(
            &json!([{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}]),
            "type=Vehicle&id=urn:ngsi-ld:Vehicle:A1&attrs=speed",
        )
        .await;
        pretty_assertions::assert_eq!(value["type"], "Query");
        pretty_assertions::assert_eq!(value["entities"][0]["type"], "Vehicle");
        pretty_assertions::assert_eq!(value["entities"][0]["id"], "urn:ngsi-ld:Vehicle:A1");
        pretty_assertions::assert_eq!(value["attrs"], json!(["speed"]));
    }

    #[tokio::test]
    async fn a_type_only_query_builds_a_type_only_selector_and_no_attrs() {
        let value = written(
            &json!([{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}]),
            "type=Vehicle",
        )
        .await;
        pretty_assertions::assert_eq!(value["entities"][0]["type"], "Vehicle");
        assert!(value["entities"][0].get("id").is_none());
        assert!(value.get("attrs").is_none());
    }

    /// Several identifiers become one selector each, carrying the same type.
    #[tokio::test]
    async fn several_identifiers_build_one_selector_each() {
        let value = written(
            &json!([{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}]),
            "type=Vehicle&id=urn:ngsi-ld:Vehicle:1,urn:ngsi-ld:Vehicle:2",
        )
        .await;
        pretty_assertions::assert_eq!(value["entities"].as_array().map(Vec::len), Some(2));
        pretty_assertions::assert_eq!(value["entities"][1]["id"], "urn:ngsi-ld:Vehicle:2");
        pretty_assertions::assert_eq!(value["entities"][1]["type"], "Vehicle");
    }

    /// Clause 5.2.33: the body sent to a source holds only the selector of the
    /// client's union that the source's block covers.
    #[tokio::test]
    async fn a_union_query_writes_only_the_selector_the_block_covers() {
        let by_type = registration(&json!([{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Beta"}]}]));
        let mut params: EntityQueryParameters = serde_urlencoded::from_str("").expect("valid query");
        params.entity_selectors = ["Alpha", "Beta"]
            .into_iter()
            .map(|raw| EntitySelectorParams {
                type_: Some(raw.parse().expect("valid type expression")),
                ..EntitySelectorParams::default()
            })
            .collect();
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let names = ForwardedNames::prepare(&engine, &resolver, &context, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        let forwarded = ForwardedQuery::for_registration(
            ClientQuery::EntityQuery(&params),
            &by_type,
            SplitEntities::Complete,
            None,
            TemporalAnswer::AsRequested,
            &names,
        )
        .await
        .expect("narrowing succeeds");
        pretty_assertions::assert_eq!(forwarded.len(), 1);
        let value = serde_json::to_value(batch_query(&forwarded[0]).expect("the query builds")).expect("the query writes");
        pretty_assertions::assert_eq!(value["entities"].as_array().map(Vec::len), Some(1));
        pretty_assertions::assert_eq!(value["entities"][0]["type"], "Beta");
    }
}
