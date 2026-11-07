//! Expansion of a subscription request's terms against the request `@context`
//! (ETSI GS CIM 009 clause 5.5.7).
//!
//! Everything a subscription is later matched by is stored in expanded form —
//! entity types, watched attributes, the notification's attribute lists, the
//! `q` and `csf` filters, the `geoQ` geoproperty — because matching runs
//! against expanded entity data and never has the creator's `@context` at hand.
//! Context hosting is not handled here; the lifecycle adds that on top.

use crate::{error::SubscriptionError, filter_opt_ins::FilterOptIns, projection_expansion::expand_notification_projection};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference};
use andromeda_ngsi_ld_model::{
    clearable::Clearable,
    notification_request::NotificationRequest,
    subscription::entity_selector::EntitySelector,
    subscription_request::SubscriptionRequest,
};
use andromeda_ngsi_ld_query::{
    error::QueryError,
    filter::expansion::{expand_attribute_names, expand_csf_expr},
    term_expander::TermExpander,
    type_selection::{expansion::expand_type_expression, grammar::TypeExpr},
};
use std::mem;

/// Expand every name of one attribute-name list in place (clause 5.5.7).
async fn expand_name_list(names: &mut Vec<AttributeName>, expander: &impl TermExpander) -> Result<(), SubscriptionError> {
    *names = expand_attribute_names(names, expander).await?;
    Ok(())
}

/// Expand the type selection of every entity selector in place.
async fn expand_selector_types(selectors: &mut [EntitySelector], expander: &impl TermExpander) -> Result<(), SubscriptionError> {
    for selector in selectors {
        selector.type_ = expand_type_expression(&selector.type_, expander).await.and_then(TypeExpr::into_raw)?;
    }
    Ok(())
}

/// Expand the attribute list and the projection lists of a notification.
async fn expand_notification(notification: &mut NotificationRequest, expander: &impl TermExpander) -> Result<(), SubscriptionError> {
    if let Some(names) = notification.attributes.as_mut() {
        expand_name_list(names, expander).await?;
    }
    expand_notification_projection(notification.pick.as_deref_mut(), notification.omit.as_deref_mut(), expander).await
}

/// Expand the geoproperty a `geoQ` names.
async fn expand_geoproperty(geoproperty: &mut NgsiLdReference, expander: &impl TermExpander) -> Result<(), SubscriptionError> {
    let iri = expander.expand_term(geoproperty.as_ref()).await?;
    *geoproperty = NgsiLdReference::try_from(iri.to_string()).map_err(QueryError::AttributeNameInvalid)?;
    Ok(())
}

