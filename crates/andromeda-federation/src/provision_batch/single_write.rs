//! The request a single-entity fallback write sends for one narrowed entity
//! (clauses 6.4 and 6.5).

use crate::{
    error::FederationError,
    forwarding_url::forwarding_url,
    provision_batch::{fallback_plan::SingleWrite, narrowing::NarrowedEntity},
};
use andromeda_ngsi_ld_entity::document::EntityDocument;
use std::borrow::Cow;
use url::Url;

/// The request one single-entity fallback write sends for `entity`: where, by
/// which method, and with which admitted document, if any.
pub(super) struct SingleWriteRequest<'e> {
    /// The forwarded URL, or the failure that stops it being built.
    pub(super) url: Result<Url, FederationError>,
    /// The HTTP method.
    pub(super) method: reqwest::Method,
    /// The document the request carries; a deletion carries none.
    pub(super) body: Option<Cow<'e, EntityDocument>>,
}

/// The request one single-entity fallback write sends for `entity`.
pub(super) fn single_write_request<'e>(write: SingleWrite, endpoint: &Url, entity: &'e NarrowedEntity<'_>) -> SingleWriteRequest<'e> {
    let id = entity.id.as_str();
    let whole = || entity.body.as_deref().map(Cow::Borrowed);
    let fragment = || entity.body.as_deref().map(without_identifier).map(Cow::Owned);
    let (url, method, body) = match write {
        SingleWrite::Create | SingleWrite::CreateThenOnConflict(_) => (forwarding_url(endpoint, &["entities"], None), reqwest::Method::POST, whole()),
        SingleWrite::Replace => (forwarding_url(endpoint, &["entities", id], None), reqwest::Method::PUT, whole()),
        SingleWrite::UpdateAttributes => (forwarding_url(endpoint, &["entities", id, "attrs"], None), reqwest::Method::PATCH, fragment()),
        SingleWrite::AppendAttributesNoOverwrite => (
            forwarding_url(endpoint, &["entities", id, "attrs"], Some("options=noOverwrite")),
            reqwest::Method::POST,
            fragment(),
        ),
        SingleWrite::Delete => (forwarding_url(endpoint, &["entities", id], None), reqwest::Method::DELETE, None),
        SingleWrite::Merge => (forwarding_url(endpoint, &["entities", id], None), reqwest::Method::PATCH, fragment()),
    };
    SingleWriteRequest { url, method, body }
}

/// An entity fragment for the attribute-level operations: the document
/// without its `id`, which the request path already names.
fn without_identifier(document: &EntityDocument) -> EntityDocument {
    let mut parts = document.clone().into_parts(); // clone: the fragment is a new document
    parts.id = None;
    EntityDocument::new(parts.id, parts.types, parts.scope, parts.expires_at, parts.attributes)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::provision_batch::{
        fallback_plan::{SingleWrite, UpsertFollowUp},
        narrowing::NarrowedEntity,
        registration_fixture::{document, uri},
        single_write::single_write_request,
    };
    use andromeda_jsonld::{batch_contexts::BatchContext, context_spec::ContextSpec};
    use std::borrow::Cow;
    use url::Url;

    #[test]
    fn every_single_entity_fallback_addresses_its_clause_6_4_and_6_5_resource() {
        let endpoint = Url::parse("http://peer").expect("valid URL");
        let context = BatchContext::shared(ContextSpec::none());
        let entity = NarrowedEntity {
            id: uri("urn:ngsi-ld:Vehicle:1"),
            body: Some(Cow::Owned(document(
                "urn:ngsi-ld:Vehicle:1",
                "https://example.org/Vehicle",
                &["https://example.org/speed"],
            ))),
            proxied_attrs: Vec::new(),
            context: &context,
        };
        let cases = [
            (SingleWrite::Create, reqwest::Method::POST, "http://peer/ngsi-ld/v1/entities", true),
            (
                SingleWrite::CreateThenOnConflict(UpsertFollowUp::Replace),
                reqwest::Method::POST,
                "http://peer/ngsi-ld/v1/entities",
                true,
            ),
            (
                SingleWrite::Replace,
                reqwest::Method::PUT,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1",
                true,
            ),
            (
                SingleWrite::UpdateAttributes,
                reqwest::Method::PATCH,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1/attrs",
                false,
            ),
            (
                SingleWrite::AppendAttributesNoOverwrite,
                reqwest::Method::POST,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1/attrs?options=noOverwrite",
                false,
            ),
            (
                SingleWrite::Delete,
                reqwest::Method::DELETE,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1",
                false,
            ),
            (
                SingleWrite::Merge,
                reqwest::Method::PATCH,
                "http://peer/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1",
                false,
            ),
        ];
        for (write, expected_method, expected_url, keeps_id) in cases {
            let request = single_write_request(write, &endpoint, &entity);
            pretty_assertions::assert_eq!(request.url.expect("joins").as_str(), expected_url, "{write:?}");
            pretty_assertions::assert_eq!(request.method, expected_method, "{write:?}");
            if write == SingleWrite::Delete {
                assert!(request.body.is_none(), "a deletion carries no body");
                continue;
            }
            let body = request.body.expect("a body");
            pretty_assertions::assert_eq!(body.id().is_some(), keeps_id, "{write:?}");
            assert!(!body.attributes().is_empty(), "{write:?}");
        }
    }
}
