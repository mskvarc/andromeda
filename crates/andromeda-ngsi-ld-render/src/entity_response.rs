//! The response pipeline for typed entities: the body written straight from
//! the typed [`Entity`] into whatever the HTTP binding writes the response
//! into.
//!
//! Every stage is applied while each member is written: the clause 4.5
//! representation, the clause 4.15 negotiation, the clause 4.21 structural
//! rule, the clause 4.5.23 inline join, the presentation-boundary URI decoding,
//! the clause 4.3.6.8 version fallback a `Prefer: ngsi-ld=<version>` asks for,
//! the clause 6.3.6 `@context` member and the clause 4.5.16 `GeoJSON` envelope.
//! No document of the entity is built beside the body.

use crate::{
    context::ResponseContext,
    entity_body::{
        entity_collection::EntityCollection,
        feature_body::FeatureBody,
        feature_collection::FeatureCollection,
        member_walk::MemberWalk,
        typed_entity::{IdentifierMember, TypedEntity},
    },
    error::{RenderError, RenderedShape},
    response::{ResponseHead, ResponseRenderer},
    structural_projection::StructuralProjection,
    uri_decoding::UriDecoding,
    version::{adaptation::VersionAdaptation, adapted_payload::AdaptedPayload, fallback::VersionFallback},
};
use andromeda_http_headers::{language::LanguageFilter, media_type::ResponseMediaType};
use andromeda_linked_retrieval::{
    narrowing::member::MemberRule,
    plan::{FlatEntity, InlineCursor},
    typed::TypedEntities,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use std::io::Write;

/// One entity of an entity response, with the clause 4.21 structural rule
/// that governs it.
///
/// The rule travels with each entity because a clause 4.5.23 flat join puts
/// entities governed by different lists into one collection: the request's own
/// for the entities it matched, the list in force of each Linked Entity for
/// the Linked Entities (clause 5.7.1.5).
#[derive(Debug, Clone, Copy)]
pub struct EntityBody<'a> {
    /// The entity, in the attribute names its representation uses.
    entity: &'a Entity,
    /// The clause 4.21 rule over `id`, `type` and `scope`, if any.
    projection: Option<StructuralProjection<'a>>,
    /// The rule a flat Linked Entity's members are written by, if it narrows
    /// anything.
    narrowing: Option<MemberRule<'a>>,
    /// Where the inline join its Relationships nest their targets by starts,
    /// if any.
    join: Option<InlineCursor<'a, TypedEntities>>,
}

