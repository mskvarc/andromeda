//! The body of a Notification a Context Source sends this broker (clauses
//! 5.3.1, 5.3.2 and 5.8.6), admitted and kept as the text it arrived as.
//!
//! A Notification is a document a broker rendered rather than one a client
//! wrote, so it carries no user `@context` for clause 5.5.7 to admit: a
//! `FeatureCollection` in its `data` member, and each Entity of an
//! `application/ld+json` Notification, carry their own `@context` (Table
//! 5.3.1-1). Every other body precondition applies, and the receiver decodes
//! the text straight into the notification it is.

use crate::body_admission::{AdmittedBody, admit_request_body};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_jsonld::context_spec::ContextSpec;
use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
};

/// Axum extractor for the body of a received Notification.
pub struct NotificationBody {
    /// The body, as it arrived.
    pub text: Bytes,
    /// The context the notification names in its body or `Link` header.
    pub context: ContextSpec,
}

impl<S> FromRequest<S> for NotificationBody
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Clause 6.3.4 admits `application/merge-patch+json` for PATCH alone, so
        // the admission refuses it on a Notification's POST.
        admit_request_body(req, state, |AdmittedBody { text, context, .. }| Ok(NotificationBody { text, context })).await
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::notification_body::NotificationBody;
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use andromeda_jsonld::{context_spec::ContextSpec, request_context::RequestContext};
    use axum::{
        body::Body,
        extract::{FromRequest, Request},
        http::{StatusCode, header},
        response::IntoResponse,
    };
    use iri_rs::IriBuf;

    const NOTIFICATION: &str = r#"{"id":"urn:ngsi-ld:Notification:1","type":"Notification","subscriptionId":"urn:ngsi-ld:Subscription:1","notifiedAt":"2024-01-01T00:00:00Z","data":[]}"#;

    fn request(content_type: &str, body: &'static str, link: Option<&str>) -> Request {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/notifications")
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CONTENT_LENGTH, body.len());
        if let Some(link) = link {
            builder = builder.header(header::LINK, link);
        }
        builder.body(Body::from(body)).expect("a request")
    }

    fn status(rejection: ExtractorRejection) -> StatusCode {
        rejection.into_response().status()
    }

    /// N-D2: `application/merge-patch+json` is the Merge Entity body alone.
    #[tokio::test]
    async fn merge_patch_is_not_a_notification() {
        let rejection = NotificationBody::from_request(request("application/merge-patch+json", NOTIFICATION, None), &())
            .await
            .err()
            .expect("refused");
        pretty_assertions::assert_eq!(status(rejection), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn a_json_notification_takes_its_context_from_the_link() {
        let link = r#"<https://example.org/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;
        let body = NotificationBody::from_request(request("application/json", NOTIFICATION, Some(link)), &())
            .await
            .expect("admitted");
        let expected = IriBuf::new("https://example.org/ctx.jsonld".to_owned()).expect("an IRI");
        pretty_assertions::assert_eq!(body.context, ContextSpec::from_request(RequestContext::Link(expected)));
    }

    #[tokio::test]
    async fn an_ld_json_notification_takes_its_context_from_the_body() {
        let body = NotificationBody::from_request(
            request(
                "application/ld+json",
                r#"{"@context":"https://example.org/ctx.jsonld","id":"urn:ngsi-ld:Notification:1","type":"Notification","subscriptionId":"urn:ngsi-ld:Subscription:1","notifiedAt":"2024-01-01T00:00:00Z","data":[]}"#,
                None,
            ),
            &(),
        )
        .await
        .expect("admitted");
        assert!(body.context.body_context().is_some(), "{:?}", body.context);
    }

    /// Clause 6.3.5: an `application/json` body names its context in a
    /// `Link` header, never in the body.
    #[tokio::test]
    async fn json_with_an_inline_context_is_refused() {
        let rejection = NotificationBody::from_request(
            request(
                "application/json",
                r#"{"@context":"https://example.org/ctx.jsonld","id":"urn:ngsi-ld:Notification:1"}"#,
                None,
            ),
            &(),
        )
        .await
        .err()
        .expect("refused");
        pretty_assertions::assert_eq!(status(rejection), StatusCode::BAD_REQUEST);
    }

    /// Clause 6.3.4: a body without a declared length is refused before it is
    /// read.
    #[tokio::test]
    async fn a_missing_content_length_is_411() {
        let request = Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/notifications")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(NOTIFICATION))
            .expect("a request");
        let rejection = NotificationBody::from_request(request, &()).await.err().expect("refused");
        pretty_assertions::assert_eq!(status(rejection), StatusCode::LENGTH_REQUIRED);
    }

    /// A Notification is broker-rendered, so its `data` may hold a
    /// `FeatureCollection` carrying its own `@context` (Table 5.3.1-1).
    #[tokio::test]
    async fn a_notification_carrying_a_feature_collection_context_is_admitted() {
        let body = r#"{"id": "urn:ngsi-ld:Notification:1", "type": "Notification", "subscriptionId": "urn:ngsi-ld:Subscription:1", "notifiedAt": "2024-01-01T00:00:00Z", "data": {"type": "FeatureCollection", "@context": "https://example.org/ctx.jsonld", "features": []}}"#;
        let notification = NotificationBody::from_request(request("application/json", body, None), &())
            .await
            .expect("a notification is admitted");
        pretty_assertions::assert_eq!(notification.text.as_ref(), body.as_bytes());
    }
}
