//! The representation a forwarded request carries (clauses 4.3.6.6, 4.3.6.8, 6.3.5, 6.3.19).
//!
//! A request arriving at this broker carries its `@context` in one of the two
//! ways clause 6.3.5 allows -- a `Link` header with `application/json`, or an
//! inline `@context` with `application/ld+json` -- and a registered source
//! enforces the same rule on what it receives. [`crate::forwarding_context`]
//! decides once, per registration, which single context a forwarded request is
//! expressed against; this module writes the request that way: which context
//! URL goes into its one `Link` header, and what the body looks like once its
//! admitted content is compacted under that context and the registration's
//! `contextSourceInfo` has been applied.
//!
//! Spelling is compaction. The content of a write is the typed expanded
//! document the admission produced, so it names no term: compacting it under
//! the forwarding context ([`EntityCompactor::patch`]) gives every name the
//! spelling the source reads, and the typed walk of
//! [`andromeda_ngsi_ld_render::forwarded_entity`] writes the result under the
//! registration's clause 4.3.6.8 fallback.

use crate::{
    error::FederationError,
    federation_invariant::FederationInvariant,
    forwarded_content::AttributeWrite,
    forwarded_names::ForwardedNames,
    forwarding_context::ForwardingDecision,
    term_resolver::TermResolver,
};
use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{batch_contexts::BatchContext, context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, spec_version::SpecVersion};
use andromeda_ngsi_ld_model::{
    attribute::{instance::AttributeValue, member_reading::MemberReading, merge_member::MergeMember},
    context_source_info::ContextSourceInfo,
    entity::EntityPatch,
    query::body::Query,
    subscription_request::SubscriptionRequest,
    temporal::temporal_attribute::TemporalAttribute,
};
use andromeda_ngsi_ld_render::{
    contextual_body::ContextualBody,
    forwarded_entity::{ForwardedAttribute, ForwardedEntity},
    forwarded_fragment::ForwardedFragment,
    forwarded_merge_entity::ForwardedMergeEntity,
    forwarded_subscription::ForwardedSubscription,
    version::adapted_payload::AdaptedPayload,
};
use indexmap::IndexMap;
use iri_rs::IriBuf;
use serde::{Serialize, Serializer, ser::SerializeSeq};
use std::borrow::Cow;

/// The representation one forwarded request is sent in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedPayload {
    /// The media type of the forwarded body, which also decides how the
    /// context is carried (clause 6.3.5).
    pub media_type: ResponseMediaType,
    /// The context IRI placed in the one `Link` header; absent for
    /// `application/ld+json`, whose context travels inline, and for a request
    /// that names the default context. A forwarded request never names two
    /// (JSON-LD 1.1 section 6.1).
    pub link_context: Option<IriBuf>,
    /// The forwarded body, absent for a body-less request.
    pub body: Option<Vec<u8>>,
}