impl<'a> EntityBody<'a> {
    /// The entity, narrowed by `projection`.
    #[must_use]
    pub const fn new(entity: &'a Entity, projection: Option<StructuralProjection<'a>>) -> EntityBody<'a> {
        EntityBody {
            entity,
            projection,
            narrowing: None,
            join: None,
        }
    }

    /// One Linked Entity a flat join appends, written by the rule its first
    /// encounter decided (clause 4.5.23.3).
    #[must_use]
    pub const fn linked(linked: &FlatEntity<'a, TypedEntities>) -> EntityBody<'a> {
        EntityBody {
            entity: linked.node,
            projection: None,
            narrowing: linked.rule.restricting(),
            join: None,
        }
    }

    /// The entity, its Relationships nesting the Linked Entities the inline
    /// plan's cursor answers (clause 4.5.23.2).
    #[must_use]
    pub const fn with_inline_join(self, join: InlineCursor<'a, TypedEntities>) -> EntityBody<'a> {
        EntityBody { join: Some(join), ..self }
    }

    /// The entity written in `format` under the walk's presentation rules and
    /// `fallback`.
    const fn typed(&self, format: ResponseFormat, language_filter: Option<&'a LanguageFilter>, fallback: Option<VersionFallback>) -> TypedEntity<'a> {
        TypedEntity {
            entity: self.entity,
            format,
            walk: MemberWalk::new(UriDecoding::Decode, language_filter, fallback),
            projection: self.projection,
            identifier: IdentifierMember::Written,
            context: None,
            stamped: None,
            join: self.join,
            narrowing: self.narrowing,
            changes: None,
        }
    }
}

/// What an entity response answers with.
#[derive(Debug)]
pub enum EntityPayload<'a> {
    /// One entity (clause 6.5.3.1).
    Single(EntityBody<'a>),
    /// A collection of entities (clause 6.4.3.2, and a clause 4.5.23 flat
    /// join).
    Collection(Vec<EntityBody<'a>>),
}

/// A renderer with its typed entities attached, ready to write.
#[derive(Debug)]
pub struct ReadyEntityResponse<'a> {
    /// The configuration the entities were attached to.
    renderer: ResponseRenderer<'a>,
    /// What is being answered.
    payload: EntityPayload<'a>,
    /// The clause 4.15 `lang` filter, if the request sent one.
    language_filter: Option<&'a LanguageFilter>,
}

impl<'a> ReadyEntityResponse<'a> {
    /// The entities `payload`, to be written under `renderer`'s configuration.
    pub(crate) const fn new(
        renderer: ResponseRenderer<'a>,
        payload: EntityPayload<'a>,
        language_filter: Option<&'a LanguageFilter>,
    ) -> ReadyEntityResponse<'a> {
        ReadyEntityResponse {
            renderer,
            payload,
            language_filter,
        }
    }
}

impl<'a> ReadyEntityResponse<'a> {
    /// Write the response body into `writer` and say how the response
    /// carrying it is written.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when the body cannot be written,
    /// including when `writer` fails.
    pub fn render_into<W: Write>(self, writer: W) -> Result<ResponseHead, RenderError> {
        let (version_adaptation, fallback) = match self.renderer.requested_version {
            None => (VersionAdaptation::None, None),
            Some(target) => (VersionAdaptation::requested(target), VersionFallback::new(target, AdaptedPayload::Entity)),
        };
        let (context, context_link) = match &self.renderer.context {
            ResponseContext::Body(context) => (Some(context), None),
            ResponseContext::Link(target) => (None, Some(target.clone())), // clone: the head owns the target the renderer keeps
            ResponseContext::Absent => (None, None),
        };
        let format = self.renderer.format;
        let selection = self.renderer.geometry_selection;
        let language_filter = self.language_filter;
        let typed = |body: &EntityBody<'a>| -> TypedEntity<'a> { body.typed(format, language_filter, fallback) };
        let written = match (&self.payload, self.renderer.media_type) {
            (EntityPayload::Single(body), ResponseMediaType::Json | ResponseMediaType::JsonLd) => {
                serde_json::to_writer(writer, &TypedEntity { context, ..typed(body) }).map_err(|error| RenderError::serialization(RenderedShape::Entity, error))
            }
            (EntityPayload::Collection(bodies), ResponseMediaType::Json | ResponseMediaType::JsonLd) => {
                let entities: Vec<TypedEntity<'_>> = bodies.iter().map(typed).collect();
                serde_json::to_writer(writer, &EntityCollection { entities: &entities, context })
                    .map_err(|error| RenderError::serialization(RenderedShape::Entity, error))
            }
            (EntityPayload::Single(body), ResponseMediaType::GeoJson) => FeatureBody::new(&typed(body), selection, context)
                .and_then(|feature| serde_json::to_writer(writer, &feature))
                .map_err(|error| RenderError::serialization(RenderedShape::GeoJsonFeature, error)),
            (EntityPayload::Collection(bodies), ResponseMediaType::GeoJson) => bodies
                .iter()
                .map(|body| FeatureBody::new(&typed(body), selection, None))
                .collect::<Result<Vec<_>, _>>()
                .and_then(|features| serde_json::to_writer(writer, &FeatureCollection { features, context }))
                .map_err(|error| RenderError::serialization(RenderedShape::GeoJsonFeatureCollection, error)),
        };
        written?;
        Ok(ResponseHead {
            context_link,
            media_type: self.renderer.media_type,
            version_adaptation,
        })
    }
}
