//! Sending forwarded writes (POST/PUT/PATCH/DELETE) to context sources and
//! classifying their answers (clauses 4.3.6, 6.3.17).
//!
//! The single-entity provision, the batch operations and the purge each plan
//! their writes; every one of them is sent from here, with its body written in
//! the registration's forwarding context (clause 4.3.6.6).

use crate::{
    FederationService,
    document_rewording::DocumentVocabulary,
    error::FederationError,
    forwarded_payload::{ItemBody, OutgoingBody, build_identifiers_payload},
    forwarding_context::BodyPresence,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
    registration_payload::forwarding_decision,
    request::TriggeringRequest,
    transport::ForwardedRequest,
    write_answer::with_full_update_result_names,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_render::version::adapted_payload::AdaptedPayload;
use futures::{StreamExt, stream};
use tracing::{debug, warn};
use url::Url;

/// One write to forward to one registration, decided before it is sent.
pub(crate) struct PlannedWrite<'a> {
    /// The registration to contact.
    pub(crate) registration: ContextSourceRegistration,
    /// The forwarded URL.
    pub(crate) url: Result<Url, FederationError>,
    /// The HTTP method.
    pub(crate) method: reqwest::Method,
    /// The body narrowed to what the registration covers.
    pub(crate) body: Option<PlannedBody>,
    /// The attributes an exclusive registration claims from the payload.
    pub(crate) proxied_attrs: Vec<AttributeName>,
    /// The context of the triggering request.
    pub(crate) context: &'a ContextSpec,
    /// The triggering request.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// The `NGSILD-EntityMap` this forwarded write carries: the map the source
    /// is linked to under the Entity Map in use (clause 5.5.14), or `None` when
    /// the write runs under no map.
    pub(crate) linked_map: Option<Identifier>,
}

/// The body of one planned write.
pub(crate) struct PlannedBody {
    /// The serialised document.
    pub(crate) bytes: Vec<u8>,
    /// What kind of document it is, which decides where its names sit when it
    /// is spelled in the forwarding context.
    pub(crate) vocabulary: DocumentVocabulary,
    /// Which NGSI-LD Element the document's Entity data is, which decides
    /// how the clause 4.3.6.8 fallback reads it.
    pub(crate) element: AdaptedPayload,
}

