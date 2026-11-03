//! Admitting an NGSI-LD request body from its text alone.
//!
//! Every body extractor applies the same preconditions before it decodes
//! anything: clauses 6.3.4 and 6.3.5 on the headers, the JSON value limit and
//! the number range on the text, and the clause 6.3.5 rules on the top-level
//! `@context`. The values are counted, the numbers checked and the context read
//! without building the body, and the body is handed on as its text, decoded
//! straight into its domain type once its context was admitted.

use crate::{
    addressed_uri::addressed_request_uri,
    blocking_decode::{BlockingDecodePolicy, decode_payload},
    body_admission_refusal::BodyAdmissionRefusal,
    body_envelope::BodyEnvelope,
    classification::classify_context_link_error,
    content_kind::{ContentKind, admit_content_kind},
    content_length_precondition::{ContentLengthPreconditionError, declared_body_length},
    json_value_budget::refuse_values_past_budget,
    not_json_document::NotJsonDocument,
    request_geometry_position_limit::RequestGeometryPositionLimit,
    request_json_value_limit::RequestJsonValueLimit,
    request_links::{PayloadCarrier, RequestLinks},
    request_payload::buffer_payload,
};
use andromeda_http_response::{extraction_rejection::ExtractorRejection, request_refusal::RequestRefusal};
use andromeda_jsonld::{context_spec::ContextSpec, request_context::RequestContext, user_context_admission::SubmittedContext};
use andromeda_ngsi_ld_number::json_value::text_admission::admit_json_text;
use andromeda_ngsi_ld_representation::embedded_context_admission::find_embedded_context;
use axum::{body::Bytes, extract::Request, http::Uri};
use iri_rs::IriBuf;

/// A request body that passed the NGSI-LD body preconditions, with its
/// `@context` read.
pub(crate) struct AdmittedBody {
    /// The payload as received.
    pub(crate) text: Bytes,
    /// The extracted context specification from the body and/or Link header.
    pub(crate) context: ContextSpec,
    /// The request URI, the `instance` of any later rejection.
    pub(crate) uri: Uri,
    /// How the body is decoded beside the executor once its context was
    /// admitted, when the stack published it.
    pub(crate) blocking_decode: Option<BlockingDecodePolicy>,
    /// The most positions a geometry of the body may hold, when the stack
    /// published it.
    pub(crate) geometry_positions: Option<RequestGeometryPositionLimit>,
}

/// The user `@context` a client body submitted: the context it names and the
/// first `@context` it embeds below a document's top level (clause 5.5.7),
/// which the user `@context` admission refuses.
///
/// # Errors
///
/// Returns an `InvalidRequest` for a text that is no JSON, which the
/// preceding preconditions refuse first.
pub(crate) fn submitted_context(context: ContextSpec, text: &[u8], uri: &Uri) -> Result<SubmittedContext, ExtractorRejection> {
    let embedded = find_embedded_context(text).map_err(|error| NotJsonDocument(error).into_rejection(uri.clone()))?; // clone: the refusal names the request
    Ok(SubmittedContext::new(context, embedded))
}

/// Apply every NGSI-LD body precondition to `req` -- clauses 6.3.4 and 6.3.5,
/// the JSON value limit and the number range -- read its `@context`, and hand
/// the admitted body to `finish`, on the blocking pool when the payload is
/// large.
///
/// # Errors
///
/// Returns the refusal of the first precondition the request fails, or of
/// `finish`.
pub(crate) async fn admit_request_body<S: Send + Sync, T: Send + 'static>(
    req: Request,
    state: &S,
    finish: impl FnOnce(AdmittedBody) -> Result<T, ExtractorRejection> + Send + 'static,
) -> Result<T, ExtractorRejection> {
    read_body(req, state).await?.decode(finish).await
}

/// A request body that passed the clause 6.3.4 and 6.3.5 header
/// preconditions and was read in full, not yet parsed.
struct ReadBody {
    /// The payload as received.
    bytes: Bytes,
    /// The representation the request announced.
    content_kind: ContentKind,
    /// Where the payload's context is named.
    carrier: PayloadCarrier,
    /// The context `Link` the request carried, read before the body.
    link_context: Option<IriBuf>,
    /// The request URI, the `instance` of any later rejection.
    uri: Uri,
    /// The most JSON values the payload may hold, when the stack published it.
    value_limit: Option<RequestJsonValueLimit>,
    /// The most positions a geometry of the payload may hold, when the stack
    /// published it.
    geometry_positions: Option<RequestGeometryPositionLimit>,
    /// How the payload is decoded beside the executor, when the stack
    /// published it.
    blocking_decode: Option<BlockingDecodePolicy>,
}

