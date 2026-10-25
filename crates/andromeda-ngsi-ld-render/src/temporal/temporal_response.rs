//! The response pipeline for Temporal Evolutions: the body written straight
//! from the typed evolution into whatever the HTTP binding writes the response
//! into.
//!
//! The stages are the ones [`crate::entity_response`] runs over entities, in
//! the same order and with the same outcome -- the clause 6.3.12
//! representation, the clause 4.15 negotiation, the clause 4.21 structural
//! rule, the clause 4.5.23 join, the presentation-boundary URI decoding, the
//! clause 4.3.6.8 version fallback a `Prefer: ngsi-ld=<version>` asks for and
//! the clause 6.3.6 `@context` member -- applied while each member is written.
//! A Temporal Evolution has no `GeoJSON` representation (clause 4.5.16 is
//! about Entities), so that media type is refused.

use crate::{
    context::ResponseContext,
    entity_body::member_walk::MemberWalk,
    error::{RenderError, RenderedShape},
    response::{ResponseHead, ResponseRenderer},
    structural_projection::StructuralProjection,
    temporal::{
        aggregated_body::AggregatedBody,
        temporal_entity_body::{AttributeShape, TemporalEntityBody},
        temporal_evolutions::TemporalEvolutions,
    },
    version::{adaptation::VersionAdaptation, adapted_payload::AdaptedPayload, fallback::VersionFallback},
};
use andromeda_http_headers::{language::LanguageFilter, media_type::ResponseMediaType};
use andromeda_json_literal::JsonLiteral;
use andromeda_linked_retrieval::{
    narrowing::member::MemberRule,
    plan::{FlatEntity, InlineCursor},
};
use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::{temporal_body::TemporalBody, temporal_entity::TemporalEntity};
use serde::{Serialize, Serializer};
use std::io::Write;

/// The clause 6.3.12 representation a temporal answer is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalRepresentation {
    /// The `format` of the request.
    pub format: TemporalFormat,
    /// The Temporal Property the request ran on, which the `[value, instant]`
    /// pairs of the simplified representation carry.
    pub timeproperty: TimeProp,
}

impl TemporalRepresentation {
    /// How the attributes of an evolution are written under it.
    const fn attribute_shape(self) -> AttributeShape {
        match self.format {
            TemporalFormat::Normalized | TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues => AttributeShape::Instances,
            TemporalFormat::TemporalValues => AttributeShape::Pairs(self.timeproperty),
        }
    }
}

/// One Temporal Evolution of a temporal response, with the clause 4.21
/// structural rule that governs it.
///
/// The rule travels with each entity because a clause 4.5.23 flat join puts
/// entities governed by different lists into one collection: the request's own
/// for the entities it matched, the list in force of each Linked Entity for the
/// Linked Entities (clause 5.7.1.5).
#[derive(Debug, Clone, Copy)]
pub struct TemporalRoot<'a> {
    /// The evolution, in the attribute names its representation uses.
    body: &'a TemporalBody,
    /// The clause 4.21 rule over `id`, `type` and `scope`, if any.
    projection: Option<StructuralProjection<'a>>,
    /// The rule a flat Linked Entity's members are written by, if it narrows
    /// anything.
    narrowing: Option<MemberRule<'a>>,
    /// Where the inline join its Relationships nest their targets by starts,
    /// if any.
    join: Option<InlineCursor<'a, TemporalEvolutions>>,
}

impl<'a> TemporalRoot<'a> {
    /// The evolution, narrowed by `projection`.
    #[must_use]
    pub const fn new(body: &'a TemporalBody, projection: Option<StructuralProjection<'a>>) -> TemporalRoot<'a> {
        TemporalRoot {
            body,
            projection,
            narrowing: None,
            join: None,
        }
    }

    /// One Linked Entity a flat join appends, written by the rule its first
    /// encounter decided (clause 4.5.23.3).
    #[must_use]
    pub const fn linked(linked: &FlatEntity<'a, TemporalEvolutions>) -> TemporalRoot<'a> {
        TemporalRoot {
            body: linked.node,
            projection: None,
            narrowing: linked.rule.restricting(),
            join: None,
        }
    }

    /// The evolution, its Relationships nesting the Linked Entities the inline
    /// plan's cursor answers (clause 4.5.23.2).
    #[must_use]
    pub const fn with_inline_join(self, join: InlineCursor<'a, TemporalEvolutions>) -> TemporalRoot<'a> {
        TemporalRoot { join: Some(join), ..self }
    }

    /// Whether the rules in force leave the evolution an Entity member to
    /// write: one reduced to none is no evolution at all (clause 5.7.3.4).
    #[must_use]
    pub fn carries_members(&self, representation: TemporalRepresentation) -> bool {
        match self.body {
            TemporalBody::Evolution(entity) => self.evolution(entity, representation, MemberWalk::new(None, None), None).carries_members(),
            TemporalBody::Aggregated(_) => true,
        }
    }

    /// The evolution written under `walk` in `representation`.
    const fn evolution(
        &self,
        entity: &'a TemporalEntity,
        representation: TemporalRepresentation,
        walk: MemberWalk<'a>,
        context: Option<&'a JsonLiteral>,
    ) -> TemporalEntityBody<'a> {
        TemporalEntityBody {
            entity,
            shape: representation.attribute_shape(),
            walk,
            projection: self.projection,
            join: self.join,
            narrowing: self.narrowing,
            context,
        }
    }
}

