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
    contextual_body::open_object,
    entity_body::{
        linked_entities::AttributeJoin,
        member_walk::MemberWalk,
        notified_member::{MemberInstances, NotifiedMember},
        reified_member::{Reification, ReifiedMember},
        simplified_member::SimplifiedMember,
        structural_rule::keeps,
    },
    notification_entity::StampedInstants,
    notified_changes::NotifiedChanges,
    structural_projection::StructuralProjection,
    version::fallback::EntityMember,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_linked_retrieval::{keyword::Keyword, narrowing::member::MemberRule, plan::InlineCursor, typed::TypedEntities};
use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer, ser::SerializeMap};

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
    pub(crate) context: Option<&'a JsonLiteral>,
    /// The entity times a notification writes on the entity, if it is one.
    pub(crate) stamped: Option<&'a StampedInstants>,
    /// Where the clause 4.5.23 inline join stands at this entity, if the
    /// response asked for one: its Relationships nest what the cursor answers.
    pub(crate) join: Option<InlineCursor<'a, TypedEntities>>,
    /// The rule a Linked Entity's members are written by, when its list in
    /// force narrows anything (clauses 4.21 and 5.7.1.5).
    pub(crate) narrowing: Option<MemberRule<'a>>,
    /// What a notification reports of the change beside the entity's state
    /// (clause 5.8.6), if it is a notified entity.
    pub(crate) changes: Option<NotifiedChanges<'a>>,
}

impl TypedEntity<'_> {
    /// Whether the clause 4.21 rule keeps one structural member: a Linked
    /// Entity's own rule, or else the request's.
    pub(crate) fn keeps(&self, keyword: Keyword) -> bool {
        keeps(self.narrowing, self.projection, keyword)
    }
}

impl Serialize for TypedEntity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entity = self.entity;
        let mut object = open_object(serializer, self.context, None)?;
        let writes_id = match self.identifier {
            IdentifierMember::Written => self.keeps(Keyword::Id),
            IdentifierMember::Mandatory => true,
            IdentifierMember::Promoted => false,
        };
        if writes_id {
            object.serialize_entry(ProjectionTerm::ID_KEYWORD, &entity.id)?;
        }
        if self.keeps(Keyword::Type) {
            match entity.type_.iter().next() {
                Some(first) if self.walk.writes_single_type() => object.serialize_entry(ProjectionTerm::TYPE_KEYWORD, first)?,
                Some(_) | None => object.serialize_entry(ProjectionTerm::TYPE_KEYWORD, &entity.type_)?,
            }
        }
        if let Some(scope) = &entity.scope
            && self.keeps(Keyword::Scope)
            && self.walk.keeps_entity_member(EntityMember::Scope)
        {
            object.serialize_entry(ProjectionTerm::SCOPE_KEYWORD, scope)?;
        }
        let stamped = self.stamped.copied().unwrap_or_default();
        // The expiry a notification stamps is the entity's own member as much
        // as the one the entity carries, so the fallback drops both.
        let expiry = self.walk.keeps_entity_member(EntityMember::ExpiresAt);
        for (name, own, stamp, precedence) in [
            (
                compact_properties::EXPIRES_AT,
                entity.expires_at.filter(|_| expiry),
                stamped.expires_at.filter(|_| expiry),
                StampPrecedence::OwnFirst,
            ),
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
            let own = own.map(|instant| EntityInstant { instant });
            let stamp = stamp.map(|instant| EntityInstant { instant });
            let written = match precedence {
                StampPrecedence::OwnFirst => own.or(stamp),
                StampPrecedence::StampFirst => stamp.or(own),
            };
            if let Some(instant) = written {
                object.serialize_entry(name, &instant)?;
            }
        }
        let open = self.join.map(|cursor| cursor.open(&entity.id));
        for (name, member) in entity.attribute_members() {
            if self.narrowing.is_some_and(|rule| rule.member(name).is_skip()) {
                continue;
            }
            let walk = self.walk.with_join(open.as_ref().map(|open| AttributeJoin::Entity { open, attribute: name }));
            match &self.changes {
                None => object.serialize_entry(name, &self.member_under(MemberInstances::Held(member), walk))?,
                Some(changes) => {
                    if let Some(notified) = NotifiedMember::of(name, Some(member), changes) {
                        object.serialize_entry(name, &self.member_under(MemberInstances::Notified(&notified), walk))?;
                    }
                }
            }
        }
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
            ResponseFormat::KeyValues | ResponseFormat::Simplified => AttributeMember::Simplified(SimplifiedMember::new(member, walk)),
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

/// One of the clause 4.8 and 4.22 entity timestamps, written as RFC 3339 with
/// the sub-second precision the instant carries, at most the microsecond
/// clause 4.6.3 admits.
#[derive(Debug, Clone, Copy)]
struct EntityInstant {
    /// The instant.
    instant: DateTime<Utc>,
}

impl Serialize for EntityInstant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&instant_text(self.instant))
    }
}