impl ReadBody {
    /// Admit the payload, then hand the admitted body to `finish`, all of it
    /// on the blocking pool when the payload is large.
    ///
    /// # Errors
    ///
    /// Returns the refusal of [`ReadBody::admit`] or of `finish`, and an
    /// `InternalError` when the blocking task fails.
    async fn decode<T: Send + 'static>(
        self,
        finish: impl FnOnce(AdmittedBody) -> Result<T, ExtractorRejection> + Send + 'static,
    ) -> Result<T, ExtractorRejection> {
        let uri = self.uri.clone(); // clone: a failed decoding task still names the request
        let blocking_decode = self.blocking_decode.clone(); // clone: the decode moves the body it was read with
        decode_payload(blocking_decode.as_ref(), self.bytes.len(), move || self.admit().and_then(finish))
            .await
            .map_err(|failure| ExtractorRejection::Classified(failure.report().with_instance(uri.clone())))?
    }

    /// Count the payload's values against the request limit, check its
    /// numbers, and read its `@context`, all from its text.
    fn admit(self) -> Result<AdmittedBody, ExtractorRejection> {
        let ReadBody {
            bytes,
            content_kind,
            carrier,
            link_context,
            uri,
            value_limit,
            geometry_positions,
            blocking_decode,
        } = self;
        // Counted before anything is built, so a payload of too many values
        // costs its bytes and nothing more.
        // clone: each refusal names the request, which the admitted body keeps naming
        refuse_values_past_budget(&bytes, value_limit).map_err(|refusal| ExtractorRejection::Classified(refusal.report().with_instance(uri.clone())))?;
        // clone: as above
        admit_json_text(&bytes).map_err(|refusal| BodyAdmissionRefusal::from(refusal).into_rejection(uri.clone()))?;
        // clone: as above
        let envelope = BodyEnvelope::read(&bytes).map_err(|error| NotJsonDocument(error).into_rejection(uri.clone()))?;

        // An empty array is neither an NGSI-LD entity nor a batch payload.
        if envelope.is_empty_array() {
            return Err(BodyAdmissionRefusal::EmptyArray.into_rejection(uri));
        }

        // Clause 6.3.21: a profile `Link` announcing the payload's NGSI-LD
        // version is read by the `PayloadProfile` extractor. This broker
        // interprets every version the clause 4.3.6.8 tables describe, so no
        // amendment of the payload is needed for it to be accepted here.
        // Clause 6.3.5: an application/json body must not carry @context.
        if matches!(content_kind, ContentKind::Json) && envelope.has_context() {
            return Err(BodyAdmissionRefusal::ContextInJsonBody.into_rejection(uri));
        }

        if !envelope.contexts_are_well_formed() {
            return Err(BodyAdmissionRefusal::MalformedContext.into_rejection(uri));
        }

        // The context is carried by the spec; the member is skipped when the
        // body is decoded into its domain type.
        let context = match carrier {
            PayloadCarrier::LdJsonBody => ContextSpec::from_body(
                envelope
                    .into_body_context()
                    .map_err(|error| BodyAdmissionRefusal::from(error).into_rejection(uri.clone()))?, // clone: as above
            ),
            PayloadCarrier::LinkHeader => ContextSpec::from_request(link_context.map_or(RequestContext::Default, RequestContext::Link)),
        };

        Ok(AdmittedBody {
            text: bytes,
            context,
            uri,
            blocking_decode,
            geometry_positions,
        })
    }
}

/// Apply the clause 6.3.4 and 6.3.5 request preconditions and read the body.
async fn read_body<S: Send + Sync>(req: Request, state: &S) -> Result<ReadBody, ExtractorRejection> {
    let uri = addressed_request_uri(&req);
    // Clause 6.3.4: the payload length is checked before the payload is
    // looked at, so an absent Content-Length is the bare 411 the clause
    // requires rather than a decoding failure.
    let declared = declared_body_length(req.headers(), req.extensions()).map_err(|error| error.into_rejection(uri.clone()))?;
    let content_kind = admit_content_kind(req.headers(), req.method())?;

    // Clause 6.3.5: an `application/ld+json` body names its context itself and
    // refuses a context `Link` beside it; any other body is read against the
    // one context `Link` the request carries, or none. The headers are read
    // before the body is consumed.
    let carrier = match content_kind {
        ContentKind::LdJson => PayloadCarrier::LdJsonBody,
        ContentKind::Json | ContentKind::MergePatchJson => PayloadCarrier::LinkHeader,
    };
    let (mut parts, body) = req.into_parts();
    let link_context = RequestLinks::of(&mut parts)
        .and_then(|links| links.context_for(carrier))
        .map_err(|error| ExtractorRejection::Classified(classify_context_link_error(&error).with_instance(uri.clone())))?;
    let value_limit = parts.extensions.get::<RequestJsonValueLimit>().copied();
    let geometry_positions = parts.extensions.get::<RequestGeometryPositionLimit>().copied();
    let blocking_decode = parts.extensions.get::<BlockingDecodePolicy>().cloned(); // clone: the body owns its share of the permits
    let req = Request::from_parts(parts, body);

    let bytes = buffer_payload(req, state, declared, uri.clone()).await?;

    // A declared length the payload did not carry leaves clause 5.5.4 with
    // no JSON document to read, which is the refusal the `Content-Length`
    // precondition already owns.
    if bytes.is_empty() {
        return Err(ContentLengthPreconditionError::Empty.into_rejection(uri));
    }

    Ok(ReadBody {
        bytes,
        content_kind,
        carrier,
        link_context,
        uri,
        value_limit,
        geometry_positions,
        blocking_decode,
    })
}
