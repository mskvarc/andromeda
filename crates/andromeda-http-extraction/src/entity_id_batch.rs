//! The request body of a batch delete: an array of Entity IDs (clause
//! 5.6.10).
//!
//! Clause 5.6.10.3 takes "A JSON-LD Array", so the body passes every NGSI-LD
//! body precondition a JSON-LD document does -- clauses 6.3.4 and 6.3.5, the
//! JSON value limit and the number range -- before its items are read. An
//! `application/ld+json` array of strings names no `@context`, and clause
//! 6.3.5 refuses it as such.
//!
//! The body is decoded item by item rather than as one typed array, so an
//! `id` that is a URI too long to be stored is reported against itself as a
//! clause 5.2.17 `BatchEntityError` while the rest of the batch is deleted.
//! Only an item with no URI to report against fails the request.

use crate::{
    batch_array::batch_items,
    batch_body_refusal::{BatchBodyRefusal, BatchMembers},
    batch_item_refusal::BatchItemRefusal,
    jsonld::{AdmittedBody, admit_request_body},
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_jsonld::user_context_admission::SubmittedContext;
use andromeda_ngsi_ld_lexicon::{
    identifier::{Identifier, IdentifierParseError},
    uri::Uri,
};
use andromeda_ngsi_ld_problem_details::batch_operation::BatchEntityError;
use axum::extract::{FromRequest, Request};
use serde_json::Value;

/// The Entity IDs a batch delete names, and the ones refused per entity.
#[derive(Debug)]
pub struct EntityIdBatch {
    /// The identifiers to delete, in array order.
    pub ids: Vec<Identifier>,
    /// One clause 5.2.17 error per item refused against its own `id`.
    pub refused: Vec<BatchEntityError>,
}

/// Axum extractor for the body of a batch delete, read as the JSON-LD array
/// clause 5.6.10.3 takes.
pub struct JsonLdEntityIds {
    /// The identifiers the body names.
    pub batch: EntityIdBatch,
    /// The context the request named, in its `Link` header (clause 6.3.5),
    /// admitted before anything is read under it.
    pub context: SubmittedContext,
}

impl<S> FromRequest<S> for JsonLdEntityIds
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request(req: Request, state: &S) -> Result<JsonLdEntityIds, ExtractorRejection> {
        admit_request_body(req, state, |AdmittedBody { text, context, uri }| {
            let batch = entity_id_batch(&text).map_err(|refusal| ExtractorRejection::Classified(refusal.into_problem(uri)))?;
            Ok(JsonLdEntityIds {
                batch,
                context: SubmittedContext::from(context),
            })
        })
        .await
    }
}

