//! Initialising a subscription's notification `@context` from the `@context`
//! applicable when it was created (ETSI GS CIM 009 clause 5.8.2.4).
//!
//! Clause 5.8.2.4: "The @context to be used for sending Notifications related
//! to this Subscription shall be the one specified in the jsonldContext field.
//! If not present, the jsonldContext field shall be initialized with the
//! @context applicable for the Subscription (if @context is also not present in
//! the Subscription, see clause 5.5.5)." Table 5.2.12-1 types `jsonldContext`
//! as a single dereferenceable URI, so the applicable `@context` has to reduce
//! to one URI to be storable: a lone context URL is stored verbatim, while a
//! request that supplied several contexts, or supplied one inline in its body,
//! is materialised into a single broker-served Context (clause 5.13.1
//! `ImplicitlyCreated`) whose URL is stored instead. A request that supplied no
//! context of its own leaves the field absent and notifications fall to the core
//! context (clause 5.5.5).
//!
//! Hosting a compound Context needs the Context registry, which only the entity
//! subscription variant carries; a Context Source Registration Subscription has
//! none, so a compound applicable context it cannot represent falls to the core
//! context rather than being stored as an undereferenceable value.

use andromeda_context_registry::service::ContextRegistryService;
use andromeda_jsonld::context_spec::ContextSpec;
use serde_json::{Value, json};
use tracing::warn;
use url::Url;

/// The applicable creation `@context`, reduced to what `jsonldContext` can hold.
enum ApplicableContext {
    /// A single dereferenceable URI, storable in `jsonldContext` verbatim.
    Direct(String),
    /// Several contexts, or one supplied inline, that only a broker-served
    /// Context can name by a single URI. Carries the ordered `@context` array.
    Compound(Value),
}

/// Collect every `@context` the request supplied into one ordered list, `Link`
/// header contexts first and the body `@context` last (clause 5.5.7), then
/// reduce it to the single form `jsonldContext` can hold.
///
/// A body `@context` array is flattened into the list so a body carrying a
/// single URL string is indistinguishable from a single `Link` context, and an
/// inline body object is one element the list cannot express as a URI.
fn applicable_context(context: &ContextSpec) -> Option<ApplicableContext> {
    let mut elements: Vec<Value> = context.link_contexts().iter().map(|iri| Value::String(iri.as_str().to_owned())).collect();
    match context.body_context() {
        Some(Value::String(url)) => elements.push(Value::String(url.clone())), // clone: the list owns an element borrowed from the spec.
        Some(Value::Array(entries)) => elements.extend(entries.iter().cloned()), // clone: the list owns elements borrowed from the spec.
        Some(object @ Value::Object(_)) => elements.push(object.clone()),      // clone: the list owns an element borrowed from the spec.
        Some(Value::Null | Value::Bool(_) | Value::Number(_)) | None => {}
    }

    match elements.as_slice() {
        [] => None,
        [Value::String(single)] => Some(ApplicableContext::Direct(single.clone())), // clone: the outcome owns the sole element.
        _ => Some(ApplicableContext::Compound(Value::Array(elements))),
    }
}

/// Materialise a compound `@context` as a broker-served Context and return the
/// URL under which it is served, or `None` when it cannot be hosted.
///
/// `None` when the variant carries no registry (a Context Source Registration
/// Subscription), and `None` with a warning when the registry rejects the
/// document: the local subscription is valid either way, so an unhostable
/// context falls to the core context rather than failing the request.
async fn host_compound(host: Option<&ContextRegistryService>, context_array: Value, base_url: Option<&Url>) -> Option<String> {
    let registry = host?;
    match registry.create_implicitly(json!({ "@context": context_array }), base_url).await {
        Ok(served) => Some(match base_url {
            Some(base) => {
                let mut hosted = base.clone(); // clone: the request origin remains reusable.
                hosted.set_path(&format!("/ngsi-ld/v1/jsonldContexts/{served}"));
                hosted.to_string()
            }
            None => served.to_string(),
        }),
        Err(error) => {
            warn!(%error, "failed to host a subscription's applicable @context; its notifications fall back to the core context");
            None
        }
    }
}

