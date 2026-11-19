//! Expansion of a subscription request's terms against the request `@context`
//! (ETSI GS CIM 009 clause 5.5.7).
//!
//! Everything a subscription is later matched by is stored in expanded form
//! (entity types, watched attributes, the notification's attribute lists, the
//! `q` and `csf` filters, the `geoQ` geoproperty), because matching runs
//! against expanded entity data and never has the creator's `@context` at hand.
//! The request stays as written and the expanded stage is a value of its own,
//! [`ExpandedTerms`]. Context hosting is not handled here; the lifecycle adds
//! that on top.

use crate::{
    error::SubscriptionError,
    expanded_terms::ExpandedTerms,
    filter_opt_ins::expanded_opt_ins,
    projection_expansion::expand_notification_projection,
    subscription_invariant::SubscriptionInvariant,
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName};
use andromeda_ngsi_ld_model::{
    clearable::Clearable,
    entity_selector::EntitySelector,
    expanded_notification_terms::ExpandedNotificationTerms,
    notification_request::NotificationRequest,
    subscription_request::SubscriptionRequest,
};
use andromeda_ngsi_ld_query::{
    filter::{
        compilation::{AdmittedFilter, compile_q_filter},
        expansion::{expand_attribute_names, expand_context_source_filter},
    },
    geo::expansion::expand_geo_constraint,
    name_stage::{Expanded, Written},
    query_error::QueryError,
    term_expander::TermExpander,
    type_selection::expansion::expand_type_selection,
};

/// Expand the type of every entity selector (clause 5.5.7). The expanded type
/// is held within the identifier bound a stored name keeps.
async fn expand_selectors(selectors: &[EntitySelector<Written>], expander: &impl TermExpander) -> Result<Vec<EntitySelector<Expanded>>, SubscriptionError> {
    let mut rewritten = Vec::with_capacity(selectors.len());
    for selector in selectors {
        let type_ = expand_type_selection(&selector.type_, expander).await?;
        type_.check_written_bound().map_err(QueryError::from)?;
        rewritten.push(selector.with_type(type_));
    }
    Ok(rewritten)
}

/// Expand a list of attribute names, as the request writes its member.
pub(crate) async fn expand_names(
    names: &Clearable<Vec<AttributeName>>,
    expander: &impl TermExpander,
) -> Result<Clearable<Vec<AttributeIri>>, SubscriptionError> {
    Ok(match names {
        Clearable::Set(names) => Clearable::Set(expand_attribute_names(names, expander).await?),
        Clearable::Absent => Clearable::Absent,
        Clearable::NgsiLdNull => Clearable::NgsiLdNull,
    })
}

/// Expand the attribute list and the projection lists of a notification.
async fn expand_notification(notification: &NotificationRequest, expander: &impl TermExpander) -> Result<ExpandedNotificationTerms, SubscriptionError> {
    let attributes = match notification.attributes.as_deref() {
        Some(names) => Some(expand_attribute_names(names, expander).await?),
        None => None,
    };
    let (pick, omit) = expand_notification_projection(notification.pick.as_deref(), notification.omit.as_deref(), expander).await?;
    Ok(ExpandedNotificationTerms { attributes, pick, omit })
}