/// Expand entities, watchedAttributes, the notification's attributes, pick and
/// omit, the filters, the opt-in lists and geoQ of a subscription request in
/// place. Context hosting is NOT handled here — the lifecycle adds that on top.
///
/// The `q` the request holds afterwards is the expanded filter itself, which
/// also keeps the spelling the subscriber wrote of its trailing `[…]` members,
/// where clause 4.9 reads a `LanguageProperty`'s target language and a raw JSON
/// key.
///
/// # Errors
///
/// Returns [`SubscriptionError::Query`] when the `@context` cannot be resolved
/// or when any term the request names (an entity type, a watched or notified
/// attribute, a projection or opt-in name, a filter term, the `geoproperty`)
/// does not expand to a usable name. Such a term is refused rather than kept
/// as written, since matching runs only against expanded names and the
/// subscription would never match on it.
pub(crate) async fn expand_common_subscription_terms(
    engine: &JsonLdEngine,
    request: &mut SubscriptionRequest,
    context: &ContextSpec,
) -> Result<(), SubscriptionError> {
    let prepared_context = engine.prepare_term_context(context);
    let term_expander = ContextualExpander::new(engine, &prepared_context);

    if let Some(entities) = request.entities.as_set_mut() {
        expand_selector_types(entities, &term_expander).await?;
    }
    if let Some(watched) = request.watched_attributes.as_set_mut() {
        expand_name_list(watched, &term_expander).await?;
    }
    if let Some(notification) = request.notification.as_mut() {
        expand_notification(notification, &term_expander).await?;
    }

    let opt_ins = FilterOptIns::expanded(&mut request.expand_values, &mut request.json_keys, &term_expander).await?;
    request.q = match mem::take(&mut request.q) {
        Clearable::Set(q) => Clearable::Set(opt_ins.expand_q(q, &term_expander).await?),
        unchanged @ (Clearable::Absent | Clearable::NgsiLdNull) => unchanged,
    };

    // Expand the context source filter's property terms: a registration's
    // Context Source Properties are stored under their expanded names (clause
    // 5.5.7), so the filter matched against them (clauses 5.8.1.4 and
    // 5.11.2.4) has to name them by IRI too. Their values are raw JSON, so the
    // compound-value members stay as written (clause 4.9).
    request.csf = match mem::take(&mut request.csf) {
        Clearable::Set(csf) => Clearable::Set(expand_csf_expr(csf, &term_expander).await?),
        unchanged @ (Clearable::Absent | Clearable::NgsiLdNull) => unchanged,
    };

    if let Some(geo_q) = request.geo_q.as_set_mut()
        && let Some(geoproperty) = geo_q.geoproperty.as_mut()
    {
        expand_geoproperty(geoproperty, &term_expander).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{error::SubscriptionError, term_expansion::expand_common_subscription_terms};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::{request_body::RequestBody, subscription_request::SubscriptionRequest};
    use andromeda_ngsi_ld_query::{error::QueryError, filter::serialize::serialize_written_members, term_expander::TermExpansionFailure};
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use serde_json::{Value, json};

    /// The core context's default vocabulary.
    const DEFAULT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The request body `wire` writes, read as the request model reads it.
    fn request(wire: &Value) -> SubscriptionRequest {
        SubscriptionRequest::from_request_text(wire.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a readable request")
    }

    /// `wire` expanded under the core context, written back as a body.
    async fn expanded(wire: Value) -> Result<SubscriptionRequest, SubscriptionError> {
        let mut request = request(&wire);
        expand_common_subscription_terms(&offline_engine(), &mut request, &ContextSpec::none()).await?;
        Ok(request)
    }

    /// `wire` expanded under a body `@context` that maps `speed`, `Vehicle`
    /// and `place` to nothing, so none of them names an IRI.
    async fn expanded_under_null_mappings(wire: Value) -> Result<SubscriptionRequest, SubscriptionError> {
        let mut request = request(&wire);
        let context = JsonLiteral::admit(&json!({"speed": null, "Vehicle": null, "place": null}).to_string()).expect("an admissible literal");
        expand_common_subscription_terms(&offline_engine(), &mut request, &ContextSpec::from_body(context)).await?;
        Ok(request)
    }

    /// Whether `result` is the refusal of a term that expands to nothing.
    fn is_not_expanded(result: &Result<SubscriptionRequest, SubscriptionError>) -> bool {
        matches!(
            result,
            Err(SubscriptionError::Query(QueryError::TermExpansion(TermExpansionFailure::NotExpanded)))
        )
    }

    /// The expanded request as the wire form a client would read it in.
    fn written(request: &SubscriptionRequest) -> Value {
        serde_json::to_value(request).expect("the request writes")
    }

    /// Clause 5.5.7 for the clause 5.2.14 lists: `attributes` expands its
    /// names; `pick` and `omit` expand every attribute name at every nesting
    /// level into the stored term tree, and the clause 4.21 keywords stay as
    /// written.
    #[tokio::test]
    async fn nested_projection_terms_expand_every_name_and_keep_keywords() {
        let request = expanded(json!({
            "notification": {
                "endpoint": {"uri": "https://receiver.example.com/notify"},
                "attributes": ["speed"],
                "pick": ["id", "owner{id,name}"],
                "omit": ["scope,colour"]
            }
        }))
        .await
        .expect("expands");

        let notification = request.notification.expect("the notification is kept");
        pretty_assertions::assert_eq!(written_names(notification.attributes.as_deref()), vec![format!("{DEFAULT}speed")]);
        pretty_assertions::assert_eq!(
            serde_json::to_value(notification.pick).expect("the tree writes"),
            json!([
                [{"name": "id", "linked": []}],
                [{"name": format!("{DEFAULT}owner"), "linked": [{"name": "id", "linked": []}, {"name": format!("{DEFAULT}name"), "linked": []}]}]
            ])
        );
        pretty_assertions::assert_eq!(
            serde_json::to_value(notification.omit).expect("the tree writes"),
            json!([[{"name": "scope", "linked": []}, {"name": format!("{DEFAULT}colour"), "linked": []}]])
        );
    }

    /// The names of an expanded list, as text.
    fn written_names(names: Option<&[AttributeName]>) -> Vec<String> {
        names.unwrap_or_default().iter().map(|name| name.as_str().to_owned()).collect()
    }

    /// Table 5.2.12-1: `expandValues` is stored as the same comma-separated
    /// String it arrived as, with every name expanded so it keys the expanded
    /// `q` the store holds.
    #[tokio::test]
    async fn the_opt_in_lists_are_stored_expanded_in_their_comma_separated_form() {
        let request = expanded(json!({"q": "speed>10", "expandValues": "speed,colour", "jsonKeys": "payload"}))
            .await
            .expect("expands");

        let written = written(&request);
        pretty_assertions::assert_eq!(
            written["expandValues"],
            json!("https://uri.etsi.org/ngsi-ld/default-context/speed,https://uri.etsi.org/ngsi-ld/default-context/colour")
        );
        pretty_assertions::assert_eq!(written["jsonKeys"], json!("https://uri.etsi.org/ngsi-ld/default-context/payload"));
    }

    /// Clause 5.5.8: an opt-in list the fragment deletes stays deleted through
    /// the expansion.
    #[tokio::test]
    async fn a_cleared_opt_in_list_stays_cleared() {
        let request = expanded(json!({"expandValues": "urn:ngsi-ld:null"})).await.expect("expands");

        pretty_assertions::assert_eq!(written(&request), json!({"expandValues": "urn:ngsi-ld:null"}));
    }

    /// Table 5.2.12-1: the identified attributes' values "should be expanded
    /// against the supplied @context using JSON-LD type coercion prior to
    /// executing the query", so the stored filter is the coerced one.
    #[tokio::test]
    async fn expand_values_rewrites_the_stored_filter_operand() {
        let request = expanded(json!({"q": "colour==\"active\"", "expandValues": "colour"})).await.expect("expands");

        pretty_assertions::assert_eq!(
            written(&request)["q"],
            json!("\"https://uri.etsi.org/ngsi-ld/default-context/colour\"==\"https://uri.etsi.org/ngsi-ld/default-context/active\"")
        );
    }

    /// The same filter without the opt-in keeps its literal operand, which is
    /// what makes the member observable at all.
    #[tokio::test]
    async fn a_filter_without_the_opt_in_keeps_its_literal_operand() {
        let request = expanded(json!({"q": "colour==\"active\""})).await.expect("expands");

        pretty_assertions::assert_eq!(
            written(&request)["q"],
            json!("\"https://uri.etsi.org/ngsi-ld/default-context/colour\"==\"active\"")
        );
    }

    /// Table 5.2.12-1: the values of a `jsonKeys` attribute are
    /// "uninterpretable as JSON-LD", so the object keys the filter addresses
    /// under them stay literal, while the attribute itself still expands.
    ///
    /// The path has to be the bracketed form of clause 4.9. A dotted path names
    /// sub-attributes, which are NGSI-LD terms and expand whatever `jsonKeys`
    /// says; only the `[...]` form reaches inside a value, and clause 4.9
    /// EXAMPLE 11 is exactly that case -- "the target value raw is JSON, and is
    /// not expanded to ngsi-ld:hasValue using the core @context".
    #[tokio::test]
    async fn json_keys_leaves_the_addressed_object_keys_literal() {
        let declared = expanded(json!({"q": "payload[city]==\"Berlin\"", "jsonKeys": "payload"}))
            .await
            .expect("expands");

        let written = written(&declared);
        let rendered = written["q"].as_str().expect("the filter is a string");
        assert!(
            rendered.contains("https://uri.etsi.org/ngsi-ld/default-context/payload"),
            "the addressed attribute still expands: {rendered}"
        );
        assert!(
            !rendered.contains("https://uri.etsi.org/ngsi-ld/default-context/city"),
            "the object key under a jsonKeys attribute stays literal: {rendered}"
        );
        assert!(rendered.contains("\"Berlin\""), "the operand is left literal: {rendered}");
    }

    /// The control for the test above: the same bracketed path without the
    /// declaration has its member name expanded, which is what makes the
    /// declaration observable at all.
    #[tokio::test]
    async fn the_same_bracketed_key_expands_without_the_json_keys_declaration() {
        let undeclared = expanded(json!({"q": "payload[city]==\"Berlin\""})).await.expect("expands");

        let written = written(&undeclared);
        let rendered = written["q"].as_str().expect("the filter is a string");
        assert!(
            rendered.contains("https://uri.etsi.org/ngsi-ld/default-context/city"),
            "an undeclared attribute's member name expands: {rendered}"
        );
    }

    /// Clause 5.5.8: a member the fragment deletes has no terms to expand, and
    /// is still a deletion once the others are.
    #[tokio::test]
    async fn a_deleted_member_has_nothing_to_expand_and_stays_deleted() {
        let wire = json!({"entities": ["urn:ngsi-ld:null"], "watchedAttributes": ["urn:ngsi-ld:null"], "q": "urn:ngsi-ld:null", "csf": "urn:ngsi-ld:null", "geoQ": "urn:ngsi-ld:null"});
        let request = expanded(wire.clone()).await.expect("expands");

        pretty_assertions::assert_eq!(written(&request), wire);
    }

    /// Clause 4.9 reads `name[en]` on a `LanguageProperty` as the target
    /// language `en`: the filter the request holds keeps that spelling beside
    /// the expanded member the text form carries.
    #[tokio::test]
    async fn the_expanded_q_keeps_the_written_trailing_members() {
        let request = expanded(json!({"q": "name[en]==\"Door\""})).await.expect("expands");
        let q = request.q.as_set().expect("the request names a q");

        pretty_assertions::assert_eq!(
            written(&request)["q"],
            json!("\"https://uri.etsi.org/ngsi-ld/default-context/name\"[\"https://uri.etsi.org/ngsi-ld/default-context/en\"]==\"Door\"")
        );
        pretty_assertions::assert_eq!(
            serialize_written_members(q),
            "\"https://uri.etsi.org/ngsi-ld/default-context/name\"[en]==\"Door\""
        );
    }

    /// Clause 4.9: a `csf` targets Context Source Properties, whose values are
    /// the raw JSON a registrant wrote, so the stored filter names the property
    /// by IRI and keeps the member as the literal key it addresses.
    #[tokio::test]
    async fn the_expanded_csf_keeps_its_trailing_members_verbatim() {
        let request = expanded(json!({"csf": "coverage[region]==\"north\""})).await.expect("expands");

        pretty_assertions::assert_eq!(
            written(&request)["csf"],
            json!("\"https://uri.etsi.org/ngsi-ld/default-context/coverage\"[region]==\"north\"")
        );
    }

    /// Clause 5.5.7: the `geoproperty` a `geoQ` names expands like every other
    /// term.
    #[tokio::test]
    async fn the_geoproperty_of_a_geo_q_is_expanded() {
        let request = expanded(json!({"geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1, 2], "geoproperty": "observationSpace"}}))
            .await
            .expect("expands");

        // `observationSpace` is a term of the core context, so it expands to the
        // NGSI-LD vocabulary rather than the default one.
        pretty_assertions::assert_eq!(written(&request)["geoQ"]["geoproperty"], json!("https://uri.etsi.org/ngsi-ld/observationSpace"));
    }

    /// Clause 5.5.7: the type selection of an entity selector expands.
    #[tokio::test]
    async fn an_entity_selector_type_expands_through_the_context() {
        let request = expanded(json!({"entities": [{"type": "Vehicle"}], "watchedAttributes": ["speed"]}))
            .await
            .expect("expands");

        let written = written(&request);
        pretty_assertions::assert_eq!(written["entities"], json!([{"type": format!("{DEFAULT}Vehicle")}]));
        pretty_assertions::assert_eq!(written["watchedAttributes"], json!([format!("{DEFAULT}speed")]));
    }

    /// Clause 5.5.7: matching reads only expanded names, so a term the
    /// `@context` maps to nothing would leave a member that never matches.
    #[tokio::test]
    async fn an_unexpandable_term_in_each_member_is_refused() {
        let cases = [
            json!({"entities": [{"type": "Vehicle"}]}),
            json!({"watchedAttributes": ["speed"]}),
            json!({"notification": {"attributes": ["speed"], "endpoint": {"uri": "http://example.com/notify"}}}),
            json!({"notification": {"pick": ["speed"], "endpoint": {"uri": "http://example.com/notify"}}}),
            json!({"q": "speed>10"}),
            json!({"geoQ": {"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": [1, 2], "geoproperty": "place"}}),
        ];
        for wire in cases {
            let result = expanded_under_null_mappings(wire.clone()).await;
            assert!(is_not_expanded(&result), "{wire} gave {result:?}");
        }
    }

    /// The control for the test above: the same members expand when the
    /// context maps their terms.
    #[tokio::test]
    async fn the_same_members_expand_under_the_core_context() {
        let request = expanded(json!({"watchedAttributes": ["speed"], "geoQ": {"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": [1, 2], "geoproperty": "place"}}))
            .await
            .expect("expands");

        let written = written(&request);
        pretty_assertions::assert_eq!(written["watchedAttributes"], json!([format!("{DEFAULT}speed")]));
        pretty_assertions::assert_eq!(written["geoQ"]["geoproperty"], json!(format!("{DEFAULT}place")));
    }
}
