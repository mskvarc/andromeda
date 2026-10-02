//! One entity written straight from its typed form into the response
//! serializer.
//!
//! The entity members clause 5.2.4 defines are written as they are, narrowed by
//! the clause 4.21 structural rule, and every attribute member is written in
//! the requested clause 4.5 representation by the walk that representation
//! owns. Nothing is first serialised into a document and transformed: the
//! representation, the clause 4.15 negotiation, the clause 4.5.23 inline join
//! and the presentation-boundary URI decoding are all applied as the members
//! are written.

use crate::{
    entity_body::{
        linked_entities::{EntityJoin, JoinPath},
        member_walk::MemberWalk,
        notified_member::{MemberInstances, NotifiedMember},
        reified_member::{Reification, ReifiedMember},
        simplified_member::{SimplifiedMember, VerbatimReading},
    },
    nested_projection::NestedProjection,
    notification_entity::StampedInstants,
    notified_changes::NotifiedChanges,
    structural_projection::StructuralProjection,
};
use andromeda_ngsi_ld_lexicon::rfc3339::sub_second;
use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer, ser::SerializeMap};
use serde_json::Value;

/// The member a JSON-LD document declares its context in.
const CONTEXT_MEMBER: &str = "@context";

/// Whether the entity's `id` is written as one of its members.
///
/// A `GeoJSON` Feature promotes the identifier out of its `properties` into the
/// Feature's own `id` (clause 4.5.16.2), so its properties are the entity
/// without it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdentifierMember {
    /// The `id` is a member, as clause 5.2.4 places it, unless the clause 4.21
    /// rule leaves it out.
    Written,
    /// The `id` is a member whatever the clause 4.21 rule says: a notification
    /// carries Entities, and an Entity is only actionable with its `id`
    /// (clauses 4.5.1 and 5.8.6).
    Mandatory,
    /// The `id` stands outside the members written.
    Promoted,
}

/// How the four clause 4.8 and 4.22 entity timestamps are spelled.
///
/// The two spellings agree on every instant a broker holds; they are kept
/// apart because the entity reading of clause 4.5.4 and the clause 4.15
/// reading spell the instant through RFC 3339 directly while every other
/// rendering writes the typed timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstantSpelling {
    /// The typed timestamp's own rendering.
    Typed,
    /// RFC 3339 with the sub-second precision the instant carries.
    SubSecond,
}

/// One entity in a clause 4.5 representation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TypedEntity<'a> {
    /// The entity written.
    pub(crate) entity: &'a Entity,
    /// The representation its attributes are written in.
    pub(crate) format: ResponseFormat,
    /// What the walk applies to the attribute members.
    pub(crate) walk: MemberWalk<'a>,
    /// The clause 4.21 rule over `id`, `type` and `scope`, if any.
    pub(crate) projection: Option<StructuralProjection<'a>>,
    /// Whether `id` is written among the members.
    pub(crate) identifier: IdentifierMember,
    /// The `@context` member written beside the entity's own, if the body
    /// carries one.
    pub(crate) context: Option<&'a Value>,
    /// The entity times a notification writes on the entity, if it is one.
    pub(crate) stamped: Option<&'a StampedInstants>,
    /// The clause 4.5.23 inline join its Relationships nest their targets by,
    /// if the response asked for one.
    pub(crate) join: Option<EntityJoin<'a>>,
    /// The clause 4.21 sub-projection that narrows its attributes, when it is
    /// a Linked Entity a Relationship carrying one nested.
    pub(crate) narrowing: Option<NestedProjection<'a>>,
    /// What a notification reports of the change beside the entity's state
    /// (clause 5.8.6), if it is a notified entity.
    pub(crate) changes: Option<NotifiedChanges<'a>>,
}

impl TypedEntity<'_> {
    /// Whether the clause 4.21 rule keeps one structural member.
    pub(crate) fn keeps(&self, keyword: &ProjectionTerm) -> bool {
        self.projection.is_none_or(|projection| projection.keeps(keyword))
    }

    /// Whether the request asked for nothing beyond the representation: no
    /// language to negotiate, no member to project and no entity to nest.
    const fn plain(&self) -> bool {
        self.walk.language_filter.is_none() && self.projection.is_none() && self.join.is_none() && self.narrowing.is_none()
    }

    /// How the entity timestamps are spelled.
    const fn instant_spelling(&self) -> InstantSpelling {
        if self.walk.language_filter.is_some() || (self.format.is_simplified() && self.plain()) {
            InstantSpelling::SubSecond
        } else {
            InstantSpelling::Typed
        }
    }

    /// How a member the broker carries verbatim is read under clause 4.5.4.
    const fn verbatim_reading(&self) -> VerbatimReading {
        if self.plain() { VerbatimReading::Envelope } else { VerbatimReading::Document }
    }
}

