//! Batch entity request parsing for NGSI-LD batch operations.
//!
//! Provides [`BatchBody`] -- a [`FromRequest`] extractor that applies the clause
//! 6.3.4 request preconditions and classifies the representation the request
//! announced -- and [`parse_batch_entities`], which decodes the array a
//! [`BatchBody`] carries. Every Content-Type decision is taken once, in the
//! extractor: a `BatchBody` exists only for a request whose media type clause
//! 6.3.4 admits, so nothing below it has to re-derive that or answer for it.

use crate::{
    addressed_uri::addressed_request_uri,
    batch_array::batch_items,
    batch_body_refusal::{BatchBodyRefusal, BatchMembers},
    batch_item_decoding::{BatchItem, ItemIdentifier, written_identifier},
    batch_item_refusal::BatchItemRefusal,
    blocking_decode::{BlockingDecodePolicy, decode_payload},
    body_envelope::BodyEnvelope,
    classification::classify_context_link_error,
    content_kind::{ContentKind, admit_content_kind},
    content_length_precondition::{ContentLengthPreconditionError, declared_body_length},
    json_value_budget::refuse_values_past_budget,
    not_json_document::NotJsonDocument,
    request_decode_bounds::RequestDecodeBounds,
    request_json_value_limit::RequestJsonValueLimit,
    request_links::{PayloadCarrier, RequestLinks},
    request_payload::buffer_payload,
};
use andromeda_http_response::{problem_response::ProblemResponse, request_refusal::RequestRefusal};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{
    batch_contexts::{BatchContext, BatchContexts},
    context_spec::ContextSpec,
    request_context::RequestContext,
};
use andromeda_ngsi_ld_lexicon::identifier_length::IdentifierTooLong;
use andromeda_ngsi_ld_number::json_value::text_admission::{JsonTextRefusal, admit_json_text};
use andromeda_ngsi_ld_problem_details::batch_operation::BatchEntityError;
use andromeda_ngsi_ld_representation::embedded_context_admission::{EmbeddedContext, find_embedded_context};
use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    http::{HeaderMap, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use serde_json::value::RawValue;
use std::mem::take;

/// A batch request body that has passed the clause 6.3.4 preconditions.
///
/// The type is the evidence: it exists only for a request that declared a
/// payload length, announced a media type clause 6.3.4 admits, and did not
/// combine `application/ld+json` with a context `Link` header. Its fields are
/// private for that reason -- one cannot be assembled around a request that
/// failed any of those, so [`parse_batch_entities`] never has to ask again.
pub struct BatchBody {
    /// Request headers, replayed onto forwarded copies of the operation.
    headers: HeaderMap,
    /// The representation the request announced, classified once here.
    content_kind: ContentKind,
    /// The context the request's `Link` header named: the one the items of an
    /// `application/json` batch are read against, and the default context for
    /// an `application/ld+json` batch, which may not name one (clause 6.3.5).
    link_context: ContextSpec,
    /// Raw request body bytes.
    body: Bytes,
    /// The most JSON values the body may hold, when the stack published it.
    value_limit: Option<RequestJsonValueLimit>,
    /// The most positions a geometry of the body may hold, when the stack
    /// published it.
    decode_bounds: Option<RequestDecodeBounds>,
    /// How the body is decoded beside the executor, when the stack published
    /// it.
    blocking_decode: Option<BlockingDecodePolicy>,
}

impl BatchBody {
    /// The request's own headers, which a forwarded copy of the operation
    /// replays (clause 6.3.19).
    #[must_use]
    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// How the items are decoded beside the executor once their contexts
    /// were admitted, when the stack published it.
    #[must_use]
    pub const fn blocking_decode(&self) -> Option<&BlockingDecodePolicy> {
        self.blocking_decode.as_ref()
    }
}

impl<S> FromRequest<S> for BatchBody
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<BatchBody, Self::Rejection> {
        let uri = addressed_request_uri(&req);
        let (mut parts, body) = req.into_parts();
        // Clause 6.3.4 checks both preconditions on a POST, and answers an
        // absent Content-Length with a bare 411 before the Content-Type is
        // looked at: a request that declares no payload length has declared no
        // payload for the media type to describe.
        let declared = declared_body_length(&parts.headers, &parts.extensions).map_err(|error| error.into_rejection(uri.clone()).into_response())?; // clone: the media-type refusal below names the same request
        let content_kind = admit_content_kind(&parts.headers, &parts.method).map_err(IntoResponse::into_response)?;
        // Clause 6.3.5: an `application/ld+json` body names its context in the
        // body, so "the presence of a JSON-LD Link header in the incoming HTTP
        // request when the Content-Type header is application/ld+json shall
        // result in an HTTP error response of type BadRequestData"; any other
        // body is read against the request's one context `Link`, which is
        // validated here like every other request's.
        let carrier = match content_kind {
            ContentKind::LdJson => PayloadCarrier::LdJsonBody,
            ContentKind::Json | ContentKind::MergePatchJson => PayloadCarrier::LinkHeader,
        };
        let link_context = match RequestLinks::of(&mut parts).and_then(|links| links.context_for(carrier)) {
            Ok(link_context) => ContextSpec::from_request(link_context.map_or(RequestContext::Default, RequestContext::Link)),
            Err(error) => return Err(classify_context_link_error(&error).with_instance(uri).into_response()),
        };
        // Move headers out before reassembling the request for body extraction:
        // the payload reader looks only at the declared length it is handed and
        // the size limit the request's extensions carry, never at a header.
        let headers = take(&mut parts.headers);
        let value_limit = parts.extensions.get::<RequestJsonValueLimit>().copied();
        let decode_bounds = parts.extensions.get::<RequestDecodeBounds>().copied();
        let blocking_decode = parts.extensions.get::<BlockingDecodePolicy>().cloned(); // clone: the body owns its share of the permits
        let req = Request::from_parts(parts, body);
        let body = buffer_payload(req, state, declared, uri.clone()) // clone: an empty payload below names the same request
            .await
            .map_err(IntoResponse::into_response)?;
        // A declared length the payload did not carry leaves clause 5.5.4 with
        // no JSON document to read, which is the same refusal a declared length
        // of zero draws.
        if body.is_empty() {
            return Err(ContentLengthPreconditionError::Empty.into_rejection(uri).into_response());
        }
        Ok(BatchBody {
            headers,
            content_kind,
            link_context,
            body,
            value_limit,
            decode_bounds,
            blocking_decode,
        })
    }
}

