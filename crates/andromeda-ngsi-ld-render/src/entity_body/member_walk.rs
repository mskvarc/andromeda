//! What the walk over an entity's attribute members applies on the way.

use crate::{
    entity_body::linked_entities::AttributeJoin,
    version::fallback::{EntityMember, InstanceMember, VersionFallback},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

/// The presentation-boundary rules the walk applies while it writes an
/// attribute member: the clause 4.3.6.8 version fallback
/// (everywhere), and clause 4.15 language negotiation and the clause 4.5.23
/// inline join (top-level attributes only).
#[derive(Debug, Clone, Copy)]
pub(crate) struct MemberWalk<'a> {
    /// The clause 4.15 `lang` filter a top-level `LanguageProperty` is
    /// negotiated against, if the request sent one.
    pub(crate) language_filter: Option<&'a LanguageFilter>,
    /// The inline join a top-level Relationship nests its targets by, if the
    /// response asked for one.
    pub(crate) join: Option<AttributeJoin<'a>>,
    /// The clause 4.3.6.8 fallback every member is written under, when the
    /// payload is written for an older specification version.
    pub(crate) fallback: Option<VersionFallback>,
}

impl<'a> MemberWalk<'a> {
    /// The walk over a top-level attribute member.
    pub(crate) const fn new(language_filter: Option<&'a LanguageFilter>, fallback: Option<VersionFallback>) -> MemberWalk<'a> {
        MemberWalk {
            language_filter,
            join: None,
            fallback,
        }
    }

    /// The same walk, nesting the targets of the member by `join`.
    pub(crate) const fn with_join(self, join: Option<AttributeJoin<'a>>) -> MemberWalk<'a> {
        MemberWalk { join, ..self }
    }

    /// The walk over a sub-attribute, which clause 4.15 does not negotiate --
    /// the filter selects among the Attributes of the Entity, and a
    /// sub-attribute is written as it is -- and whose targets clause 4.5.23
    /// does not nest. A sub-attribute is a Property or Relationship like any
    /// other (clauses 4.5.2 and 4.5.3), so the version fallback reaches it.
    pub(crate) const fn nested(self) -> MemberWalk<'a> {
        MemberWalk {
            language_filter: None,
            join: None,
            fallback: self.fallback,
        }
    }

    /// Whether the entity keeps `member` (Table 4.3.6.8-1).
    pub(crate) const fn keeps_entity_member(self, member: EntityMember) -> bool {
        match self.fallback {
            Some(fallback) => fallback.keeps_entity_member(member),
            None => true,
        }
    }

    /// Whether the entity's `type` is written as its first value alone (Table
    /// 4.3.6.8-1 note 1).
    pub(crate) const fn writes_single_type(self) -> bool {
        match self.fallback {
            Some(fallback) => fallback.writes_single_type(),
            None => false,
        }
    }

    /// Whether a multi-attribute is written as the one instance Table
    /// 4.3.6.8-1 notes 2 and 3 keep.
    pub(crate) const fn collapses_instances(self) -> bool {
        match self.fallback {
            Some(fallback) => fallback.collapses_instances(),
            None => false,
        }
    }

    /// The family an instance of `family` is written as (Table 4.3.6.8-1).
    pub(crate) const fn written_family(self, family: AttributeType) -> AttributeType {
        match self.fallback {
            Some(fallback) => fallback.family(family),
            None => family,
        }
    }

    /// Whether an instance written as `family` keeps `member` (Tables
    /// 4.3.6.8-2 and -3).
    pub(crate) const fn keeps_instance_member(self, family: AttributeType, member: InstanceMember) -> bool {
        match self.fallback {
            Some(fallback) => fallback.keeps_instance_member(family, member),
            None => true,
        }
    }
}