impl Serialize for TypedEntity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entity = self.entity;
        let mut object = serializer.serialize_map(None)?;
        let writes_id = match self.identifier {
            IdentifierMember::Written => self.keeps(&ProjectionTerm::Id),
            IdentifierMember::Mandatory => true,
            IdentifierMember::Promoted => false,
        };
        if writes_id {
            object.serialize_entry(ProjectionTerm::ID_KEYWORD, &entity.id)?;
        }
        if self.keeps(&ProjectionTerm::Type) {
            object.serialize_entry(ProjectionTerm::TYPE_KEYWORD, &entity.type_)?;
        }
        if let Some(scope) = &entity.scope
            && self.keeps(&ProjectionTerm::Scope)
        {
            object.serialize_entry(ProjectionTerm::SCOPE_KEYWORD, scope)?;
        }
        let spelling = self.instant_spelling();
        let stamped = self.stamped.copied().unwrap_or_default();
        for (name, own, stamp, precedence) in [
            (compact_properties::EXPIRES_AT, entity.expires_at, stamped.expires_at, StampPrecedence::OwnFirst),
            (compact_properties::CREATED_AT, entity.created_at, stamped.created_at, StampPrecedence::OwnFirst),
            (
                compact_properties::MODIFIED_AT,
                entity.modified_at,
                stamped.modified_at,
                StampPrecedence::OwnFirst,
            ),
            (
                compact_properties::DELETED_AT,
                entity.deleted_at,
                stamped.deleted_at,
                StampPrecedence::StampFirst,
            ),
        ] {
            let own = own.map(|instant| EntityInstant { instant, spelling });
            let stamp = stamp.map(|instant| EntityInstant {
                instant,
                spelling: InstantSpelling::SubSecond,
            });
            let written = match precedence {
                StampPrecedence::OwnFirst => own.or(stamp),
                StampPrecedence::StampFirst => stamp.or(own),
            };
            if let Some(instant) = written {
                object.serialize_entry(name, &instant)?;
            }
        }
        // The entity is open on the join's path while its own Relationships
        // nest their targets, so none of them nests it again.
        let path = JoinPath::new(entity.id.as_uri(), self.join.and_then(|join| join.parent()));
        for (name, member) in entity.attribute_members() {
            if self.narrowing.is_some_and(|narrowing| !narrowing.keeps_attribute(name)) {
                continue;
            }
            let walk = self.walk.with_join(self.join.map(|join| join.attribute(name, &path)));
            match &self.changes {
                None => object.serialize_entry(name, &self.member_under(MemberInstances::Held(member), walk))?,
                Some(changes) => {
                    if let Some(notified) = NotifiedMember::of(name, Some(member), changes) {
                        object.serialize_entry(name, &self.member_under(MemberInstances::Notified(&notified), walk))?;
                    }
                }
            }
        }
        // The Attributes a change deleted whole join the ones still held.
        if let Some(changes) = &self.changes
            && let Some(deleted) = changes.deleted
        {
            for name in deleted
                .instances
                .attribute_member_names()
                .filter(|name| entity.attribute_member(name).is_none())
            {
                if let Some(notified) = NotifiedMember::of(name, None, changes) {
                    object.serialize_entry(name, &self.member_under(MemberInstances::Notified(&notified), self.walk.with_join(None)))?;
                }
            }
        }
        if let Some(context) = self.context {
            object.serialize_entry(CONTEXT_MEMBER, context)?;
        }
        object.end()
    }
}

impl<'a> TypedEntity<'a> {
    /// One attribute member in the entity's representation.
    pub(crate) const fn attribute_member(&self, member: &'a AttributeValue) -> AttributeMember<'a> {
        self.member_under(MemberInstances::Held(member), self.walk)
    }

    /// One attribute member in the entity's representation, under `walk`.
    const fn member_under<'m>(&self, member: MemberInstances<'m>, walk: MemberWalk<'m>) -> AttributeMember<'m> {
        match self.format {
            ResponseFormat::Normalized => AttributeMember::Reified(ReifiedMember::new(member, Reification::Normalized, walk)),
            ResponseFormat::Concise => AttributeMember::Reified(ReifiedMember::new(member, Reification::Concise, walk)),
            // Clause 4.5.4 gives `keyValues` and `simplified` one rendering.
            ResponseFormat::KeyValues | ResponseFormat::Simplified => AttributeMember::Simplified(SimplifiedMember::new(member, walk, self.verbatim_reading())),
        }
    }
}

/// One attribute member, in whichever representation the entity is written in.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AttributeMember<'a> {
    /// The normalized or concise representation.
    Reified(ReifiedMember<'a>),
    /// The simplified representation.
    Simplified(SimplifiedMember<'a>),
}

impl Serialize for AttributeMember<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AttributeMember::Reified(member) => member.serialize(serializer),
            AttributeMember::Simplified(member) => member.serialize(serializer),
        }
    }
}

/// Which of the entity's own time and the one a notification stamps on it is
/// written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StampPrecedence {
    /// The entity's own, the stamp only where it carries none.
    OwnFirst,
    /// The stamp, the entity's own only where there is none.
    StampFirst,
}

/// One entity timestamp in the spelling the rendering gives it.
#[derive(Debug, Clone, Copy)]
struct EntityInstant {
    /// The instant.
    instant: DateTime<Utc>,
    /// How it is spelled.
    spelling: InstantSpelling,
}

impl Serialize for EntityInstant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.spelling {
            InstantSpelling::Typed => self.instant.serialize(serializer),
            InstantSpelling::SubSecond => serializer.serialize_str(&sub_second(self.instant)),
        }
    }
}