/// What one forwarded write carries, and the context each part of it was
/// submitted under.
pub(crate) enum WritePayload<'a> {
    /// A body-less write (a DELETE), triggered under `context`.
    Empty(&'a ContextSpec),
    /// One document, already narrowed to what the registration covers,
    /// submitted under `context`.
    Document {
        /// The serialised document.
        bytes: &'a [u8],
        /// What kind of document it is.
        vocabulary: DocumentVocabulary,
        /// Which NGSI-LD Element its Entity data is.
        element: AdaptedPayload,
        /// The context it was submitted under.
        context: &'a ContextSpec,
    },
    /// A batch array whose entities each carry the context they were
    /// submitted under.
    Items(Vec<ItemBody<'a>>),
    /// The bare Entity IDs of a batch deletion (clause 5.6.10.3), which name
    /// no term and so are read under no context.
    Identifiers(&'a [Identifier]),
}

/// One forwarded write request, as [`FederationService::forward_write`] sends it.
///
/// The fields are what the transport needs to build and classify one request;
/// they travel as a struct rather than a long argument list.
pub(crate) struct ForwardWrite<'a> {
    /// The registration being contacted.
    pub(crate) registration: &'a ContextSourceRegistration,
    /// The forwarded URL, or the failure that stops it being built.
    pub(crate) url: Result<Url, FederationError>,
    /// The HTTP method.
    pub(crate) method: reqwest::Method,
    /// What the request carries.
    pub(crate) payload: WritePayload<'a>,
    /// The request that triggered the forwarding.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// The `NGSILD-EntityMap` the forwarded request carries (clause 5.5.14).
    pub(crate) linked_map: Option<&'a Identifier>,
}

impl FederationService {
    /// Send every planned write, at most `csr_concurrency` at a time.
    pub(crate) async fn execute_writes(&self, planned: Vec<PlannedWrite<'_>>) -> Vec<ForwardedWriteResult> {
        stream::iter(planned)
            .map(|write| self.execute_write(write))
            .buffer_unordered(self.csr_concurrency)
            .collect()
            .await
    }

    /// Send one planned write and classify what came back.
    pub(crate) async fn execute_write(&self, write: PlannedWrite<'_>) -> ForwardedWriteResult {
        let PlannedWrite {
            registration,
            url,
            method,
            body,
            proxied_attrs,
            context,
            triggering,
            linked_map,
        } = write;
        let mode = registration.access.mode.unwrap_or_default();
        let payload = match &body {
            Some(PlannedBody { bytes, vocabulary, element }) => WritePayload::Document {
                bytes,
                vocabulary: *vocabulary,
                element: *element,
                context,
            },
            None => WritePayload::Empty(context),
        };
        let outcome = self
            .forward_write(ForwardWrite {
                registration: &registration,
                url,
                method,
                payload,
                triggering,
                linked_map: linked_map.as_ref(),
            })
            .await;
        if let ForwardedWriteOutcome::Transport(failure) = &outcome {
            warn!("Failed to forward to context source {}: {}", registration.id, failure);
        }
        ForwardedWriteResult {
            registration_id: registration.id,
            mode,
            outcome,
            proxied_attrs,
        }
    }

    /// Make a write request (POST/PUT/PATCH/DELETE) to a context source endpoint.
    pub(crate) async fn forward_write(&self, write: ForwardWrite<'_>) -> ForwardedWriteOutcome {
        let ForwardWrite {
            registration,
            url,
            method,
            payload,
            triggering,
            linked_map,
        } = write;
        if self.registration_loops(registration, triggering.via) {
            return ForwardedWriteOutcome::LoopDetected;
        }
        let unreachable = |error: FederationError| ForwardedWriteOutcome::Transport(TransportFailure::unprepared(error));
        let url = match url {
            Ok(url) => url,
            Err(error) => return unreachable(error),
        };
        if let Some(remaining) = self.cooldowns.remaining(registration) {
            // Clause 5.2.34: a request inside the cooldown window is answered
            // with a timeout for the registration, without contacting it.
            return ForwardedWriteOutcome::Transport(TransportFailure::CoolingDown { remaining });
        }
        debug!("CS forwarding {} {}", method, url);
        // A single-entity write answers a partial success with an
        // `UpdateResult` whose names are spelled in the context the request
        // was written in; a batch answers with entity identifiers alone.
        let (payload, naming) = match payload {
            WritePayload::Empty(context) => (
                self.payload_for(registration, context, None).await,
                single_write_naming(registration, context, BodyPresence::Absent),
            ),
            WritePayload::Document {
                bytes,
                vocabulary,
                element,
                context,
            } => (
                self.payload_for(registration, context, Some(OutgoingBody::Document { bytes, vocabulary, element }))
                    .await,
                single_write_naming(registration, context, BodyPresence::Present),
            ),
            WritePayload::Items(items) => (self.payload_for_items(registration, items).await, Ok(None)),
            WritePayload::Identifiers(ids) => (build_identifiers_payload(ids), Ok(None)),
        };
        let (payload, naming) = match (payload, naming) {
            (Ok(payload), Ok(naming)) => (payload, naming),
            (Err(error), _) | (_, Err(error)) => return unreachable(error),
        };
        let request = match self.build_request(ForwardedRequest {
            method,
            url,
            payload: &payload,
            registration,
            triggering,
            linked_map,
        }) {
            Ok(request) => request,
            Err(error) => return unreachable(error),
        };

        let sent = match self.send_request(registration, request).await {
            Ok(sent) => sent,
            Err(failure) => return ForwardedWriteOutcome::Transport(failure),
        };

        let outcome = ForwardedWriteOutcome::of_sent(&registration.id, &sent);
        match naming {
            Some(naming) => with_full_update_result_names(&self.engine, &naming, outcome).await,
            None => outcome,
        }
    }
}

/// The context a single-entity write to `registration` is written in.
fn single_write_naming(
    registration: &ContextSourceRegistration,
    inbound: &ContextSpec,
    presence: BodyPresence,
) -> Result<Option<ContextSpec>, FederationError> {
    Ok(Some(forwarding_decision(registration, inbound, presence)?.context.naming(inbound)))
}