/// Expand the terms of `request` through `expander`, its `q` compiled from
/// `admitted_q`.
///
/// `admitted_q` is the admission the request's `q` received at validation. The
/// expanded `q` also keeps the spelling the subscriber wrote of its trailing
/// `[…]` members, where clause 4.9 reads a `LanguageProperty`'s target language
/// and a raw JSON key.
///
/// # Errors
///
/// Returns [`SubscriptionError::Query`] when a term does not expand or expands
/// past what a stored name keeps, and [`SubscriptionError::Internal`] when `q`
/// and its admission do not come together. A term that does not expand is
/// refused rather than kept as written, since matching runs only against
/// expanded names and the subscription would never match on it.
pub(crate) async fn expand_request_terms(
    request: &SubscriptionRequest,
    expander: &impl TermExpander,
    admitted_q: Option<&AdmittedFilter>,
) -> Result<ExpandedTerms, SubscriptionError> {
    let entities = match &request.entities {
        Clearable::Set(selectors) => Clearable::Set(expand_selectors(selectors, expander).await?),
        Clearable::Absent => Clearable::Absent,
        Clearable::NgsiLdNull => Clearable::NgsiLdNull,
    };
    let watched_attributes = expand_names(&request.watched_attributes, expander).await?;
    let notification = match request.notification.as_ref() {
        Some(notification) => expand_notification(notification, expander).await?,
        None => ExpandedNotificationTerms::default(),
    };
    let (expand_values, json_keys) = expanded_opt_ins(&request.expand_values, &request.json_keys, expander).await?;
    let mut terms = ExpandedTerms {
        entities,
        watched_attributes,
        notification,
        expand_values,
        json_keys,
        ..ExpandedTerms::default()
    };
    let opt_ins = terms.filter_opt_ins();
    terms.q = match (&request.q, admitted_q) {
        (Clearable::Set(_), Some(admitted)) => Clearable::Set(compile_q_filter(admitted, &opt_ins, expander).await?),
        (Clearable::Absent, None) => Clearable::Absent,
        (Clearable::NgsiLdNull, None) => Clearable::NgsiLdNull,
        (Clearable::Set(_), None) | (Clearable::Absent | Clearable::NgsiLdNull, Some(_)) => return Err(SubscriptionInvariant::QNotAdmitted.into()),
    };
    terms.csf = match &request.csf {
        // A registration's Context Source Properties are stored under their
        // expanded names (clause 5.5.7), so the filter matched against them
        // (clauses 5.8.1.4 and 5.11.2.4) names them by IRI too. Their values
        // are raw JSON, so the compound-value members stay as written (clause
        // 4.9).
        // clone: expansion consumes its tree and the request keeps its filter as written
        Clearable::Set(csf) => Clearable::Set(expand_context_source_filter(csf.clone(), expander).await?),
        Clearable::Absent => Clearable::Absent,
        Clearable::NgsiLdNull => Clearable::NgsiLdNull,
    };
    terms.geo_q = match &request.geo_q {
        Clearable::Set(geo_q) => Clearable::Set(expand_geo_constraint(geo_q, expander).await?),
        Clearable::Absent => Clearable::Absent,
        Clearable::NgsiLdNull => Clearable::NgsiLdNull,
    };
    Ok(terms)
}

/// Expand the terms of `request` under `context`, the way [`expand_request_terms`] describes.
///
/// # Errors
///
/// As [`expand_request_terms`].
pub(crate) async fn expand_common_subscription_terms(
    engine: &JsonLdEngine,
    request: &SubscriptionRequest,
    context: &ContextSpec,
    admitted_q: Option<&AdmittedFilter>,
) -> Result<ExpandedTerms, SubscriptionError> {
    let prepared_context = engine.prepare_term_context(context);
    expand_request_terms(request, &ContextualExpander::new(engine, &prepared_context), admitted_q).await
}

#[cfg(test)]
mod tests {
    use crate::{
        admission_fixture::broker_defaults,
        error::SubscriptionError,
        expanded_terms::ExpandedTerms,
        request_validation::admit_q,
        subscription_invariant::SubscriptionInvariant,
        term_expansion::expand_common_subscription_terms,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, identifier_length::IDENTIFIER_BYTE_LIMIT, reference::NgsiLdReferenceError};
    use andromeda_ngsi_ld_model::{attribute_iri_csv, request_body::RequestBody, subscription_request::SubscriptionRequest};
    use andromeda_ngsi_ld_query::{
        filter::language_targeting::LanguageTargeting,
        query_error::QueryError,
        term_expander::TermExpansionError,
        type_selection::type_selection_error::TypeSelectionError,
    };
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use serde_json::{Value, json};