/// What the batch rules decided of one item, beside the `@context` it
/// carried.
struct ItemAdmission {
    /// The item's own top-level `@context`, as written.
    own_context: Option<JsonLiteral>,
    /// The first `@context` the item embedded below its own top level, or the
    /// refusal the item drew.
    outcome: Result<Option<EmbeddedContext>, BatchItemRefusal>,
}

/// A parsed batch request, its items not yet decoded.
#[derive(Debug)]
pub struct ParsedBatch {
    /// The items that passed the batch rules, each with its own context, in
    /// submission order.
    pub items: Vec<BatchItem>,
    /// The context the request as a whole is answered and forwarded under:
    /// the one every item shares, or the default context when the items of an
    /// `application/ld+json` batch name different ones.
    pub context: ContextSpec,
    /// The context the request itself named for every item, its `Link` header
    /// context or the default one: present for an `application/json` batch,
    /// absent for an `application/ld+json` one, whose items each name their own.
    pub shared_context: Option<BatchContext>,
    /// The items refused before their context is known, each against its own
    /// entity.
    pub errors: Vec<BatchEntityError>,
}

/// One batch array item: the slice of text it is, and the members at its top
/// the batch rules ask about.
struct BatchArrayItem<'b> {
    /// The item as written.
    text: &'b RawValue,
    /// The item at its top level.
    envelope: BodyEnvelope,
}