/// The identifiers `body` names, or the refusal that fails the request.
fn entity_id_batch(body: &[u8]) -> Result<EntityIdBatch, BatchBodyRefusal> {
    let items = batch_items::<Value>(body, BatchMembers::EntityIds)?;
    let mut batch = EntityIdBatch {
        ids: Vec::with_capacity(items.len()),
        refused: Vec::new(),
    };
    for (index, item) in items.into_iter().enumerate() {
        let raw = match item {
            Value::String(raw) => raw,
            Value::Null => return Err(BatchBodyRefusal::NullItem { index }),
            Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => return Err(BatchBodyRefusal::NotAnEntityId { index }),
        };
        match raw.parse::<Identifier>() {
            Ok(id) => batch.ids.push(id),
            Err(IdentifierParseError::Malformed(source)) => return Err(BatchBodyRefusal::MalformedId { index, source }),
            // The `id` is a URI past the identifier bound; it is parsed once
            // more as the URI the error is reported against, which only this
            // refused item pays for.
            Err(IdentifierParseError::TooLong(too_long)) => {
                let entity_id = raw.parse::<Uri>().map_err(|source| BatchBodyRefusal::MalformedId { index, source })?;
                batch.refused.push(BatchItemRefusal::IdTooLong(too_long).against(entity_id));
            }
        }
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use crate::entity_id_batch::JsonLdEntityIds;
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use andromeda_jsonld::{context_spec::ContextSpec, request_context::RequestContext, user_context_admission::SubmittedContext};
    use andromeda_ngsi_ld_lexicon::identifier_length::IDENTIFIER_BYTE_LIMIT;
    use andromeda_ngsi_ld_problem_details::{batch_operation::BatchErrorKind, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use axum::{
        body::Body,
        extract::FromRequest,
        http::{
            Request,
            StatusCode,
            header::{CONTENT_LENGTH, CONTENT_TYPE, LINK},
        },
    };
    use iri_rs::IriBuf;

    const CONTEXT_LINK: &str = r#"<https://example.org/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;

    /// A batch delete request of `content_type` carrying `body`, declaring its
    /// length, and one `Link` line per entry of `links`.
    fn request(content_type: &str, body: String, links: &[&str]) -> Request<Body> {
        let mut request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/delete")
            .header(CONTENT_TYPE, content_type)
            .header(CONTENT_LENGTH, body.len());
        for link in links {
            request = request.header(LINK, *link);
        }
        request.body(Body::from(body)).expect("request builds")
    }

    /// An `application/json` batch delete request carrying `body`.
    fn json_request(body: &str) -> Request<Body> {
        request("application/json", body.to_owned(), &[])
    }

    async fn extract(request: Request<Body>) -> Result<JsonLdEntityIds, ExtractorRejection> {
        JsonLdEntityIds::from_request(request, &()).await
    }

    /// The category and detail the request is refused with.
    async fn refusal_of(request: Request<Body>) -> (NgsiLdErrorType, String) {
        match extract(request).await {
            Ok(extracted) => panic!("the request was to fail as a whole, parsed {:?}", extracted.batch),
            Err(ExtractorRejection::Classified(response)) => (response.problem().error_type(), response.problem().detail().to_owned()),
            Err(ExtractorRejection::BadRequestData { detail, .. }) => (NgsiLdErrorType::BadRequestData, detail),
            Err(ExtractorRejection::InvalidRequest { detail, .. }) => (NgsiLdErrorType::InvalidRequest, detail),
            Err(other @ (ExtractorRejection::Empty(_) | ExtractorRejection::NotAcceptable(_))) => panic!("expected a problem, got {other:?}"),
        }
    }

    /// A URN of exactly `length` octets.
    fn urn_of_length(length: usize) -> String {
        let prefix = "urn:ngsi-ld:Vehicle:";
        format!("{prefix}{}", "a".repeat(length - prefix.len()))
    }

    #[tokio::test]
    async fn a_json_id_array_is_read_against_its_one_context_link() {
        let extracted = extract(request("application/json", r#"["urn:ngsi-ld:T:1"]"#.to_owned(), &[CONTEXT_LINK]))
            .await
            .expect("a batch of ids");
        pretty_assertions::assert_eq!(extracted.batch.ids.iter().map(ToString::to_string).collect::<Vec<_>>(), vec!["urn:ngsi-ld:T:1"]);
        pretty_assertions::assert_eq!(
            extracted.context,
            SubmittedContext::from(ContextSpec::from_link(
                IriBuf::new("https://example.org/ctx.jsonld".to_owned()).expect("a valid IRI")
            ))
        );
    }

    #[tokio::test]
    async fn a_json_id_array_without_a_link_is_read_against_the_default_context() {
        let extracted = extract(json_request(r#"["urn:ngsi-ld:T:1"]"#)).await.expect("a batch of ids");
        pretty_assertions::assert_eq!(extracted.context, SubmittedContext::from(ContextSpec::from_request(RequestContext::Default)));
    }

    /// Clause 6.3.5: an `application/ld+json` body names its context in an
    /// `@context` member, which an array of strings cannot carry.
    #[tokio::test]
    async fn an_ld_json_id_array_names_no_context_and_is_bad_request_data() {
        pretty_assertions::assert_eq!(
            refusal_of(request("application/ld+json", r#"["urn:ngsi-ld:T:1"]"#.to_owned(), &[])).await,
            (NgsiLdErrorType::BadRequestData, "application/ld+json body must contain @context".to_owned())
        );
    }

    /// Clause 6.3.5: an `application/ld+json` body beside a context `Link` is
    /// `BadRequestData`.
    #[tokio::test]
    async fn an_ld_json_id_array_beside_a_context_link_is_bad_request_data() {
        let (error_type, _detail) = refusal_of(request("application/ld+json", r#"["urn:ngsi-ld:T:1"]"#.to_owned(), &[CONTEXT_LINK])).await;
        pretty_assertions::assert_eq!(error_type, NgsiLdErrorType::BadRequestData);
    }

    /// Clause 6.3.4: an unsupported media type is a bare 415, an absent
    /// `Content-Length` a bare 411.
    #[tokio::test]
    async fn the_clause_6_3_4_preconditions_answer_with_bare_statuses() {
        match extract(request("application/xml", r#"["urn:ngsi-ld:T:1"]"#.to_owned(), &[])).await {
            Err(ExtractorRejection::Empty(status)) => pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE),
            _other => panic!("expected a bare 415"),
        }
        let unmeasured = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/delete")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(r#"["urn:ngsi-ld:T:1"]"#))
            .expect("request builds");
        match extract(unmeasured).await {
            Err(ExtractorRejection::Empty(status)) => pretty_assertions::assert_eq!(status, StatusCode::LENGTH_REQUIRED),
            _other => panic!("expected a bare 411"),
        }
    }

    #[tokio::test]
    async fn well_formed_ids_are_all_identifiers() {
        let extracted = extract(json_request(r#"["urn:ngsi-ld:Vehicle:a","urn:ngsi-ld:Vehicle:b"]"#))
            .await
            .expect("a batch of ids");
        pretty_assertions::assert_eq!(
            extracted.batch.ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]
        );
        pretty_assertions::assert_eq!(extracted.batch.refused, Vec::new());
    }

    /// Table 5.2.17-1 sets no length on `entityId`: an `id` too long to be
    /// stored is reported against itself and its neighbours are deleted.
    #[tokio::test]
    async fn an_over_long_id_is_refused_against_itself() {
        let long_id = urn_of_length(IDENTIFIER_BYTE_LIMIT + 1);
        let body = format!(r#"["urn:ngsi-ld:Vehicle:a","{long_id}"]"#);
        let batch = extract(request("application/json", body, &[])).await.expect("a batch of ids").batch;
        pretty_assertions::assert_eq!(batch.ids.iter().map(ToString::to_string).collect::<Vec<_>>(), vec!["urn:ngsi-ld:Vehicle:a"]);
        pretty_assertions::assert_eq!(batch.refused.len(), 1);
        pretty_assertions::assert_eq!(batch.refused[0].entity_id.as_str(), long_id.as_str());
        pretty_assertions::assert_eq!(
            batch.refused[0].error.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::BadRequestData))
        );
        assert!(batch.refused[0].error.detail.contains("1025 bytes"), "{}", batch.refused[0].error.detail);
    }

    /// Clauses 5.6.10.3 and 5.6.10.4 and Table 5.2.17-1: an item with no URI to
    /// report against fails the request as `BadRequestData`, naming the item.
    #[tokio::test]
    async fn an_item_with_no_uri_to_report_against_fails_the_request() {
        for (body, detail) in [
            (r#"["urn:ngsi-ld:Vehicle:a",null]"#, "batch item 1 is null"),
            (r#"["urn:ngsi-ld:Vehicle:a",42]"#, "batch item 1 is not an entity id string"),
            (r#"["urn:ngsi-ld:Vehicle:a",{}]"#, "batch item 1 is not an entity id string"),
            (
                r#"["urn:ngsi-ld:Vehicle:a","not a uri"]"#,
                "the 'id' of batch item 1 is not a URI: invalid URI: not a valid URN, URL or IRI",
            ),
        ] {
            pretty_assertions::assert_eq!(
                refusal_of(json_request(body)).await,
                (NgsiLdErrorType::BadRequestData, detail.to_owned()),
                "{body}"
            );
        }
    }

    /// Clause 5.5.4: only a body that is not JSON is `InvalidRequest`.
    #[tokio::test]
    async fn the_body_shape_refusals_carry_the_clause_5_5_4_categories() {
        pretty_assertions::assert_eq!(refusal_of(json_request("[]")).await.0, NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(
            refusal_of(json_request("{}")).await,
            (
                NgsiLdErrorType::BadRequestData,
                "the request body must be a JSON array of entity ids".to_owned()
            )
        );
        pretty_assertions::assert_eq!(refusal_of(json_request("[,")).await.0, NgsiLdErrorType::InvalidRequest);
    }
}
