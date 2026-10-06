//! The `@context` one subscription's notifications carry.
//!
//! Clause 5.8.1.4 has a notification sent under "the one specified in the
//! jsonldContext field", and the two carriers clause 6.3.8 chooses between name
//! it differently: a `Link` header names the `jsonldContext` itself, while a
//! body names it followed by the core context whenever clause 5.2.3 still owes
//! that. Both spellings are one decision, so they are resolved together, once
//! per notification, and travel as one value the transports read.

use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{engine::processor::JsonLdEngine, error::JsonLdError};
#[cfg(test)]
use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
use iri_rs::IriBuf;

/// The `@context` of one subscription's notification, in the spelling each
/// carrier names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationContext {
    /// The subscription's `jsonldContext`, when it set one; the `Link` header
    /// of a notification that carries its `@context` there names it.
    jsonld_context: Option<IriBuf>,
    /// The `@context` member of a notification that carries it in the body:
    /// `jsonldContext` followed by the core context when clause 5.2.3 still
    /// owes it, as the subscription's JSON-LD processor resolved it, held as
    /// the text every notification body writes.
    body_context: JsonLiteral,
}

impl NotificationContext {
    /// Resolve the notification `@context` of a subscription whose
    /// `jsonldContext` is `jsonld_context`.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] a document the `jsonldContext` names could
    /// not be loaded with (clause 5.5.6): no notification can be rendered
    /// under a context that cannot be read.
    pub async fn resolve(jsonld_context: Option<&IriBuf>, engine: &JsonLdEngine) -> Result<NotificationContext, JsonLdError> {
        let body_context = engine.notification_body_context(jsonld_context).await?;
        Ok(NotificationContext {
            jsonld_context: jsonld_context.cloned(), // clone: the context outlives the subscription it was read from
            body_context,
        })
    }

    /// The subscription's `jsonldContext`, when it set one.
    #[must_use]
    pub const fn jsonld_context(&self) -> Option<&IriBuf> {
        self.jsonld_context.as_ref()
    }

    /// The `@context` member a notification carrying it in the body names.
    #[must_use]
    pub const fn body_context(&self) -> &JsonLiteral {
        &self.body_context
    }

    /// A context whose two spellings a test states directly, for the
    /// transports' own unit tests, which run without a JSON-LD processor.
    #[cfg(test)]
    pub(crate) const fn stated(jsonld_context: Option<IriBuf>, body_context: JsonLiteral) -> NotificationContext {
        NotificationContext { jsonld_context, body_context }
    }

    /// The context of a subscription that named no `jsonldContext`, for the
    /// transports' own unit tests.
    #[cfg(test)]
    pub(crate) fn core() -> NotificationContext {
        let core = JsonLiteral::string(NGSI_LD_CORE_CONTEXT_URL.as_str()).expect("the core context is a JSON string");
        NotificationContext::stated(None, core)
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_context::NotificationContext;
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;

    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds")
    }

    /// Clause 5.5.5: a subscription that named no `jsonldContext` is notified
    /// under the core context alone.
    #[tokio::test]
    async fn no_jsonld_context_resolves_to_the_core_context() {
        let context = NotificationContext::resolve(None, &offline_engine()).await.expect("resolves");

        assert!(context.jsonld_context().is_none());
        pretty_assertions::assert_eq!(context.body_context().text(), format!("\"{NGSI_LD_CORE_CONTEXT_URL}\""));
    }

    /// A `jsonldContext` naming a core context is the subscriber's explicit
    /// choice, named alone as written in both carriers.
    #[tokio::test]
    async fn a_core_jsonld_context_is_named_as_written() {
        let chosen = IriBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".to_owned()).expect("a valid IRI");

        let context = NotificationContext::resolve(Some(&chosen), &offline_engine()).await.expect("resolves");

        pretty_assertions::assert_eq!(context.jsonld_context(), Some(&chosen));
        pretty_assertions::assert_eq!(context.body_context().text(), format!("\"{chosen}\""));
    }

    /// Clause 5.5.6: a `jsonldContext` whose document cannot be loaded names no
    /// context a notification can be rendered under.
    #[tokio::test]
    async fn an_unloadable_jsonld_context_is_refused() {
        let unreachable = IriBuf::new("https://example.com/unreachable.jsonld".to_owned()).expect("a valid IRI");

        assert!(NotificationContext::resolve(Some(&unreachable), &offline_engine()).await.is_err());
    }
}