/// Initialise the `jsonldContext` member of a subscription create payload from
/// the `@context` applicable at creation, when the request named none itself
/// (clause 5.8.2.4).
///
/// An explicit `jsonldContext` member takes precedence and is left untouched;
/// otherwise the applicable context is stored as a single URI, hosted first
/// when it does not already reduce to one. `host` carries the Context registry
/// for the variants that can serve a compound Context, and is `None` for those
/// that cannot.
pub(crate) async fn initialize_notification_context(payload: &mut Value, context: &ContextSpec, base_url: Option<&Url>, host: Option<&ContextRegistryService>) {
    if payload.get("jsonldContext").is_some() {
        return;
    }
    let stored = match applicable_context(context) {
        None => None,
        Some(ApplicableContext::Direct(uri)) => Some(uri),
        Some(ApplicableContext::Compound(context_array)) => host_compound(host, context_array, base_url).await,
    };
    if let Some(uri) = stored
        && let Some(object) = payload.as_object_mut()
    {
        object.insert("jsonldContext".to_owned(), Value::String(uri));
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_context::initialize_notification_context;
    use andromeda_jsonld::context_spec::ContextSpec;
    use iri_rs::IriRefBuf;
    use serde_json::{Value, json};

    /// A context URL as the [`IriRefBuf`] a `Link` header carries.
    fn iri_ref(url: &str) -> IriRefBuf {
        IriRefBuf::new(url.to_owned()).expect("a valid IRI reference")
    }

    /// The stored `jsonldContext`, or `None` when the fallback left it absent.
    fn stored_context(payload: &Value) -> Option<&str> {
        payload.get("jsonldContext").and_then(Value::as_str)
    }

    /// Clause 5.8.2.4: a lone `Link` context is the applicable context and is
    /// storable as the single URI `jsonldContext` holds, so it is kept verbatim.
    #[tokio::test]
    async fn a_single_link_context_is_stored_verbatim() {
        let mut payload = json!({});
        let context = ContextSpec::from_links(vec![iri_ref("https://example.com/one.jsonld")]);

        initialize_notification_context(&mut payload, &context, None, None).await;

        pretty_assertions::assert_eq!(stored_context(&payload), Some("https://example.com/one.jsonld"));
    }

    /// A body carrying a single context URL is indistinguishable from a lone
    /// `Link` context and is likewise stored verbatim.
    #[tokio::test]
    async fn a_single_body_context_url_is_stored_verbatim() {
        let mut payload = json!({});
        let context = ContextSpec::none().with_body_context(Some(json!("https://example.com/body.jsonld")));

        initialize_notification_context(&mut payload, &context, None, None).await;

        pretty_assertions::assert_eq!(stored_context(&payload), Some("https://example.com/body.jsonld"));
    }

    /// Clause 5.8.2.4 with Table 5.2.12-1: several `Link` contexts cannot be
    /// named by the single URI `jsonldContext` holds, and a variant with no
    /// registry cannot host a compound Context, so the field is left absent and
    /// notifications fall to the core context (clause 5.5.5) rather than storing
    /// an undereferenceable value.
    #[tokio::test]
    async fn several_link_contexts_without_a_host_fall_back_to_the_core_context() {
        let mut payload = json!({});
        let context = ContextSpec::from_links(vec![iri_ref("https://example.com/one.jsonld"), iri_ref("https://example.com/two.jsonld")]);

        initialize_notification_context(&mut payload, &context, None, None).await;

        assert!(
            stored_context(&payload).is_none(),
            "an unhostable compound context leaves the field absent: {payload}"
        );
    }

    /// An explicit `jsonldContext` member takes precedence over the applicable
    /// creation context and is left untouched (clause 5.8.2.4).
    #[tokio::test]
    async fn an_explicit_member_takes_precedence_over_the_applicable_context() {
        let mut payload = json!({"jsonldContext": "https://example.com/explicit.jsonld"});
        let context = ContextSpec::from_links(vec![iri_ref("https://example.com/one.jsonld")]);

        initialize_notification_context(&mut payload, &context, None, None).await;

        pretty_assertions::assert_eq!(stored_context(&payload), Some("https://example.com/explicit.jsonld"));
    }

    /// A request that supplied no context of its own leaves the field absent, so
    /// notifications fall to the core context (clause 5.5.5).
    #[tokio::test]
    async fn no_supplied_context_leaves_the_field_absent() {
        let mut payload = json!({});

        initialize_notification_context(&mut payload, &ContextSpec::none(), None, None).await;

        assert!(stored_context(&payload).is_none(), "no applicable context leaves the field absent: {payload}");
    }
}