/// What a temporal response answers with.
#[derive(Debug)]
pub enum TemporalPayload<'a> {
    /// One Temporal Evolution (clause 6.5.3.1 of the temporal retrieval).
    Single(TemporalRoot<'a>),
    /// A collection of Temporal Evolutions (clause 5.7.4), and a clause 4.5.23
    /// flat join.
    Collection(Vec<TemporalRoot<'a>>),
}

/// One Temporal Evolution, in whichever of the two shapes it is written in.
enum NodeBody<'a> {
    /// The instances.
    Evolution(TemporalEntityBody<'a>),
    /// The aggregates.
    Aggregated(AggregatedBody<'a>),
}

impl Serialize for NodeBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            NodeBody::Evolution(body) => body.serialize(serializer),
            NodeBody::Aggregated(body) => body.serialize(serializer),
        }
    }
}

/// How one root is written, once the response's presentation rules are known.
#[derive(Debug, Clone, Copy)]
struct Presentation<'a> {
    /// The representation written.
    representation: TemporalRepresentation,
    /// What the walk applies to the attribute members.
    walk: MemberWalk<'a>,
    /// The `@context` each entity carries, if the body carries one.
    context: Option<&'a JsonLiteral>,
}

impl<'a> Presentation<'a> {
    /// `root` as it is written.
    const fn body(&self, root: &TemporalRoot<'a>) -> NodeBody<'a> {
        match root.body {
            TemporalBody::Evolution(entity) => NodeBody::Evolution(root.evolution(entity, self.representation, self.walk, self.context)),
            TemporalBody::Aggregated(entity) => NodeBody::Aggregated(AggregatedBody {
                entity,
                walk: self.walk,
                context: self.context,
            }),
        }
    }
}

/// A renderer with its temporal evolutions attached, ready to write.
#[derive(Debug)]
pub struct ReadyTemporalResponse<'a> {
    /// The configuration the evolutions were attached to.
    renderer: ResponseRenderer<'a>,
    /// What is being answered.
    payload: TemporalPayload<'a>,
    /// The representation the evolutions are written in.
    representation: TemporalRepresentation,
    /// The clause 4.15 `lang` filter, if the request sent one.
    language_filter: Option<&'a LanguageFilter>,
}

impl<'a> ResponseRenderer<'a> {
    /// Attach the Temporal Evolutions to render, in `representation` and
    /// negotiated against `language_filter` (clause 4.15), producing a
    /// runnable [`ReadyTemporalResponse`].
    #[must_use]
    pub const fn with_temporal(
        self,
        payload: TemporalPayload<'a>,
        representation: TemporalRepresentation,
        language_filter: Option<&'a LanguageFilter>,
    ) -> ReadyTemporalResponse<'a> {
        ReadyTemporalResponse {
            renderer: self,
            payload,
            representation,
            language_filter,
        }
    }
}

impl ReadyTemporalResponse<'_> {
    /// Write the response body into `writer` and say how the response
    /// carrying it is written.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when the body cannot be written,
    /// including when `writer` fails, and [`RenderError::TemporalGeoJson`] for
    /// the `GeoJSON` media type.
    pub fn render_into<W: Write>(self, writer: W) -> Result<ResponseHead, RenderError> {
        let adapted = AdaptedPayload::of_temporal(self.representation.format);
        let (version_adaptation, fallback) = match self.renderer.requested_version {
            None => (VersionAdaptation::None, None),
            Some(target) => (VersionAdaptation::requested(target), VersionFallback::new(target, adapted)),
        };
        let (context, context_link) = match &self.renderer.context {
            ResponseContext::Body(context) => (Some(context), None),
            ResponseContext::Link(target) => (None, Some(target.clone())), // clone: the head owns the target the renderer keeps
            ResponseContext::Absent => (None, None),
        };
        let presentation = Presentation {
            representation: self.representation,
            walk: MemberWalk::new(self.language_filter, fallback),
            context: None,
        };
        let written = match (&self.payload, self.renderer.media_type) {
            (TemporalPayload::Single(root), ResponseMediaType::Json | ResponseMediaType::JsonLd) => {
                serde_json::to_writer(writer, &Presentation { context, ..presentation }.body(root))
                    .map_err(|error| RenderError::serialization(RenderedShape::TemporalEntity, error))
            }
            (TemporalPayload::Collection(roots), ResponseMediaType::Json | ResponseMediaType::JsonLd) => {
                let presentation = Presentation { context, ..presentation };
                serde_json::to_writer(writer, &Collection { roots, presentation })
                    .map_err(|error| RenderError::serialization(RenderedShape::TemporalEntity, error))
            }
            (TemporalPayload::Single(_) | TemporalPayload::Collection(_), ResponseMediaType::GeoJson) => Err(RenderError::TemporalGeoJson),
        };
        written?;
        Ok(ResponseHead {
            context_link,
            media_type: self.renderer.media_type,
            version_adaptation,
        })
    }
}

/// An array of evolutions, each carrying the body's `@context`.
struct Collection<'r, 'a> {
    /// The evolutions.
    roots: &'r [TemporalRoot<'a>],
    /// How each is written.
    presentation: Presentation<'a>,
}

impl Serialize for Collection<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.roots.iter().map(|root| self.presentation.body(root)))
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod join_tests;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
