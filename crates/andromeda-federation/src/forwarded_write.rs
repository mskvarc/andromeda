//! Sending forwarded writes (POST/PUT/PATCH/DELETE) to context sources and
//! classifying their answers (clauses 4.3.6, 6.3.17).
//!
//! The single-entity provision, the batch operations and the purge each plan
//! their writes; every one of them is sent from here, with its body written in
//! the registration's forwarding context (clause 4.3.6.6).

use crate::{
    FederationService,
    error::FederationError,
    forwarded_content::ForwardedContent,
    forwarded_payload::{ItemBody, OutgoingBody, build_identifiers_payload},
    forwarding_context::BodyPresence,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
    registration_payload::forwarding_decision,
    request::TriggeringRequest,
    transport::ForwardedRequest,
    write_answer::with_full_update_result_names,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::document::{EntityDocument, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_render::version::adapted_payload::AdaptedPayload;
use futures::{StreamExt, stream};
use std::borrow::Cow;
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
    pub(crate) body: Option<PlannedBody<'a>>,
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
pub(crate) struct PlannedBody<'a> {
    /// The admitted content, narrowed to what the registration covers.
    pub(crate) content: PlannedContent<'a>,
    /// Which NGSI-LD Element the content's Entity data is, which decides how
    /// the clause 4.3.6.8 fallback reads it.
    pub(crate) element: AdaptedPayload,
}

/// The admitted content of one planned write: borrowed as admitted when the
/// registration covers all of it, its own narrowed copy otherwise.
pub(crate) enum PlannedContent<'a> {
    /// An entity or an entity-shaped fragment, boxed because it is the widest
    /// member of a planned write.
    Entity(Box<Cow<'a, EntityDocument>>),
    /// A Temporal Evolution with a reified Scope: the narrowed instances, and
    /// the Scope history, which narrowing leaves whole.
    Evolution {
        /// The instances, narrowed to what the registration covers.
        document: Box<Cow<'a, EntityDocument>>,
        /// The reified Scope instances.
        scope_history: &'a EntityDocument,
    },
    /// The one attribute instance a request path names.
    Attribute {
        /// The attribute's expanded name.
        name: &'a AttributeName,
        /// The admitted instance.
        instance: &'a InstanceDocument,
    },
}

impl PlannedContent<'_> {
    /// The content as the forwarding reads it.
    fn forwarded(&self) -> ForwardedContent<'_> {
        match self {
            PlannedContent::Entity(document) => ForwardedContent::Entity(document),
            PlannedContent::Evolution { document, scope_history } => ForwardedContent::Evolution { document, scope_history },
            PlannedContent::Attribute { name, instance } => ForwardedContent::Attribute { name, instance },
        }
    }
}

/// What one forwarded write carries, and the context each part of it was
/// submitted under.
pub(crate) enum WritePayload<'a> {
    /// A body-less write (a DELETE), triggered under `context`.
    Empty(&'a ContextSpec),
    /// One admitted content, already narrowed to what the registration
    /// covers, submitted under `context`.
    Document {
        /// The admitted content.
        content: ForwardedContent<'a>,
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
            Some(PlannedBody { content, element }) => WritePayload::Document {
                content: content.forwarded(),
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
            WritePayload::Document { content, element, context } => (
                self.payload_for(registration, context, Some(outgoing_body(content, element))).await,
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

/// The body `content` of an `element` payload is forwarded as.
const fn outgoing_body(content: ForwardedContent<'_>, element: AdaptedPayload) -> OutgoingBody<'_> {
    match content {
        ForwardedContent::Entity(document) => OutgoingBody::Entity { document, element },
        ForwardedContent::Evolution { document, scope_history } => OutgoingBody::Evolution {
            document,
            scope_history,
            element,
        },
        ForwardedContent::Attribute { name, instance } => OutgoingBody::Attribute { name, instance },
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