/// The body one forwarded request carries.
#[derive(Debug, Clone, Copy)]
pub(crate) enum OutgoingBody<'a> {
    /// An entity, or an entity-shaped fragment, of the given Element, written
    /// as the admission expanded it.
    Entity {
        /// The admitted document.
        document: &'a EntityDocument,
        /// Which NGSI-LD Element its Entity data is, which decides how the
        /// clause 4.3.6.8 fallback reads it.
        element: AdaptedPayload,
    },
    /// The fragment of a Merge Entity (clause 5.6.17), whose attributes are
    /// merge members.
    Merge {
        /// The admitted document.
        document: &'a EntityDocument,
    },
    /// A Temporal Evolution whose Scope is reified (clause 4.5.6): the
    /// instances it adds, and the Scope history written as its `scope`.
    Evolution {
        /// The admitted instances.
        document: &'a EntityDocument,
        /// The reified Scope instances, keyed by the Scope IRI.
        scope_history: &'a EntityDocument,
        /// Which NGSI-LD Element its Entity data is.
        element: AdaptedPayload,
    },
    /// The one attribute instance a request path names (clauses 5.6.4 and
    /// 5.6.19).
    Attribute {
        /// The attribute's expanded name.
        name: &'a AttributeName,
        /// The admitted instance.
        instance: &'a InstanceDocument,
        /// What the instance writes of the attribute.
        write: AttributeWrite,
    },
    /// A reduced subscription (clause 5.8.1.4), whose names are respelled in
    /// the forwarding context.
    Subscription(&'a SubscriptionRequest),
    /// A `Query` this broker wrote in the forwarding context's terms already,
    /// such as the one of a forwarded batch query (clause 6.23).
    Query(&'a Query),
}

/// `document` compacted under `naming` to the partial entity a source reads.
async fn compacted_patch(engine: &JsonLdEngine, naming: &ContextSpec, document: &EntityDocument) -> Result<EntityPatch, FederationError> {
    let compactor = engine.compactor(naming);
    Ok(EntityCompactor::new(&compactor).patch(document).await?)
}

/// The reified Scope `scope_history` holds, compacted under `naming` to the
/// instances a source reads as its `scope`; none when the document holds no
/// Scope history.
async fn compacted_scope_history(
    engine: &JsonLdEngine,
    naming: &ContextSpec,
    scope_history: &EntityDocument,
) -> Result<Option<AttributeValue>, FederationError> {
    let compactor = engine.compactor(naming);
    let history = EntityCompactor::new(&compactor).scope_history(scope_history).await?;
    Ok(history.map(TemporalAttribute::into_member))
}

/// `document`, holding the one attribute a request path names, compacted
/// under `naming` to the bare Attribute Fragment a source reads (clauses
/// 5.6.4 and 5.6.19).
async fn compacted_fragment<M: MemberReading>(
    engine: &JsonLdEngine,
    naming: &ContextSpec,
    document: &EntityDocument,
) -> Result<EntityPatch<M>, FederationError> {
    let compactor = engine.compactor(naming);
    Ok(EntityCompactor::new(&compactor).attribute_fragments(document).await?)
}

/// The one attribute member `patch` holds.
fn only_attribute<M>(patch: &EntityPatch<M>) -> Option<&M> {
    patch.attribute_members().next().map(|(_, member)| member)
}

/// The document holding the one attribute `instance` of `name`.
fn attribute_document(name: &AttributeName, instance: &InstanceDocument) -> EntityDocument {
    // clone: the document owns the name and the instance it is compacted from
    let member = AttributeMember::Instances(vec![instance.clone()]);
    EntityDocument::new(None, Vec::new(), None, None, IndexMap::from([(name.clone(), member)])) // clone: same
}

/// `body`, written with the `@context` member first when `context` is given.
fn written<T: Serialize>(context: Option<&JsonLiteral>, body: &T) -> Result<Vec<u8>, FederationError> {
    Ok(serde_json::to_vec(&ContextualBody::new(context, body))?)
}

/// The bytes of one forwarded body, spelled in the context `decision` names.
async fn written_body(
    engine: &JsonLdEngine,
    inbound: &ContextSpec,
    decision: &ForwardingDecision,
    body: OutgoingBody<'_>,
    conformance: Option<SpecVersion>,
) -> Result<Vec<u8>, FederationError> {
    let naming = decision.context.naming(inbound);
    // Clause 6.3.5: `application/ld+json` carries its context inline and never
    // beside a context `Link`.
    let inline = decision.context.inline_for(inbound)?;
    match body {
        OutgoingBody::Entity { document, element } => {
            let patch = compacted_patch(engine, &naming, document).await?;
            written(inline.as_ref(), &ForwardedEntity::new(&patch, conformance, element))
        }
        OutgoingBody::Merge { document } => {
            let patch = compacted_fragment::<MergeMember>(engine, &naming, document).await?;
            written(inline.as_ref(), &ForwardedMergeEntity::new(&patch, conformance))
        }
        OutgoingBody::Evolution {
            document,
            scope_history,
            element,
        } => {
            let patch = compacted_patch(engine, &naming, document).await?;
            let history = compacted_scope_history(engine, &naming, scope_history).await?;
            let entity = ForwardedEntity::new(&patch, conformance, element);
            match &history {
                Some(history) => written(inline.as_ref(), &entity.with_scope_history(history)),
                None => written(inline.as_ref(), &entity),
            }
        }
        OutgoingBody::Attribute { name, instance, write } => {
            let document = attribute_document(name, instance);
            match write {
                AttributeWrite::Fragment => {
                    let patch = compacted_fragment::<MergeMember>(engine, &naming, &document).await?;
                    written(
                        inline.as_ref(),
                        &ForwardedFragment::new(only_attribute(&patch).ok_or(FederationInvariant::FragmentWithoutAttribute)?, conformance),
                    )
                }
                AttributeWrite::Replacement => {
                    let patch = compacted_fragment::<AttributeValue>(engine, &naming, &document).await?;
                    written(
                        inline.as_ref(),
                        &ForwardedAttribute::new(
                            only_attribute(&patch).ok_or(FederationInvariant::FragmentWithoutAttribute)?,
                            conformance,
                            AdaptedPayload::Entity,
                        ),
                    )
                }
            }
        }
        OutgoingBody::Subscription(request) => {
            let resolver = TermResolver::new(engine, inbound);
            let names = ForwardedNames::prepare(engine, &resolver, &naming, decision.context.rewording()).await?;
            written(inline.as_ref(), &ForwardedSubscription::new(names.subscription(request).await?, conformance))
        }
        OutgoingBody::Query(query) => written(inline.as_ref(), query),
    }
}

/// Build the payload of one forwarded request.
///
/// `inbound` is the context the triggering request resolved to; `decision` is
/// the registration's forwarding decision; `body` is the admitted content;
/// `info` is the registration's `contextSourceInfo`.
///
/// # Errors
///
/// Returns [`FederationError::Compaction`] when the content cannot be
/// compacted in the forwarding context, [`FederationError::JsonLd`] or
/// [`FederationError::Query`] when a name cannot be spelled there, and
/// [`FederationError::Representation`] when the body cannot be written.
pub(crate) async fn build_forwarded_payload(
    engine: &JsonLdEngine,
    inbound: &ContextSpec,
    decision: &ForwardingDecision,
    body: Option<OutgoingBody<'_>>,
    info: &ContextSourceInfo,
) -> Result<ForwardedPayload, FederationError> {
    let media_type = decision.media_type;
    let link_context = decision.context.link();
    let body = match body {
        Some(body) => Some(written_body(engine, inbound, decision, body, info.ngsild_conformance).await?),
        None => None,
    };
    Ok(ForwardedPayload {
        media_type,
        link_context,
        body,
    })
}

/// One entity of a forwarded batch array, and the context it was submitted
/// under: JSON-LD applies a top-level node's `@context` to that node, so each
/// entity of an `application/ld+json` batch is spelled back from its own.
#[derive(Debug)]
pub(crate) struct ItemBody<'a> {
    /// The admitted document, narrowed to what the registration receives.
    pub(crate) document: Cow<'a, EntityDocument>,
    /// The context the entity was submitted under.
    pub(crate) context: &'a BatchContext,
}

