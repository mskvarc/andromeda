//! The response pipeline for typed entities: the body written straight from
//! the typed [`Entity`] into whatever the HTTP binding writes the response
//! into.
//!
//! The stages are the ones [`crate::response`] runs over a document, in the
//! same order and with the same outcome -- the clause 4.5 representation, the
//! clause 4.15 negotiation, the clause 4.21 structural rule, the clause 4.5.23
//! inline join, the presentation-boundary URI decoding, the clause 6.3.6
//! `@context` member and the clause 4.5.16 `GeoJSON` envelope -- applied while
//! each member is written. A response a client asked to adapt to an older
//! specification version (clause 4.3.6.8) is the one exception: the fallback
//! tables rewrite a finished document, so each entity is written by the same
//! walk into a document, which is adapted and handed to [`crate::response`]
//! whole.

use crate::{
    context::ResponseContext,
    entity_body::{
        feature_body::FeatureBody,
        linked_entities::EntityJoin,
        member_walk::MemberWalk,
        typed_entity::{IdentifierMember, TypedEntity},
    },
    error::{RenderError, RenderedShape},
    nested_projection::LinkedProjection,
    response::{ReadyResponse, ResponseHead, ResponsePayload, ResponseRenderer, TargetDecoding},
    structural_projection::StructuralProjection,
    uri_decoding::UriDecoding,
    version::adaptation::VersionAdaptation,
};
use andromeda_http_headers::{language::LanguageFilter, media_type::ResponseMediaType};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, spec_version::SpecVersion};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use indexmap::IndexMap;
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;
use std::io::Write;

/// The member a JSON-LD document declares its context in.
const CONTEXT_MEMBER: &str = "@context";

/// The clause 4.5.23 inline join an entity of the response nests its Linked
/// Entities by.
#[derive(Debug, Clone, Copy)]
pub struct InlineJoin<'a> {
    /// The entities the traversal resolved, keyed by identifier.
    pub linked: &'a IndexMap<Identifier, Entity>,
    /// The clause 4.21 sub-projections that narrow the nested copies, if the
    /// request carried any.
    pub projection: Option<LinkedProjection<'a>>,
}

/// One entity of an entity response, with the clause 4.21 structural rule
/// that governs it.
///
/// The rule travels with each entity because a clause 4.5.23 flat join puts
/// entities governed by different terms into one collection: the request's own
/// for the entities it matched, a Relationship's nested sub-terms for a Linked
/// Entity (clause 5.7.1.5).
#[derive(Debug, Clone, Copy)]
pub struct EntityBody<'a> {
    /// The entity, in the attribute names its representation uses.
    entity: &'a Entity,
    /// The clause 4.21 rule over `id`, `type` and `scope`, if any.
    projection: Option<StructuralProjection<'a>>,
    /// The inline join its Relationships nest their targets by, if any.
    join: Option<InlineJoin<'a>>,
}

impl<'a> EntityBody<'a> {
    /// The entity, narrowed by `projection`.
    #[must_use]
    pub const fn new(entity: &'a Entity, projection: Option<StructuralProjection<'a>>) -> EntityBody<'a> {
        EntityBody {
            entity,
            projection,
            join: None,
        }
    }

    /// The entity, its Relationships nesting the Linked Entities `join`
    /// resolved (clause 4.5.23.2).
    #[must_use]
    pub const fn with_inline_join(self, join: InlineJoin<'a>) -> EntityBody<'a> {
        EntityBody { join: Some(join), ..self }
    }

    /// The entity written in `format` under the walk's presentation rules.
    fn typed(&self, format: ResponseFormat, language_filter: Option<&'a LanguageFilter>) -> TypedEntity<'a> {
        TypedEntity {
            entity: self.entity,
            format,
            walk: MemberWalk::new(UriDecoding::Decode, language_filter),
            projection: self.projection,
            identifier: IdentifierMember::Written,
            context: None,
            stamped: None,
            join: self.join.map(|join| EntityJoin::root(join.linked, join.projection)),
            narrowing: None,
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
        let version_adaptation = match self.renderer.requested_version {
            None => VersionAdaptation::None,
            Some(target) if target >= SpecVersion::CONFORMANT => VersionAdaptation::Unchanged,
            Some(_) => return self.render_adapted(writer),
        };
        let (context, context_link) = match &self.renderer.context {
            ResponseContext::Body(context) => (Some(context), None),
            ResponseContext::Link(target) => (None, Some(target.clone())), // clone: the head owns the target the renderer keeps
            ResponseContext::Absent => (None, None),
        };
        let format = self.renderer.format;
        let selection = self.renderer.geometry_selection;
        let language_filter = self.language_filter;
        let typed = |body: &EntityBody<'a>| -> TypedEntity<'a> { body.typed(format, language_filter) };
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

    /// Write each entity as a document, adapt the documents to the requested
    /// version and write them.
    fn render_adapted<W: Write>(self, writer: W) -> Result<ResponseHead, RenderError> {
        let ReadyEntityResponse {
            renderer,
            payload,
            language_filter,
        } = self;
        let format = renderer.format;
        let document = |body: &EntityBody<'a>| {
            serde_json::to_value(body.typed(format, language_filter)).map_err(|error| RenderError::serialization(RenderedShape::Entity, error))
        };
        let payload = match payload {
            EntityPayload::Single(body) => ResponsePayload::Single(document(&body)?),
            EntityPayload::Collection(bodies) => ResponsePayload::Collection(bodies.iter().map(document).collect::<Result<_, _>>()?),
        };
        // The walk decoded every target as it wrote it; decoding the document
        // again would undo an escape the client wrote on purpose.
        let rendered = ReadyResponse {
            renderer,
            payload,
            targets: TargetDecoding::Applied,
        }
        .render()?;
        serde_json::to_writer(writer, &rendered.value).map_err(|error| RenderError::serialization(RenderedShape::Entity, error))?;
        Ok(rendered.head)
    }
}

/// A collection of entities, each carrying the body's `@context`: an array of
/// entities is no JSON-LD document, so there is no enclosing object the member
/// could go on instead (clause 6.3.6).
struct EntityCollection<'a> {
    /// The entities.
    entities: &'a [TypedEntity<'a>],
    /// The `@context` each carries, if the body carries one.
    context: Option<&'a Value>,
}

impl Serialize for EntityCollection<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.entities.iter().map(|entity| TypedEntity {
            context: self.context,
            ..*entity
        }))
    }
}

/// The clause 4.5.16.3 `FeatureCollection`, carrying the body's `@context`
/// beside its features, none of which carries one of its own.
struct FeatureCollection<'a> {
    /// The features.
    features: Vec<FeatureBody<'a>>,
    /// The `@context`, if the body carries one.
    context: Option<&'a Value>,
}

impl Serialize for FeatureCollection<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut collection = serializer.serialize_map(None)?;
        collection.serialize_entry("type", "FeatureCollection")?;
        collection.serialize_entry("features", &self.features)?;
        if let Some(context) = self.context {
            collection.serialize_entry(CONTEXT_MEMBER, context)?;
        }
        collection.end()
    }
}