    /// The core context's default vocabulary.
    const DEFAULT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The request body `wire` reads as.
    fn request(wire: &Value) -> SubscriptionRequest {
        SubscriptionRequest::from_request_text(wire.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a readable request")
    }

    /// `wire` admitted and expanded under `context`.
    async fn expanded_under(wire: Value, context: &ContextSpec) -> Result<ExpandedTerms, SubscriptionError> {
        let request = request(&wire);
        let admitted = admit_q(&request, broker_defaults(LanguageTargeting::LanguageRanges))?;
        expand_common_subscription_terms(&offline_engine(), &request, context, admitted.as_ref()).await
    }

    /// The text of the expanded `q`.
    fn expanded_q(terms: &ExpandedTerms) -> Value {
        json!(terms.q.as_set().expect("the request names a q").to_string())
    }

    /// The text of the expanded `csf`.
    fn expanded_csf(terms: &ExpandedTerms) -> Value {
        json!(terms.csf.as_set().expect("the request names a csf").to_string())
    }

    /// `wire` expanded under the core context.
    async fn expanded(wire: Value) -> Result<ExpandedTerms, SubscriptionError> {
        expanded_under(wire, &ContextSpec::none()).await
    }

    /// `wire` expanded under a body `@context` that maps `speed`, `Vehicle`
    /// and `place` to nothing, so none of them names an IRI.
    async fn expanded_under_null_mappings(wire: Value) -> Result<ExpandedTerms, SubscriptionError> {
        let context = JsonLiteral::admit(&json!({"speed": null, "Vehicle": null, "place": null}).to_string()).expect("an admissible literal");
        expanded_under(wire, &ContextSpec::from_body(context)).await
    }

    /// Whether `result` is the refusal of a term that expands to nothing.
    fn is_not_expanded(result: &Result<ExpandedTerms, SubscriptionError>) -> bool {
        matches!(
            result,
            Err(SubscriptionError::Query(QueryError::TermExpansion(TermExpansionError::NotExpanded)))
        )
    }

    /// The names of an expanded list, as text.
    fn spelled(names: Option<&Vec<AttributeIri>>) -> Vec<String> {
        names.into_iter().flatten().map(|name| name.as_str().to_owned()).collect()
    }

    /// Clause 5.5.7 for the clause 5.2.14 lists: `attributes` expands its
    /// names; `pick` and `omit` expand every attribute name at every nesting
    /// level into the stored term tree, and the clause 4.21 keywords stay as
    /// written.
    #[tokio::test]
    async fn nested_projection_terms_expand_every_name_and_keep_keywords() {
        let terms = expanded(json!({
            "notification": {
                "endpoint": {"uri": "https://receiver.example.com/notify"},
                "attributes": ["speed"],
                "pick": ["id", "owner{id,name}"],
                "omit": ["scope,colour"]
            }
        }))
        .await
        .expect("expands");

        let notification = terms.notification;
        pretty_assertions::assert_eq!(spelled(notification.attributes.as_ref()), vec![format!("{DEFAULT}speed")]);
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

    /// Clause 4.6.2 bounds a stored name, and a `@context` may map a type term to an IRI past the bound, so the
    /// subscription that selects such a type is refused when it is expanded.
    #[tokio::test]
    async fn an_entity_type_expanded_past_the_identifier_bound_is_refused() {
        let long_iri = format!("https://example.com/{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let context = JsonLiteral::admit(&json!({"Vehicle": long_iri}).to_string()).expect("an admissible literal");

        let refused = expanded_under(json!({"entities": [{"type": "Vehicle"}]}), &ContextSpec::from_body(context)).await;

        assert!(
            matches!(
                refused,
                Err(SubscriptionError::Query(QueryError::TypeSelection(TypeSelectionError::InvalidName {
                    source: NgsiLdReferenceError::TooLong(_),
                    ..
                })))
            ),
            "{refused:?}"
        );
    }

    /// Table 5.2.12-1: `expandValues` is stored as the same comma-separated
    /// String it arrived as, with every name expanded so it keys the expanded
    /// `q` the store holds.
    #[tokio::test]
    async fn the_opt_in_lists_are_stored_expanded_in_their_comma_separated_form() {
        let terms = expanded(json!({"q": "speed>10", "expandValues": "speed,colour", "jsonKeys": "payload"}))
            .await
            .expect("expands");

        pretty_assertions::assert_eq!(
            attribute_iri_csv::joined(terms.expand_values.as_set().expect("the request names expandValues")),
            format!("{DEFAULT}speed,{DEFAULT}colour")
        );
        pretty_assertions::assert_eq!(
            attribute_iri_csv::joined(terms.json_keys.as_set().expect("the request names jsonKeys")),
            format!("{DEFAULT}payload")
        );
    }

    /// Clause 5.5.8: an opt-in list the fragment deletes stays deleted through
    /// the expansion.
    #[tokio::test]
    async fn a_cleared_opt_in_list_stays_cleared() {
        let terms = expanded(json!({"expandValues": "urn:ngsi-ld:null"})).await.expect("expands");

        assert!(terms.expand_values.is_ngsi_ld_null() && terms.json_keys.is_absent());
    }

    /// Table 5.2.12-1: the identified attributes' values "should be expanded
    /// against the supplied @context using JSON-LD type coercion prior to
    /// executing the query", so the stored filter is the coerced one.
    #[tokio::test]
    async fn expand_values_rewrites_the_stored_filter_operand() {
        let terms = expanded(json!({"q": "colour==\"active\"", "expandValues": "colour"})).await.expect("expands");

        pretty_assertions::assert_eq!(
            expanded_q(&terms),
            json!("\"https://uri.etsi.org/ngsi-ld/default-context/colour\"==\"https://uri.etsi.org/ngsi-ld/default-context/active\"")
        );
    }

    /// The same filter without the opt-in keeps its literal operand, which is
    /// what makes the member observable at all.
    #[tokio::test]
    async fn a_filter_without_the_opt_in_keeps_its_literal_operand() {
        let terms = expanded(json!({"q": "colour==\"active\""})).await.expect("expands");

        pretty_assertions::assert_eq!(expanded_q(&terms), json!("\"https://uri.etsi.org/ngsi-ld/default-context/colour\"==\"active\""));
    }

    /// Table 5.2.12-1: the values of a `jsonKeys` attribute are
    /// "uninterpretable as JSON-LD", so the object keys the filter addresses
    /// under them stay literal, while the attribute itself still expands.
    ///
    /// The path has to be the bracketed form of clause 4.9. A dotted path names
    /// sub-attributes, which are NGSI-LD terms and expand whatever `jsonKeys`
    /// says; only the `[...]` form reaches inside a value, and clause 4.9
    /// EXAMPLE 11 is exactly that case, which the spec words as "the target value
    /// raw is JSON, and is not expanded to ngsi-ld:hasValue using the core @context".
    #[tokio::test]
    async fn json_keys_leaves_the_addressed_object_keys_literal() {
        let declared = expanded(json!({"q": "payload[city]==\"Berlin\"", "jsonKeys": "payload"}))
            .await
            .expect("expands");

        let written = expanded_q(&declared);
        let rendered = written.as_str().expect("the filter is a string");
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

        let written = expanded_q(&undeclared);
        let rendered = written.as_str().expect("the filter is a string");
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
        let terms = expanded(wire).await.expect("expands");

        assert!(
            terms.entities.is_ngsi_ld_null()
                && terms.watched_attributes.is_ngsi_ld_null()
                && terms.q.is_ngsi_ld_null()
                && terms.csf.is_ngsi_ld_null()
                && terms.geo_q.is_ngsi_ld_null(),
            "{terms:?}"
        );
    }

    /// Clause 4.9 reads `name[en]` on a `LanguageProperty` as the target
    /// language `en`: the filter keeps that spelling beside the expanded member
    /// its text form carries.
    #[tokio::test]
    async fn the_expanded_q_keeps_the_written_trailing_members() {
        let terms = expanded(json!({"q": "name[en]==\"Door\""})).await.expect("expands");
        let q = terms.q.as_set().expect("the request names a q");

        pretty_assertions::assert_eq!(
            expanded_q(&terms),
            json!("\"https://uri.etsi.org/ngsi-ld/default-context/name\"[\"https://uri.etsi.org/ngsi-ld/default-context/en\"]==\"Door\"")
        );
        pretty_assertions::assert_eq!(q.written_member_text(), "\"https://uri.etsi.org/ngsi-ld/default-context/name\"[en]==\"Door\"");
    }

    /// Clause 4.9: a `csf` targets Context Source Properties, whose values are
    /// the raw JSON a registrant wrote, so the stored filter names the property
    /// by IRI and keeps the member as the literal key it addresses.
    #[tokio::test]
    async fn the_expanded_csf_keeps_its_trailing_members_verbatim() {
        let terms = expanded(json!({"csf": "coverage[region]==\"north\""})).await.expect("expands");

        pretty_assertions::assert_eq!(
            expanded_csf(&terms),
            json!("\"https://uri.etsi.org/ngsi-ld/default-context/coverage\"[region]==\"north\"")
        );
    }

    /// Clause 5.5.7: the `geoproperty` a `geoQ` names expands like every other
    /// term. `observationSpace` is a term of the core context, so it expands to
    /// the NGSI-LD vocabulary rather than the default one.
    #[tokio::test]
    async fn the_geoproperty_of_a_geo_q_is_expanded() {
        let terms = expanded(json!({"geoQ": {"georel": "within", "geometry": "Point", "coordinates": [1, 2], "geoproperty": "observationSpace"}}))
            .await
            .expect("expands");

        let geoproperty = terms.geo_q.as_set().and_then(|geo_q| geo_q.geoproperty.as_ref());
        pretty_assertions::assert_eq!(geoproperty.map(AttributeIri::as_str), Some("https://uri.etsi.org/ngsi-ld/observationSpace"));
    }

    /// Clause 5.5.7: the type selection of an entity selector expands.
    #[tokio::test]
    async fn an_entity_selector_type_expands_through_the_context() {
        let terms = expanded(json!({"entities": [{"type": "Vehicle"}], "watchedAttributes": ["speed"]}))
            .await
            .expect("expands");

        pretty_assertions::assert_eq!(
            serde_json::to_value(terms.entities.as_set()).expect("the selectors write"),
            json!([{"type": format!("{DEFAULT}Vehicle")}])
        );
        pretty_assertions::assert_eq!(spelled(terms.watched_attributes.as_set()), vec![format!("{DEFAULT}speed")]);
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
        let terms = expanded(json!({"watchedAttributes": ["speed"], "geoQ": {"georel": "near;maxDistance==10", "geometry": "Point", "coordinates": [1, 2], "geoproperty": "place"}}))
            .await
            .expect("expands");

        pretty_assertions::assert_eq!(spelled(terms.watched_attributes.as_set()), vec![format!("{DEFAULT}speed")]);
        let geoproperty = terms.geo_q.as_set().and_then(|geo_q| geo_q.geoproperty.as_ref());
        pretty_assertions::assert_eq!(geoproperty.map(AttributeIri::as_str), Some(format!("{DEFAULT}place").as_str()));
    }

    /// A written `q` that validation never admitted, and an admission with no
    /// `q` to compile, are broken invariants and not a compilation.
    #[tokio::test]
    async fn a_q_and_its_admission_must_come_together() {
        let written = request(&json!({"q": "speed>1"}));
        let unadmitted = expand_common_subscription_terms(&offline_engine(), &written, &ContextSpec::none(), None).await;
        assert!(
            matches!(unadmitted, Err(SubscriptionError::Internal(SubscriptionInvariant::QNotAdmitted))),
            "{unadmitted:?}"
        );

        let admitted = admit_q(&request(&json!({"q": "speed>1"})), broker_defaults(LanguageTargeting::Specification)).expect("the q is admitted");
        let absent = request(&json!({}));
        let unwritten = expand_common_subscription_terms(&offline_engine(), &absent, &ContextSpec::none(), admitted.as_ref()).await;
        assert!(
            matches!(unwritten, Err(SubscriptionError::Internal(SubscriptionInvariant::QNotAdmitted))),
            "{unwritten:?}"
        );
    }
}