/// How the attributes of the entities of a forwarded batch are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ItemReading {
    /// Whole Attributes, as every batch operation but the merge writes them.
    Attributes,
    /// Merge members, which change only the members they name (clause 5.6.20).
    Merge,
}

/// One entity of a forwarded batch array, compacted.
enum BatchPatch {
    /// An entity of whole Attributes.
    Attributes(EntityPatch),
    /// An entity of merge members.
    Merge(EntityPatch<MergeMember>),
}

/// A forwarded batch array: each entity with the `@context` it carries inline.
struct BatchBody<'a> {
    /// Every entity, with the inline context its body carries.
    items: &'a [(Option<JsonLiteral>, BatchPatch)],
    /// The version the source conforms to.
    conformance: Option<SpecVersion>,
}

impl Serialize for BatchBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.items.len()))?;
        for (context, patch) in self.items {
            match patch {
                BatchPatch::Attributes(patch) => {
                    let entity = ForwardedEntity::new(patch, self.conformance, AdaptedPayload::Entity);
                    sequence.serialize_element(&ContextualBody::new(context.as_ref(), &entity))?;
                }
                BatchPatch::Merge(patch) => {
                    let entity = ForwardedMergeEntity::new(patch, self.conformance);
                    sequence.serialize_element(&ContextualBody::new(context.as_ref(), &entity))?;
                }
            }
        }
        sequence.end()
    }
}

/// Build the payload of one forwarded batch array, each entity compacted from
/// its own context into the one `decision` names, its attributes written as
/// `reading` says.
///
/// # Errors
///
/// Returns [`FederationError::Compaction`] when an entity cannot be compacted
/// in the forwarding context, and [`FederationError::Representation`] when the
/// array cannot be written.
pub(crate) async fn build_items_payload(
    engine: &JsonLdEngine,
    decision: &ForwardingDecision,
    items: Vec<ItemBody<'_>>,
    reading: ItemReading,
    info: &ContextSourceInfo,
) -> Result<ForwardedPayload, FederationError> {
    let mut patches = Vec::with_capacity(items.len());
    for ItemBody { document, context } in items {
        let naming = decision.context.naming(context.spec());
        let patch = match reading {
            ItemReading::Attributes => BatchPatch::Attributes(compacted_patch(engine, &naming, &document).await?),
            ItemReading::Merge => BatchPatch::Merge(compacted_fragment::<MergeMember>(engine, &naming, &document).await?),
        };
        patches.push((decision.context.inline_for(context.spec())?, patch));
    }
    let body = BatchBody {
        items: &patches,
        conformance: info.ngsild_conformance,
    };
    Ok(ForwardedPayload {
        media_type: decision.media_type,
        link_context: decision.context.link(),
        body: Some(serde_json::to_vec(&body)?),
    })
}

/// Build the payload of a forwarded batch deletion: the bare Entity IDs, as
/// `application/json` under no context `Link`.
///
/// Clause 6.3.5 has an `application/ld+json` body name its context in an
/// `@context` member, which an array of strings cannot carry, and the
/// identifiers name no term a context could resolve, so the array is sent the
/// one way a source admits it whatever representation its registration asks
/// for.
///
/// # Errors
///
/// Returns [`FederationError::Representation`] when the array cannot be
/// serialised.
pub(crate) fn build_identifiers_payload(ids: &[Identifier]) -> Result<ForwardedPayload, FederationError> {
    Ok(ForwardedPayload {
        media_type: ResponseMediaType::Json,
        link_context: None,
        body: Some(serde_json::to_vec(ids)?),
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