impl<'b> BatchArrayItem<'b> {
    /// Read the top level of the item `text`.
    ///
    /// # Errors
    ///
    /// Returns the [`BatchBodyRefusal`] of an item that is no JSON, which an
    /// item of a body read as a JSON array never is.
    fn read(text: &'b RawValue) -> Result<BatchArrayItem<'b>, BatchBodyRefusal> {
        let envelope = BodyEnvelope::read(text.get().as_bytes()).map_err(|source| BatchBodyRefusal::NotJson(NotJsonDocument(source)))?;
        Ok(BatchArrayItem { text, envelope })
    }

    /// Admit the item and read the `@context` member it carried.
    ///
    /// The outcome is the [`BatchItemRefusal`] when the item carries a number
    /// the model cannot represent or a string the broker cannot hold, or
    /// carries an `@context` its representation forbids or lacks one it
    /// requires (clause 6.3.5). Clause 5.6.7.4 reports these per entity,
    /// leaving the rest of the batch intact. Whether the item is an entity is
    /// judged once the context it names was admitted.
    fn admit(self, content_kind: ContentKind) -> ItemAdmission {
        // The context is carried beside the entity, not decoded into it.
        let own_context = match self.envelope {
            BodyEnvelope::Object(object) => object.into_context(),
            BodyEnvelope::Array(_) | BodyEnvelope::Null | BodyEnvelope::Scalar => None,
        };
        let outcome = admitted_text(self.text.get().as_bytes()).and_then(|embedded| match (content_kind, &own_context) {
            (ContentKind::Json | ContentKind::MergePatchJson, Some(_)) => Err(BatchItemRefusal::ContextInJson),
            (ContentKind::LdJson, None) => Err(BatchItemRefusal::NoContextInLdJson),
            (ContentKind::Json | ContentKind::MergePatchJson, None) | (ContentKind::LdJson, Some(_)) => Ok(embedded),
        });
        ItemAdmission { own_context, outcome }
    }
}

/// The first `@context` the item `text` embeds below its own top level.
///
/// # Errors
///
/// Returns the [`BatchItemRefusal`] of a number the model cannot represent or
/// a string the broker cannot hold.
fn admitted_text(text: &[u8]) -> Result<Option<EmbeddedContext>, BatchItemRefusal> {
    admit_json_text(text)?;
    Ok(find_embedded_context(text).map_err(JsonTextRefusal::Syntax)?)
}

/// Parse a batch request body with per-entity error handling.
///
/// Every item comes back as its text, with the context it is read against: its
/// own `@context` member for an `application/ld+json` batch, the request's
/// `Link` context for an `application/json` one. A body at or above the
/// published decode threshold is parsed on the blocking pool.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the request fails as a whole, for one of
/// the [`BatchBodyRefusal`]s: a body holding more JSON values than the request
/// limit admits, an unparsable body, a body that is not a non-empty JSON array,
/// a null array item, or an item writing as `id` or `@id` a value that is not
/// a URI; and an `InternalError`
/// when the blocking decoding task fails. Every other failure, an `id` too long
/// to be stored among them, is reported per entity in the returned
/// [`BatchEntityError`] list, leaving the rest of the batch intact. The clause
/// 6.3.4 media-type and payload-length refusals are not among them:
/// [`BatchBody`] raised those before this was reached.
pub async fn parse_batch_entities(batch: &BatchBody, uri: &HttpUri) -> Result<ParsedBatch, ProblemResponse> {
    let body = batch.body.clone(); // clone: a reference-count bump; the decode may run on the blocking pool
    let link_context = batch.link_context.clone(); // clone: the decode may run on the blocking pool, which borrows nothing
    let (content_kind, value_limit, decode_bounds) = (batch.content_kind, batch.value_limit, batch.decode_bounds);
    let instance = uri.clone(); // clone: the decode's refusals name the request it answers
    decode_payload(batch.blocking_decode.as_ref(), body.len(), move || {
        decode_batch(&body, content_kind, link_context, value_limit, decode_bounds).map_err(|refusal| refusal.into_problem(instance))
    })
    .await
    .map_err(|failure| failure.report().with_instance(uri.clone()))? // clone: the refusal names the request it answers
}

/// Decode a batch body read under `content_kind` into its entities, each with
/// the context it is read against.
///
/// # Errors
///
/// Returns the [`BatchBodyRefusal`] that fails the request as a whole.
fn decode_batch(
    body: &Bytes,
    content_kind: ContentKind,
    link_context: ContextSpec,
    value_limit: Option<RequestJsonValueLimit>,
    decode_bounds: Option<RequestDecodeBounds>,
) -> Result<ParsedBatch, BatchBodyRefusal> {
    // Counted before anything is built, so a body of too many values costs
    // its bytes and nothing more.
    refuse_values_past_budget(body, value_limit)?;
    let items = batch_items::<&RawValue>(body, BatchMembers::Entities)?;

    let shared_context = match content_kind {
        ContentKind::Json | ContentKind::MergePatchJson => Some(BatchContext::shared(link_context)),
        ContentKind::LdJson => None,
    };
    // clone: the batch and every item it reads hold the shared handle
    let mut contexts = shared_context.clone().map_or_else(BatchContexts::per_entity, BatchContexts::shared);
    let mut admitted = Vec::new();
    let mut errors = Vec::new();

    for (index, item) in items.into_iter().enumerate() {
        let item = BatchArrayItem::read(item)?;
        let written = written_identifier(&item.envelope, index)?;
        let text = body.slice_ref(item.text.get().as_bytes());
        let ItemAdmission { own_context, outcome } = item.admit(content_kind);
        let (identifier, embedded) = match written {
            Some(entity_id) => match IdentifierTooLong::check(entity_id.as_str()).map_err(BatchItemRefusal::from).and(outcome) {
                Ok(embedded) => (ItemIdentifier::Written(entity_id), embedded),
                Err(refusal) => {
                    errors.push(refusal.against(entity_id));
                    continue;
                }
            },
            // Clause 4.4: an `id` written under neither spelling can only be a
            // term the item's context defines for `@id`, so the item is kept
            // until that context was admitted, and any refusal is held for the
            // `id` it reads as.
            None => match outcome {
                Ok(embedded) => (ItemIdentifier::Aliased { index, held: None }, embedded),
                Err(refusal) => (ItemIdentifier::Aliased { index, held: Some(refusal) }, None),
            },
        };
        admitted.push(BatchItem {
            identifier,
            text,
            context: contexts.context_of(own_context),
            embedded,
            decode_bounds,
        });
    }

    Ok(ParsedBatch {
        items: admitted,
        context: contexts.request_context(),
        shared_context,
        errors,
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        batch_item_decoding::{BatchItem, ItemIdentifier, entity_of},
        batch_item_refusal::BatchItemRefusal,
        blocking_decode::BlockingDecodePolicy,
        entity_batch::{BatchBody, ParsedBatch, parse_batch_entities},
        request_json_value_limit::RequestJsonValueLimit,
    };
    use andromeda_http_response::problem_response::ProblemResponse;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_lexicon::{identifier_length::IDENTIFIER_BYTE_LIMIT, uri::Uri as EntityUri};
    use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_problem_details::{
        batch_operation::{BatchEntityError, BatchErrorKind},
        error_type::NgsiLdErrorType,
        problem_detail::ProblemClassification,
    };
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use axum::{
        body::{Body, to_bytes},
        extract::FromRequest,
        http::{
            Request,
            StatusCode,
            Uri,
            header::{CONTENT_LENGTH, CONTENT_TYPE},
        },
    };
    use iri_rs::IriBuf;
    use serde_json::json;
    use std::num::NonZeroUsize;

    /// The URI every fixture request addresses.
    fn batch_uri() -> Uri {
        Uri::from_static("/ngsi-ld/v1/entityOperations/create")
    }

    /// The `id` an item wrote as `id` or `@id`.
    fn written(identifier: &ItemIdentifier) -> &EntityUri {
        match identifier {
            ItemIdentifier::Written(entity_id) => entity_id,
            ItemIdentifier::Aliased { index, .. } => panic!("item {index} wrote its id under neither spelling"),
        }
    }

    /// The entities and per-entity errors a batch decodes into, read against
    /// the default context: the refusals held until an item's context is
    /// admitted follow the ones raised before it.
    async fn parsed(batch: &BatchBody) -> (Vec<Entity>, Vec<BatchEntityError>) {
        let ParsedBatch { items, mut errors, .. } = parse_batch_entities(batch, &batch_uri()).await.expect("batch parses");
        let mut entities = Vec::new();
        for BatchItem { identifier, text, .. } in items {
            match entity_of(&text, ContextReading::keyword_spellings(), None) {
                Ok(entity) => entities.push(entity),
                Err(refusal) => errors.push(refusal.against(written(&identifier).clone())), // clone: the fixture keeps the item it reports
            }
        }
        (entities, errors)
    }

    /// The batch body an `application/json` request carries, as the extractor
    /// admits it: the clause 6.3.4 preconditions are the extractor's, so a
    /// fixture reaches [`parse_batch_entities`] only by passing them.
    async fn json_batch(body: &str) -> BatchBody {
        batch_of("application/json", body).await
    }

    /// The batch body a request of `content_type` carries, as the extractor
    /// admits it.
    async fn batch_of(content_type: &str, body: &str) -> BatchBody {
        admitted_batch(
            Request::builder()
                .uri(batch_uri())
                .header(CONTENT_TYPE, content_type)
                .header(CONTENT_LENGTH, body.len())
                .body(Body::from(body.to_owned()))
                .expect("request builds"),
        )
        .await
    }

    /// The batch body `request` carries, as the extractor admits it.
    async fn admitted_batch(request: Request<Body>) -> BatchBody {
        match BatchBody::from_request(request, &()).await {
            Ok(batch) => batch,
            Err(refusal) => panic!("the fixture meets the clause 6.3.4 preconditions, got {}", refusal.status()),
        }
    }

    #[tokio::test]
    async fn a_malformed_attribute_key_rejects_only_its_own_entity() {
        // NGSI-LD clause 5.6.7: batch creation reports failures per entity, so
        // one entity whose attribute key is not a valid NGSI-LD name must not
        // take the rest of the batch with it.
        let batch = json_batch(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":10}},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","bad name":{"type":"Property","value":20}},{"id":"urn:ngsi-ld:Vehicle:3","type":"Vehicle","speed":{"type":"Property","value":30}}]"#,
        )
        .await;
        let (entities, errors) = parsed(&batch).await;

        assert_eq!(entities.len(), 2, "{errors:?}");
        pretty_assertions::assert_eq!(
            entities.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1".to_string(), "urn:ngsi-ld:Vehicle:3".to_string()]
        );
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
    }

