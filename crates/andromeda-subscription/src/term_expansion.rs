//! Expansion of a subscription payload's terms against the request `@context`
//! (ETSI GS CIM 009 clause 5.5.7).
//!
//! Everything a subscription is later matched by is stored in expanded form —
//! entity types, watched attributes, the notification's attribute lists, the
//! `q` and `csf` filters, the `geoQ` geoproperty — because matching runs
//! against expanded entity data and never has the creator's `@context` at hand.
//! Context hosting is not handled here; the entity-subscription variant adds
//! that on top.

use crate::{error::SubscriptionError, fields::attribute_name_csv_member};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
    term_expansion::ContextualExpander,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    filter::{
        expansion::{apply_expand_values, expand_attribute_names, expand_q_expr},
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
        parser,
        serialize,
    },
    projection::term::ProjectionTerm,
    term_expander::TermExpander,
    type_selection::{
        expansion::expand_type_expression,
        grammar::{TypeExpr, TypeExprRaw},
    },
};
use serde_json::Value;
use std::collections::HashSet;
use tracing::warn;

/// Which names of an attribute list are left as written rather than expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructuralKeywords {
    /// Every name is an attribute term and expands.
    Expand,
    /// The clause 4.21 `id`, `type` and `scope` name structural members of
    /// the representation, not attributes, and stay as written.
    Keep,
}

/// Expand every string of one attribute-name list in place (clause 5.5.7).
///
/// An unavailable `@context` fails the request; a single name the context
/// cannot expand is left as written and reported, the same lenience the
/// attribute lists always had.
async fn expand_name_list(
    engine: &JsonLdEngine,
    prepared_context: &PreparedTermContext,
    names: &mut [Value],
    member: &str,
    keywords: StructuralKeywords,
) -> Result<(), SubscriptionError> {
    for name in names.iter_mut() {
        let Some(term) = name.as_str() else {
            continue;
        };
        if keywords == StructuralKeywords::Keep && matches!(term, ProjectionTerm::ID_KEYWORD | ProjectionTerm::TYPE_KEYWORD | ProjectionTerm::SCOPE_KEYWORD) {
            continue;
        }
        match engine.expand_term(term, prepared_context).await {
            Ok(expanded) => *name = Value::String(expanded.to_string()),
            Err(error) => {
                if error.is_context_not_available() {
                    return Err(SubscriptionError::from(error));
                }
                warn!(member, term, %error, "Failed to expand subscription attribute name");
            }
        }
    }
    Ok(())
}

/// The two clause 6.4.3.2 opt-in lists a subscription carries, already expanded
/// through the request `@context` (Table 5.2.12-1 `expandValues` / `jsonKeys`).
///
/// They travel together because both are read from the same payload, both key
/// the same `q` filter, and neither means anything without it.
#[derive(Debug, Default)]
struct FilterOptIns {
    /// Attributes whose `q` values are expanded by JSON-LD type coercion.
    expand_values: HashSet<AttributeName>,
    /// Attributes whose values are uninterpretable as JSON-LD, so the object
    /// keys the filter addresses under them stay literal.
    json_keys: JsonKeyAttributes,
}

/// Write one opt-in member back in the comma-separated form Table 5.2.12-1
/// defines, leaving an empty list absent.
fn rewrite_opt_in_member(payload: &mut Value, member: &str, names: &[AttributeName]) {
    if names.is_empty() {
        return;
    }
    let rendered = names.iter().map(AttributeName::as_str).collect::<Vec<_>>().join(",");
    if let Some(object) = payload.as_object_mut() {
        object.insert(member.to_owned(), Value::String(rendered));
    }
}

/// Expand both opt-in lists and store them expanded, so the persisted member
/// names the same IRIs the persisted `q` was expanded into.
async fn expand_filter_opt_ins(payload: &mut Value, expander: &impl TermExpander) -> Result<FilterOptIns, SubscriptionError> {
    let expand_values = expand_attribute_names(&attribute_name_csv_member(payload, "expandValues")?.unwrap_or_default(), expander).await?;
    let json_keys = expand_attribute_names(&attribute_name_csv_member(payload, "jsonKeys")?.unwrap_or_default(), expander).await?;
    rewrite_opt_in_member(payload, "expandValues", &expand_values);
    rewrite_opt_in_member(payload, "jsonKeys", &json_keys);
    Ok(FilterOptIns {
        expand_values: expand_values.into_iter().collect(),
        json_keys: json_keys.into_iter().collect(),
    })
}

/// Apply the `expandValues` coercion to an already term-expanded `q` filter.
///
/// The subscription's filter is evaluated against notified entity data by a
/// synchronous matcher that has no `@context` and no expander, so the coercion
/// Table 5.2.12-1 asks for happens once, here, at create and update time; what
/// is stored is the filter matching consumes. `jsonKeys` is not applied here at
/// all — it decides how the filter's own terms expand, so it is handed to the
/// expansion instead.
async fn apply_expand_values_opt_in(expr: Expr, opt_ins: &FilterOptIns, expander: &impl TermExpander) -> Result<Expr, SubscriptionError> {
    if opt_ins.expand_values.is_empty() {
        return Ok(expr);
    }
    Ok(apply_expand_values(expr, &opt_ins.expand_values, expander).await?)
}

