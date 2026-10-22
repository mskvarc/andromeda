//! One Temporal Evolution of an Entity written straight from its typed form
//! into the response serializer (clauses 4.5.7 and 4.5.9).
//!
//! The entity's own members are written as clause 4.5.6 keeps them, narrowed by
//! the clause 4.21 structural rule, and every attribute is written in the
//! representation the request selected: the array of its instances, each
//! written as any attribute instance is (clause 4.5.7), or the `[value,
//! instant]` pairs of the simplified representation (clause 4.5.9). The clause
//! 4.15 negotiation, the clause 4.5.23 inline join, the clause 4.3.6.8
//! version fallback and the presentation-boundary URI decoding are all applied
//! as the members are written, so no second copy of the evolution is built.

use crate::{
    contextual_body::open_object,
    entity_body::{
        linked_entities::AttributeJoin,
        member_walk::MemberWalk,
        notified_member::MemberInstances,
        reified_member::{Reification, ReifiedMember},
        structural_rule::keeps,
    },
    structural_projection::StructuralProjection,
    temporal::{temporal_evolutions::TemporalEvolutions, temporal_values_member::ValuesMember},
    version::fallback::EntityMember,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_linked_retrieval::{keyword::Keyword, narrowing::member::MemberRule, plan::InlineCursor};
use andromeda_ngsi_ld_lexicon::{time::rendering::instant_text, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::{temporal_attribute::TemporalAttribute, temporal_entity::TemporalEntity};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer, ser::SerializeMap};

/// How the attributes of an evolution are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttributeShape {
    /// The array of the attribute's instances (clause 4.5.7).
    Instances,
    /// The `[value, instant]` pairs of the simplified representation (clause
    /// 4.5.9), the instant being the one on the Temporal Property the query
    /// ran on.
    Pairs(TimeProp),
}

/// One Temporal Evolution in a clause 4.5.6 representation.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TemporalEntityBody<'a> {
    /// The evolution written.
    pub(crate) entity: &'a TemporalEntity,
    /// How its attributes are written.
    pub(crate) shape: AttributeShape,
    /// What the walk applies to the attribute members.
    pub(crate) walk: MemberWalk<'a>,
    /// The clause 4.21 rule over `id`, `type` and `scope`, if any.
    pub(crate) projection: Option<StructuralProjection<'a>>,
    /// Where the clause 4.5.23 inline join stands at this entity, if the
    /// response asked for one: its Relationship instances nest what the
    /// cursor answers.
    pub(crate) join: Option<InlineCursor<'a, TemporalEvolutions>>,
    /// The rule a Linked Entity's members are written by, when its list in
    /// force narrows anything (clauses 4.21 and 5.7.1.5).
    pub(crate) narrowing: Option<MemberRule<'a>>,
    /// The `@context` member written beside the entity's own, if the body
    /// carries one.
    pub(crate) context: Option<&'a JsonLiteral>,
}

impl<'a> TemporalEntityBody<'a> {
    /// A Linked Entity nested under a Relationship instance: always the array
    /// of instances, written under the rule its path's list decided.
    pub(crate) const fn nested(entity: &'a TemporalEntity, walk: MemberWalk<'a>, cursor: InlineCursor<'a, TemporalEvolutions>) -> TemporalEntityBody<'a> {
        TemporalEntityBody {
            entity,
            shape: AttributeShape::Instances,
            walk,
            projection: None,
            join: Some(cursor),
            narrowing: cursor.rule().restricting(),
            context: None,
        }
    }

    /// Whether the clause 4.21 rule in force keeps one structural member.
    fn keeps(&self, keyword: Keyword) -> bool {
        keeps(self.narrowing, self.projection, keyword)
    }

    /// Whether the evolution still carries an Entity member once the rule is
    /// applied: clause 5.7.3.5 reduces the evolution "down to only contain
    /// the defined Entity members", and one reduced to none is no evolution at
    /// all (clause 5.7.3.4). `@context` is a transport concern, not a member.
    pub(crate) fn carries_members(&self) -> bool {
        let entity = self.entity;
        self.keeps(Keyword::Id)
            || self.keeps(Keyword::Type)
            || (self.keeps(Keyword::Scope) && entity.scope.is_some())
            || !entity.attrs.is_empty()
            || !entity.system_times.is_empty()
    }

    /// One attribute of the evolution in the shape the body writes.
    fn member(&self, attribute: &'a TemporalAttribute, walk: MemberWalk<'a>) -> AttributeBody<'a> {
        match self.shape {
            AttributeShape::Instances => AttributeBody::Instances(ReifiedMember::new(
                MemberInstances::Evolution(attribute.instances()),
                Reification::Normalized,
                walk,
            )),
            AttributeShape::Pairs(timeproperty) => AttributeBody::Pairs(ValuesMember::new(attribute, timeproperty, walk)),
        }
    }
}

/// One attribute member, in whichever shape the evolution is written in.
enum AttributeBody<'a> {
    /// The array of instances.
    Instances(ReifiedMember<'a>),
    /// The pairs of the simplified representation.
    Pairs(ValuesMember<'a>),
}

impl Serialize for AttributeBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            AttributeBody::Instances(member) => member.serialize(serializer),
            AttributeBody::Pairs(member) => member.serialize(serializer),
        }
    }
}

impl Serialize for TemporalEntityBody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entity = self.entity;
        let mut object = open_object(serializer, self.context, None)?;
        if self.keeps(Keyword::Id) {
            object.serialize_entry(keyword::ID_ALIAS, &entity.id)?;
        }
        if self.keeps(Keyword::Type) {
            match entity.type_.iter().next() {
                Some(first) if self.walk.writes_single_type() => object.serialize_entry(keyword::TYPE_ALIAS, first)?,
                Some(_) | None => object.serialize_entry(keyword::TYPE_ALIAS, &entity.type_)?,
            }
        }
        // The Scope history is an attribute of the evolution; the keyword
        // rule decides whether it is written at all.
        let scope_walk = self.walk.with_join(None);
        if let Some(scope) = &entity.scope
            && self.keeps(Keyword::Scope)
            && self.walk.keeps_entity_member(EntityMember::Scope)
        {
            object.serialize_entry(compact_properties::SCOPE, &self.member(scope, scope_walk))?;
        }
        for (name, instant) in [
            (compact_properties::CREATED_AT, entity.system_times.created_at),
            (compact_properties::MODIFIED_AT, entity.system_times.modified_at),
            (compact_properties::DELETED_AT, entity.system_times.deleted_at),
        ] {
            if let Some(instant) = instant {
                object.serialize_entry(name, &EntityInstant(instant))?;
            }
        }
        // The entity is open on the join's path while its own Relationship
        // instances nest their targets.
        let open = self.join.map(|cursor| cursor.open(&entity.id));
        for (name, attribute) in &entity.attrs {
            if self.narrowing.is_some_and(|rule| rule.member(name.as_str()).is_skip()) {
                continue;
            }
            let walk = self.walk.with_join(open.as_ref().map(|open| AttributeJoin::Temporal {
                open,
                attribute: name.as_str(),
            }));
            object.serialize_entry(name.as_str(), &self.member(attribute, walk))?;
        }
        object.end()
    }
}

/// One entity timestamp, spelled with exactly the sub-second precision it
/// carries.
struct EntityInstant(DateTime<Utc>);

impl Serialize for EntityInstant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&instant_text(self.0))
    }
}