    /// Clause 4.4: an item spelling its `id` as `@id` is reported against that
    /// `id` when it is no entity.
    #[tokio::test]
    async fn a_batch_item_id_spelled_at_id_names_its_error() {
        let batch = json_batch(r#"[{"@id":"urn:ngsi-ld:Vehicle:1","@type":"Vehicle"},{"@id":"urn:ngsi-ld:Vehicle:2","@type":5}]"#).await;
        let (entities, errors) = parsed(&batch).await;
        pretty_assertions::assert_eq!(
            entities.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1"]
        );
        pretty_assertions::assert_eq!(
            errors.iter().map(|error| error.entity_id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2"]
        );
    }

    /// Clause 5.5.7: an item's own top-level `@context` is admitted, and the
    /// first one it embeds deeper is carried with that item alone, located
    /// from the item's own top level.
    #[tokio::test]
    async fn an_item_carries_the_context_it_embedded_and_its_sibling_none() {
        let batch = batch_of(
            "application/ld+json",
            r#"[{"@context":"https://example.org/a","id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":1,"@context":{}}},{"@context":"https://example.org/a","id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle"}]"#,
        )
        .await;
        let ParsedBatch { items, errors, .. } = parse_batch_entities(&batch, &batch_uri()).await.expect("batch parses");
        assert!(errors.is_empty(), "{errors:?}");
        let findings: Vec<(String, Option<String>)> = items
            .iter()
            .map(|item| {
                (
                    written(&item.identifier).to_string(),
                    item.embedded.as_ref().map(|found| found.path.to_string()),
                )
            })
            .collect();
        pretty_assertions::assert_eq!(
            findings,
            vec![
                ("urn:ngsi-ld:Vehicle:1".to_owned(), Some("/speed".to_owned())),
                ("urn:ngsi-ld:Vehicle:2".to_owned(), None),
            ]
        );
    }

    #[tokio::test]
    async fn a_null_batch_array_item_is_bad_request_data() {
        // ETSI GS CIM 009 v1.9.1 clauses 5.6.7 to 5.6.10: a null array item is
        // malformed batch content and fails the whole request as BadRequestData,
        // not the InvalidRequest a missing id raises.
        let batch = json_batch(r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle"},null]"#).await;
        let error = parse_batch_entities(&batch, &batch_uri()).await.expect_err("a null item fails the request");
        pretty_assertions::assert_eq!(error.problem().error_type(), NgsiLdErrorType::BadRequestData);
    }

    /// A URN of exactly `length` octets.
    fn urn_of_length(length: usize) -> String {
        let prefix = "urn:ngsi-ld:Vehicle:";
        format!("{prefix}{}", "a".repeat(length - prefix.len()))
    }

    /// The whole-request refusal a batch body draws.
    async fn request_refusal(body: &str) -> ProblemResponse {
        let batch = json_batch(body).await;
        match parse_batch_entities(&batch, &batch_uri()).await {
            Ok(parsed) => panic!(
                "the request was to fail as a whole, parsed {} entities and {:?}",
                parsed.items.len(),
                parsed.errors
            ),
            Err(refusal) => refusal,
        }
    }

    /// Table 5.2.17-1 sets no length on `entityId`, and clause 5.6.7.4 reports
    /// a failed entity against its Entity ID: an `id` too long to be stored is
    /// that entity's failure, and its neighbours are decoded.
    #[tokio::test]
    async fn an_over_long_id_is_refused_against_itself_and_its_neighbours_decode() {
        let long_id = urn_of_length(IDENTIFIER_BYTE_LIMIT + 1);
        let body = format!(
            r#"[{{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle"}},{{"id":"{long_id}","type":"Vehicle"}},{{"id":"urn:ngsi-ld:Vehicle:C","type":"Vehicle"}}]"#
        );
        let batch = json_batch(&body).await;
        let (entities, errors) = parsed(&batch).await;

        pretty_assertions::assert_eq!(
            entities.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:A".to_string(), "urn:ngsi-ld:Vehicle:C".to_string()]
        );
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), long_id.as_str());
        pretty_assertions::assert_eq!(
            errors[0].error.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::BadRequestData))
        );
        let detail = &errors[0].error.detail;
        assert!(
            detail.contains("'id'") && detail.contains("1025 bytes") && detail.contains("1024 bytes"),
            "the refusal names the member and the bound: {detail}"
        );
    }

    /// Clause 6.14.3.1: a batch in which no entity succeeds still answers per
    /// entity, so a lone over-long `id` does not fail the request.
    #[tokio::test]
    async fn a_batch_of_one_over_long_id_parses_to_one_error() {
        let body = format!(r#"[{{"id":"{}","type":"Vehicle"}}]"#, urn_of_length(IDENTIFIER_BYTE_LIMIT + 1));
        let batch = json_batch(&body).await;
        let (entities, errors) = parsed(&batch).await;
        assert!(entities.is_empty());
        pretty_assertions::assert_eq!(errors.len(), 1);
    }

    /// Table 5.2.17-1 requires `entityId` to be a valid URI, so an item whose
    /// `id` is not one has nothing to be reported against and fails the
    /// request; clause 5.5.4 makes that `BadRequestData`, not `InvalidRequest`.
    #[tokio::test]
    async fn an_id_that_is_not_a_uri_fails_the_request_with_bad_request_data_naming_the_item() {
        let refusal = request_refusal(r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle"},{"id":"not a uri","type":"Vehicle"}]"#).await;
        pretty_assertions::assert_eq!(refusal.problem().error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(
            refusal.problem().detail(),
            "the 'id' of batch item 1 is not a URI: invalid URI: not a valid URN, URL or IRI"
        );
    }

    /// Clause 4.4: an item writing no `id` string as `id` or `@id` may write
    /// it under a term its context defines for `@id`, so it is kept, by its
    /// position, until that context was admitted.
    #[tokio::test]
    async fn an_item_without_an_id_string_is_kept_until_its_context_reads_its_id() {
        for body in [
            r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle"},{"type":"Vehicle"}]"#,
            r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle"},{"id":7,"type":"Vehicle"}]"#,
        ] {
            let ParsedBatch { items, errors, .. } = parse_batch_entities(&json_batch(body).await, &batch_uri()).await.expect("batch parses");
            assert!(errors.is_empty(), "{body}: {errors:?}");
            pretty_assertions::assert_eq!(written(&items[0].identifier).as_str(), "urn:ngsi-ld:Vehicle:A", "{body}");
            assert!(
                matches!(items[1].identifier, ItemIdentifier::Aliased { index: 1, held: None }),
                "{body}: {:?}",
                items[1].identifier
            );
        }
    }

    /// Clause 5.6.7.4: a refusal an item draws before its `id` is known is
    /// held for the `id` its context reads.
    #[tokio::test]
    async fn a_refusal_of_an_item_without_an_id_string_is_held_for_its_id() {
        let body = r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle"},{"eid":"urn:ngsi-ld:Vehicle:B","type":"Vehicle","@context":{"eid":"@id"}}]"#;
        let ParsedBatch { items, errors, .. } = parse_batch_entities(&json_batch(body).await, &batch_uri()).await.expect("batch parses");
        assert!(errors.is_empty(), "{errors:?}");
        assert!(
            matches!(
                items[1].identifier,
                ItemIdentifier::Aliased {
                    index: 1,
                    held: Some(BatchItemRefusal::ContextInJson)
                }
            ),
            "{:?}",
            items[1].identifier
        );
    }

    /// Clause 5.5.4: only a body that is not a JSON document is
    /// `InvalidRequest`; a JSON document of the wrong shape is `BadRequestData`.
    #[tokio::test]
    async fn the_body_shape_refusals_carry_the_clause_5_5_4_categories() {
        for (body, expected, detail) in [
            ("[,", NgsiLdErrorType::InvalidRequest, None),
            (
                "[]",
                NgsiLdErrorType::BadRequestData,
                Some("the request body must be a non-empty JSON array of entities"),
            ),
            ("{}", NgsiLdErrorType::BadRequestData, Some("the request body must be a JSON array of entities")),
        ] {
            let refusal = request_refusal(body).await;
            pretty_assertions::assert_eq!(refusal.problem().error_type(), expected, "{body}");
            if let Some(detail) = detail {
                pretty_assertions::assert_eq!(refusal.problem().detail(), detail, "{body}");
            }
        }
    }

    /// Clause 6.3.5 governs where the `@context` of each item lives; a
    /// misplaced or missing one is that item's failure.
    #[tokio::test]
    async fn a_misplaced_or_missing_context_refuses_only_its_own_item() {
        for (content_type, body, detail) in [
            (
                "application/json",
                r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle"},{"id":"urn:ngsi-ld:Vehicle:B","type":"Vehicle","@context":"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld"}]"#,
                "an application/json batch item must not contain @context",
            ),
            (
                "application/ld+json",
                r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle","@context":"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld"},{"id":"urn:ngsi-ld:Vehicle:B","type":"Vehicle"}]"#,
                "an application/ld+json batch item must contain @context",
            ),
        ] {
            let batch = batch_of(content_type, body).await;
            let (entities, errors) = parsed(&batch).await;
            pretty_assertions::assert_eq!(
                entities.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
                vec!["urn:ngsi-ld:Vehicle:A".to_string()],
                "{content_type}"
            );
            pretty_assertions::assert_eq!(errors.len(), 1, "{content_type}");
            pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:B", "{content_type}");
            pretty_assertions::assert_eq!(errors[0].error.detail, detail, "{content_type}");
        }
    }

    #[tokio::test]
    async fn exact_numbers_survive_and_range_failures_remain_per_entity() {
        let batch = json_batch(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":0.100000000000000005}},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","json":{"type":"JsonProperty","json":{"nested":1e-16384}}},{"id":"urn:ngsi-ld:Vehicle:3","type":"Vehicle","speed":{"type":"Property","value":18446744073709551616}}]"#,
        )
        .await;
        let (entities, errors) = parsed(&batch).await;

        assert_eq!(entities.len(), 2, "{errors:?}");
        let encoded = serde_json::to_string(&entities).expect("exact entities");
        assert!(encoded.contains("0.100000000000000005"), "{encoded}");
        assert!(encoded.contains("18446744073709551616"), "{encoded}");
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
        assert!(errors[0].error.detail.contains("/json/json/nested"), "{}", errors[0].error.detail);
    }

    /// Clause 5.6.8.4 reports a failing item per entity, and clause 5.5.4 makes
    /// an object naming one member twice `BadRequestData`: the item that
    /// repeats a member is refused against its own `id`, naming the member,
    /// and its neighbours are decoded.
    #[tokio::test]
    async fn an_item_naming_a_member_twice_is_refused_alone_naming_the_member() {
        let batch = json_batch(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":10}},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","speed":{"type":"Property","value":20},"speed":{"type":"Property","value":21}},{"id":"urn:ngsi-ld:Vehicle:3","type":"Vehicle","speed":{"type":"Property","value":30}}]"#,
        )
        .await;
        let (entities, errors) = parsed(&batch).await;

        assert_eq!(entities.len(), 2, "{errors:?}");
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
        pretty_assertions::assert_eq!(
            errors[0].error.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::BadRequestData))
        );
        assert!(
            errors[0].error.detail.contains("the member at /speed is named twice"),
            "{}",
            errors[0].error.detail
        );
    }

    /// Clause 5.6.7.4 reports a failing item per entity: the item holding
    /// U+0000 is refused against its own `id`, naming where the character
    /// stands, and its neighbours are decoded.
    #[tokio::test]
    async fn an_item_holding_u0000_is_refused_alone_naming_where_it_stands() {
        let batch = json_batch(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","name":{"type":"Property","value":"a\u0000b"}},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","name":{"type":"Property","value":"ab"}}]"#,
        )
        .await;
        let (entities, errors) = parsed(&batch).await;

        assert_eq!(entities.len(), 1, "{errors:?}");
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:1");
        pretty_assertions::assert_eq!(
            errors[0].error.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::BadRequestData))
        );
        assert!(errors[0].error.detail.contains("/name/value"), "{}", errors[0].error.detail);
        assert!(errors[0].error.detail.contains("U+0000"), "{}", errors[0].error.detail);
    }

    /// JSON-LD applies a top-level node's `@context` to that node alone, so each
    /// item of an `application/ld+json` batch is read against its own context,
    /// and items naming the same one share it.
    #[tokio::test]
    async fn each_ld_json_batch_item_is_read_against_its_own_context() {
        let batch = batch_of(
            "application/ld+json",
            r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle","@context":{"speed":"https://a.example/speed"}},{"id":"urn:ngsi-ld:Vehicle:B","type":"Vehicle","@context":{"speed":"https://b.example/speed"}},{"id":"urn:ngsi-ld:Vehicle:C","type":"Vehicle","@context":{"speed":"https://a.example/speed"}}]"#,
        )
        .await;

        let ParsedBatch {
            items,
            context,
            shared_context,
            errors,
        } = parse_batch_entities(&batch, &batch_uri()).await.expect("batch parses");

        assert!(errors.is_empty(), "{errors:?}");
        assert!(shared_context.is_none(), "the items of an ld+json batch each name their own context");
        pretty_assertions::assert_eq!(
            items[0].context.spec().body_context(),
            Some(&JsonLiteral::admit(&json!({"speed": "https://a.example/speed"}).to_string()).expect("an admissible literal"))
        );
        pretty_assertions::assert_eq!(
            items[1].context.spec().body_context(),
            Some(&JsonLiteral::admit(&json!({"speed": "https://b.example/speed"}).to_string()).expect("an admissible literal"))
        );
        pretty_assertions::assert_eq!(items[0].context.id(), items[2].context.id(), "items naming one context share it");
        pretty_assertions::assert_ne!(items[0].context.id(), items[1].context.id());
        pretty_assertions::assert_eq!(context, ContextSpec::none(), "items naming several contexts leave the request the default one");
        assert!(items.iter().all(|item| written(&item.identifier).as_str().starts_with("urn:ngsi-ld:Vehicle:")));
    }

    /// Items naming one and the same context make it the request's context
    /// too, as written: repeats and order included.
    #[tokio::test]
    async fn an_ld_json_batch_whose_items_agree_is_read_against_their_context() {
        let batch = batch_of(
            "application/ld+json",
            r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle","@context":["https://a.example/c.jsonld","https://b.example/c.jsonld","https://a.example/c.jsonld"]},{"id":"urn:ngsi-ld:Vehicle:B","type":"Vehicle","@context":["https://a.example/c.jsonld","https://b.example/c.jsonld","https://a.example/c.jsonld"]}]"#,
        )
        .await;

        let ParsedBatch { items, context, .. } = parse_batch_entities(&batch, &batch_uri()).await.expect("batch parses");

        let written = JsonLiteral::admit(&json!(["https://a.example/c.jsonld", "https://b.example/c.jsonld", "https://a.example/c.jsonld"]).to_string())
            .expect("an admissible literal");
        pretty_assertions::assert_eq!(context.body_context(), Some(&written));
        pretty_assertions::assert_eq!(items[0].context.id(), items[1].context.id());
    }

    /// The items of an `application/json` batch share the request's `Link`
    /// context.
    #[tokio::test]
    async fn json_batch_items_share_the_link_context() {
        let body = r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle"},{"id":"urn:ngsi-ld:Vehicle:B","type":"Vehicle"}]"#;
        let batch = admitted_batch(
            Request::builder()
                .uri(batch_uri())
                .header(CONTENT_TYPE, "application/json")
                .header(CONTENT_LENGTH, body.len())
                .header("Link", r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#)
                .body(Body::from(body))
                .expect("request builds"),
        )
        .await;
        let link = ContextSpec::from_link(IriBuf::new("https://example.com/ctx.jsonld".to_owned()).expect("a valid IRI"));

        let ParsedBatch {
            items,
            context,
            shared_context,
            ..
        } = parse_batch_entities(&batch, &batch_uri()).await.expect("batch parses");

        pretty_assertions::assert_eq!(context, link);
        assert!(items.iter().all(|item| *item.context.spec() == link));
        pretty_assertions::assert_eq!(items[0].context.id(), items[1].context.id());
        let shared_context = shared_context.expect("an application/json batch shares the request's context");
        pretty_assertions::assert_eq!(shared_context.spec(), &link);
        pretty_assertions::assert_eq!(shared_context.id(), items[0].context.id(), "every item is read under the shared context");
    }

    /// Clause 6.3.5: a context `Link` beside an `application/ld+json` batch is
    /// `BadRequestData`, a `profile` link is not.
    #[tokio::test]
    async fn an_ld_json_batch_beside_a_context_link_is_bad_request_data() {
        let body = r#"[{"id":"urn:ngsi-ld:Vehicle:A","type":"Vehicle","@context":"https://example.com/ctx.jsonld"}]"#;
        let refused = Request::builder()
            .uri(batch_uri())
            .header(CONTENT_TYPE, "application/ld+json")
            .header(CONTENT_LENGTH, body.len())
            .header("Link", r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#)
            .body(Body::from(body))
            .expect("request builds");
        let (status, problem) = batch_refusal(refused).await;
        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
        let problem: serde_json::Value = serde_json::from_slice(&problem).expect("a problem details body");
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");

        let admitted = Request::builder()
            .uri(batch_uri())
            .header(CONTENT_TYPE, "application/ld+json")
            .header(CONTENT_LENGTH, body.len())
            .header("Link", r#"<https://uri.etsi.org/ngsi-ld/v1.9>; rel="profile""#)
            .body(Body::from(body))
            .expect("request builds");
        assert!(BatchBody::from_request(admitted, &()).await.is_ok(), "a profile link is admitted");
    }

    /// Clause 5.5.4: a malformed `Link` refuses a batch as it does any request.
    #[tokio::test]
    async fn a_malformed_link_refuses_a_batch() {
        let request = Request::builder()
            .uri(batch_uri())
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, 2)
            .header("Link", "not a link")
            .body(Body::from("[]"))
            .expect("request builds");
        pretty_assertions::assert_eq!(batch_refusal(request).await.0, StatusCode::BAD_REQUEST);
    }

    /// Clause 6.3.4: "Unsupported Media Type, i.e. 'Content-Type' header is not
    /// 'application/json' nor 'application/ld+json', shall result in just a 415
    /// HTTP status code (without any payload body)". The media type is decided
    /// once, here, so nothing below can answer it with a problem detail.
    #[tokio::test]
    async fn an_unsupported_media_type_is_the_bare_415_of_clause_6_3_4() {
        let request = Request::builder()
            .uri(batch_uri())
            .header(CONTENT_TYPE, "application/xml")
            .header(CONTENT_LENGTH, 8)
            .body(Body::from("<batch/>"))
            .expect("request builds");

        let (status, body) = batch_refusal(request).await;

        pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert!(body.is_empty(), "clause 6.3.4 forbids a payload body on a 415: {body:?}");
    }

    /// The status and body a batch request is refused with before its payload
    /// is read.
    async fn batch_refusal(request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = BatchBody::from_request(request, &()).await.err().expect("the request is refused");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap_or_default();
        (status, body.to_vec())
    }

    /// Clause 6.3.4: "'Content-Length' HTTP header absence, shall result in
    /// just a 411 HTTP status code (without any payload body)". A batch
    /// operation is an HTTP POST, so the precondition applies to it exactly as
    /// it does to a single-entity POST.
    #[tokio::test]
    async fn a_batch_request_declaring_no_length_is_the_bare_411_of_clause_6_3_4() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("[]"))
            .expect("request builds");

        let (status, body) = batch_refusal(request).await;

        pretty_assertions::assert_eq!(status, StatusCode::LENGTH_REQUIRED);
        assert!(body.is_empty(), "clause 6.3.4 forbids a payload body on a 411: {body:?}");
    }

    /// Clause 6.3.4 raises the 411 before the `Content-Type` precondition, so a
    /// request that meets neither is answered by the length it did not declare.
    #[tokio::test]
    async fn an_unsupported_media_type_without_a_length_is_still_the_411() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/xml")
            .body(Body::from("<batch/>"))
            .expect("request builds");

        pretty_assertions::assert_eq!(batch_refusal(request).await.0, StatusCode::LENGTH_REQUIRED);
    }

    /// Clause 6.3.4: an unsupported `Content-Type` "shall result in just a 415
    /// HTTP status code (without any payload body)" once the length is declared.
    #[tokio::test]
    async fn a_declared_length_leaves_the_unsupported_media_type_a_bare_415() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/xml")
            .header(CONTENT_LENGTH, "8")
            .body(Body::from("<batch/>"))
            .expect("request builds");

        let (status, body) = batch_refusal(request).await;

        pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert!(body.is_empty(), "clause 6.3.4 forbids a payload body on a 415: {body:?}");
    }

    /// Clause 5.5.4: a declared zero-length payload body is not a valid JSON
    /// document, so it is `InvalidRequest` rather than the 411 of an absent
    /// header.
    #[tokio::test]
    async fn a_batch_request_declaring_a_zero_length_is_an_invalid_request() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "0")
            .body(Body::empty())
            .expect("request builds");

        let (status, body) = batch_refusal(request).await;

        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
        let problem: serde_json::Value = serde_json::from_slice(&body).expect("a problem details body");
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
    }

    /// A batch request that meets both preconditions reaches its payload.
    #[tokio::test]
    async fn a_batch_request_meeting_the_preconditions_is_extracted() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "2")
            .body(Body::from("[]"))
            .expect("request builds");

        let extracted = BatchBody::from_request(request, &()).await.expect("both preconditions are met");

        pretty_assertions::assert_eq!(extracted.body.as_ref(), b"[]".as_slice());
    }

    /// Clause 6.3.2: a batch body holding more JSON values than the published
    /// limit is too large as a request, so the request is refused whole with
    /// the 413 rather than entity by entity.
    #[tokio::test]
    async fn a_batch_body_past_the_json_value_limit_is_refused_whole() {
        let body =
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle"},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","n":{"type":"Property","value":[1,2,3]}}]"#;
        let mut request = Request::builder()
            .uri(batch_uri())
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, body.len())
            .body(Body::from(body))
            .expect("request builds");
        let ten = NonZeroUsize::new(10).expect("ten is not zero");
        request.extensions_mut().insert(RequestJsonValueLimit::new(JsonValueLimit::new(ten)));
        let batch = admitted_batch(request).await;

        let refusal = match parse_batch_entities(&batch, &batch_uri()).await {
            Ok(parsed) => panic!(
                "the request was to fail as a whole, parsed {} entities and {:?}",
                parsed.items.len(),
                parsed.errors
            ),
            Err(refusal) => refusal,
        };

        pretty_assertions::assert_eq!(refusal.problem().error_type(), NgsiLdErrorType::RequestEntityTooLarge);
        pretty_assertions::assert_eq!(refusal.problem().status_code(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(refusal.problem().detail().contains("10 JSON values"), "{}", refusal.problem().detail());
        pretty_assertions::assert_eq!(
            refusal.problem().instance().map(ToString::to_string).as_deref(),
            Some("/ngsi-ld/v1/entityOperations/create")
        );
    }

    /// A batch decoded on the blocking pool is the batch decoded inline: the
    /// same entities, the same per-entity errors, the same refusal of a body
    /// that fails as a whole.
    #[tokio::test]
    async fn a_batch_is_decoded_alike_on_both_decode_paths() {
        let bodies = [
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","n":{"type":"Property","value":[0.100000000000000005,18446744073709551616]}},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle"}]"#,
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle"},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","bad key":{"type":"Property","value":1}}]"#,
            "[1,",
            "{}",
        ];
        for body in bodies {
            let decoded = |threshold: usize| async move {
                let mut request = Request::builder()
                    .uri(batch_uri())
                    .header(CONTENT_TYPE, "application/json")
                    .header(CONTENT_LENGTH, body.len())
                    .body(Body::from(body))
                    .expect("request builds");
                request.extensions_mut().insert(BlockingDecodePolicy::new(threshold, NonZeroUsize::MIN));
                match parse_batch_entities(&admitted_batch(request).await, &batch_uri()).await {
                    Ok(ParsedBatch { items, errors, .. }) => Ok((
                        items
                            .iter()
                            .map(
                                |item| match entity_of::<AttributeValue>(&item.text, ContextReading::keyword_spellings(), None) {
                                    Ok(entity) => serde_json::to_value(entity).expect("an entity serializes"),
                                    Err(refusal) => serde_json::Value::String(refusal.to_string()),
                                },
                            )
                            .collect::<Vec<_>>(),
                        errors,
                    )),
                    Err(refusal) => Err((refusal.problem().status_code(), refusal.problem().detail().to_owned())),
                }
            };
            pretty_assertions::assert_eq!(decoded(0).await, decoded(usize::MAX).await, "{body}");
        }
    }
}