/// Expand entities, watchedAttributes, the notification's attributes, pick
/// and omit, q, and geoQ in a subscription payload. Context hosting is NOT
/// handled here — the entity-subscription service adds that on top.
///
/// # Errors
///
/// Returns [`SubscriptionError::JsonLd`] when the `@context` cannot be
/// resolved, [`SubscriptionError::Query`] when a type selection or an opt-in
/// term does not expand to a usable name, and
/// [`SubscriptionError::Validation`] when `expandValues` or `jsonKeys` is not
/// the comma-separated attribute-name list Table 5.2.12-1 defines.
pub async fn expand_common_subscription_terms(engine: &JsonLdEngine, payload: &mut Value, context: &ContextSpec) -> Result<(), SubscriptionError> {
    let prepared_context = engine.prepare_term_context(context);
    let term_expander = ContextualExpander::new(engine, &prepared_context);
    // Expand entities[].type
    if let Some(entities) = payload.get_mut("entities")
        && let Some(arr) = entities.as_array_mut()
    {
        for entity_sel in arr.iter_mut() {
            if let Some(type_val) = entity_sel.get("type").and_then(|v| v.as_str()) {
                let raw = type_val.parse::<TypeExprRaw>()?;
                match expand_type_expression(&raw, &term_expander).await.and_then(TypeExpr::into_raw) {
                    Ok(expanded) => {
                        if let Some(obj) = entity_sel.as_object_mut() {
                            obj.insert("type".to_string(), Value::String(expanded.to_string()));
                        }
                    }
                    Err(error) => return Err(SubscriptionError::from(error)),
                }
            }
        }
    }

    if let Some(watched) = payload.get_mut("watchedAttributes").and_then(Value::as_array_mut) {
        expand_name_list(engine, &prepared_context, watched, "watchedAttributes", StructuralKeywords::Expand).await?;
    }

    // Clause 5.2.14: `attributes` names attributes only; `pick` and `omit`
    // may also name the structural members of clause 4.21.
    if let Some(notification) = payload.get_mut("notification") {
        for (member, keywords) in [
            ("attributes", StructuralKeywords::Expand),
            ("pick", StructuralKeywords::Keep),
            ("omit", StructuralKeywords::Keep),
        ] {
            if let Some(names) = notification.get_mut(member).and_then(Value::as_array_mut) {
                expand_name_list(engine, &prepared_context, names, member, keywords).await?;
            }
        }
    }

    let opt_ins = expand_filter_opt_ins(payload, &term_expander).await?;

    // Expand the `q` filter's attribute terms under the `jsonKeys` declaration,
    // then let `expandValues` rewrite the operands it opted in.
    if let Some(q_str) = payload.get("q").and_then(|v| v.as_str()).map(String::from)
        && let Ok(expr) = parser::parse(&q_str)
        && let Ok(expanded) = expand_q_expr(expr, &opt_ins.json_keys, &term_expander).await
    {
        let expanded = apply_expand_values_opt_in(expanded, &opt_ins, &term_expander).await?;
        let expanded_str = serialize::serialize(&expanded);
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("q".to_string(), Value::String(expanded_str));
        }
    }

    // Expand the context source filter's property terms the same way: a
    // registration's Context Source Properties are stored expanded (clause
    // 5.5.7), so the filter matched against them (clauses 5.8.1.4 and
    // 5.11.2.4) has to name them by IRI too.
    if let Some(csf_str) = payload.get("csf").and_then(|v| v.as_str()).map(String::from)
        && let Ok(expr) = parser::parse(&csf_str)
        && let Ok(expanded) = expand_q_expr(expr, &JsonKeyAttributes::default(), &term_expander).await
    {
        let expanded_str = serialize::serialize(&expanded);
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("csf".to_string(), Value::String(expanded_str));
        }
    }

    // Expand geoQ.geoproperty
    if let Some(geo_q) = payload.get_mut("geoQ")
        && let Some(geoprop) = geo_q.get("geoproperty").and_then(|v| v.as_str()).map(String::from)
    {
        match engine.expand_term(&geoprop, &prepared_context).await {
            Ok(expanded) => {
                if let Some(obj) = geo_q.as_object_mut() {
                    obj.insert("geoproperty".to_string(), Value::String(expanded.to_string()));
                }
            }
            Err(e) => {
                if e.is_context_not_available() {
                    return Err(SubscriptionError::from(e));
                }
                warn!("Failed to expand geoQ.geoproperty '{}': {}", geoprop, e);
            }
        }
    }

    // Strip @context — it's been used for expansion
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("@context");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::term_expansion::expand_common_subscription_terms;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use serde_json::{Value, json};

    /// An engine that resolves only what is embedded, never the network.
    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// Clause 5.5.7 for the clause 5.2.14 lists: `attributes`, `pick` and
    /// `omit` expand their attribute names through the context, and the
    /// clause 4.21 structural keywords of `pick`/`omit` stay as written.
    #[tokio::test]
    async fn notification_pick_and_omit_expand_attribute_names_and_keep_the_structural_keywords() {
        let engine = offline_engine();
        let mut payload = json!({
            "notification": {
                "attributes": ["speed"],
                "pick": ["id", "speed"],
                "omit": ["scope", "type", "colour"]
            }
        });

        expand_common_subscription_terms(&engine, &mut payload, &ContextSpec::none())
            .await
            .expect("expands");

        let notification = &payload["notification"];
        let is_iri = |value: &Value| value.as_str().is_some_and(|name| name.starts_with("https://"));
        assert!(is_iri(&notification["attributes"][0]), "{notification}");
        pretty_assertions::assert_eq!(notification["pick"][0], json!("id"));
        assert!(is_iri(&notification["pick"][1]), "{notification}");
        pretty_assertions::assert_eq!(notification["omit"][0], json!("scope"));
        pretty_assertions::assert_eq!(notification["omit"][1], json!("type"));
        assert!(is_iri(&notification["omit"][2]), "{notification}");
    }

    /// Table 5.2.12-1: `expandValues` is stored as the same comma-separated
    /// String it arrived as, with every name expanded so it keys the expanded
    /// `q` the store holds.
    #[tokio::test]
    async fn the_opt_in_lists_are_stored_expanded_in_their_comma_separated_form() {
        let engine = offline_engine();
        let mut payload = json!({"q": "speed>10", "expandValues": "speed,colour", "jsonKeys": "payload"});

        expand_common_subscription_terms(&engine, &mut payload, &ContextSpec::none())
            .await
            .expect("expands");

        pretty_assertions::assert_eq!(
            payload["expandValues"],
            json!("https://uri.etsi.org/ngsi-ld/default-context/speed,https://uri.etsi.org/ngsi-ld/default-context/colour")
        );
        pretty_assertions::assert_eq!(payload["jsonKeys"], json!("https://uri.etsi.org/ngsi-ld/default-context/payload"));
    }

    /// Table 5.2.12-1: the identified attributes' values "should be expanded
    /// against the supplied @context using JSON-LD type coercion prior to
    /// executing the query", so the stored filter is the coerced one.
    #[tokio::test]
    async fn expand_values_rewrites_the_stored_filter_operand() {
        let engine = offline_engine();
        let mut payload = json!({"q": "colour==\"active\"", "expandValues": "colour"});

        expand_common_subscription_terms(&engine, &mut payload, &ContextSpec::none())
            .await
            .expect("expands");

        pretty_assertions::assert_eq!(
            payload["q"],
            json!("\"https://uri.etsi.org/ngsi-ld/default-context/colour\"==\"https://uri.etsi.org/ngsi-ld/default-context/active\"")
        );
    }

    /// The same filter without the opt-in keeps its literal operand, which is
    /// what makes the member observable at all.
    #[tokio::test]
    async fn a_filter_without_the_opt_in_keeps_its_literal_operand() {
        let engine = offline_engine();
        let mut payload = json!({"q": "colour==\"active\""});

        expand_common_subscription_terms(&engine, &mut payload, &ContextSpec::none())
            .await
            .expect("expands");

        pretty_assertions::assert_eq!(payload["q"], json!("\"https://uri.etsi.org/ngsi-ld/default-context/colour\"==\"active\""));
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
        let engine = offline_engine();
        let mut declared = json!({"q": "payload[city]==\"Berlin\"", "jsonKeys": "payload"});

        expand_common_subscription_terms(&engine, &mut declared, &ContextSpec::none())
            .await
            .expect("expands");

        let rendered = declared["q"].as_str().expect("the filter is a string");
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
        let engine = offline_engine();
        let mut undeclared = json!({"q": "payload[city]==\"Berlin\""});

        expand_common_subscription_terms(&engine, &mut undeclared, &ContextSpec::none())
            .await
            .expect("expands");

        let rendered = undeclared["q"].as_str().expect("the filter is a string");
        assert!(
            rendered.contains("https://uri.etsi.org/ngsi-ld/default-context/city"),
            "an undeclared attribute's member name expands: {rendered}"
        );
    }

    #[tokio::test]
    async fn a_malformed_opt_in_member_is_refused_rather_than_stored() {
        let engine = offline_engine();
        for member in [json!({"expandValues": ["speed"]}), json!({"jsonKeys": "payload,"})] {
            let mut payload = member;
            assert!(
                expand_common_subscription_terms(&engine, &mut payload, &ContextSpec::none()).await.is_err(),
                "{payload} must be refused"
            );
        }
    }
}
