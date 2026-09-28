//! What an Entity has to carry for a clause 4.21 projection to leave it any
//! member, read as a selection criterion.
//!
//! Clauses 5.7.2.5 and 5.7.4.5 reduce "every Entity within the payload body"
//! down to the Entity members the projection defines. An Entity the projection
//! would reduce to no member at all is not answered with an empty object: the
//! ETSI conformance test `019_17_01` queries `type=Building,Vehicle&pick=name`
//! over a Vehicle that has no `name` and expects the Buildings alone, and
//! clause 5.7.1.4 answers the same case on a retrieval with `ResourceNotFound`.
//! Such an Entity is therefore not selected. That has to be decided where the
//! other selection criteria are, before the page is cut and before
//! `NGSILD-Results-Count` is taken: clause 6.3.13 makes the count "the total
//! number of matching results", and clause 5.5.9.1 lets a page fall short of
//! its limit only for its payload size, so an Entity left out after the page was
//! cut makes both lie.
//!
//! The member rule is clause 4.21's own. A projection that keeps `id` or `type`
//! leaves every Entity a member, since every Entity has both. Otherwise the
//! Entity keeps a member through its `scope`, when the projection keeps that
//! keyword and the Entity has one, or through an Attribute the projection
//! keeps. A `pick` term that reaches into a linked entity (`rel{a}`) keeps `rel`
//! itself; an `omit` term that does (`omit=rel{a}`) removes `a` inside the
//! linked entity and not `rel`, so only a bare `omit` term takes an Attribute
//! away.

use crate::projection::{compiled::Projection, term::ProjectionTerm};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use iri_rs::{Iri, IriBuf};

/// The expanded members clause 5.2.4 defines on an Entity beside its
/// Attributes that a stored Entity can carry under an expanded name: `scope`
/// (clause 4.18), `expiresAt` (clause 4.22) and the clause 4.8 system
/// timestamps.
///
/// None of them is an Attribute, so none of them keeps an Entity alive under an
/// `omit`; `scope` does so on its own terms, through [`ScopeRetention`].
const NON_ATTRIBUTE_MEMBERS: [Iri<&'static str>; 5] = [
    properties::SCOPE,
    properties::EXPIRES_AT,
    properties::CREATED_AT,
    properties::MODIFIED_AT,
    properties::DELETED_AT,
];

/// Whether a projection keeps the `scope` member of the Entities it reduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeRetention {
    /// An Entity with a `scope` keeps it, and with it a member.
    Kept,
    /// The projection takes `scope` away.
    Removed,
}

/// What an Entity has to carry for the projection to leave it a member.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProjectedMembers {
    /// Every Entity keeps a member: the request named no projection, or one
    /// that keeps `id` or `type`.
    #[default]
    Unconditional,
    /// A `pick` that keeps neither `id` nor `type`: the Entity keeps a member
    /// when it carries one of these Attributes, or a `scope` the pick keeps.
    AnyOf {
        /// The expanded names of the Attributes the pick keeps.
        attributes: Vec<IriBuf>,
        /// Whether the pick keeps `scope`.
        scope: ScopeRetention,
    },
    /// An `omit` that removes both `id` and `type`: the Entity keeps a member
    /// when it carries an Attribute other than these, or a `scope` the omit
    /// spares.
    AnyBesides {
        /// The expanded names of the Attributes the omit removes.
        attributes: Vec<IriBuf>,
        /// Whether the omit spares `scope`.
        scope: ScopeRetention,
    },
}

impl ProjectedMembers {
    /// What `projection` requires of an Entity to leave it a member.
    #[must_use]
    pub fn of(projection: &Projection) -> ProjectedMembers {
        if projection.is_empty() || projection.keeps(&ProjectionTerm::Id) || projection.keeps(&ProjectionTerm::Type) {
            return ProjectedMembers::Unconditional;
        }
        let scope = if projection.keeps(&ProjectionTerm::Scope) {
            ScopeRetention::Kept
        } else {
            ScopeRetention::Removed
        };
        if projection.pick.is_empty() {
            let attributes = projection
                .omit
                .iter()
                .filter(|term| term.linked_terms().is_empty())
                .filter_map(attribute_iri)
                .cloned() // clone: the selection owns its names independently of the projection it was read from
                .collect();
            ProjectedMembers::AnyBesides { attributes, scope }
        } else {
            let attributes = projection
                .pick
                .iter()
                .filter_map(attribute_iri)
                .cloned() // clone: as above
                .collect();
            ProjectedMembers::AnyOf { attributes, scope }
        }
    }

    /// Whether this requirement can leave an Entity out at all.
    #[must_use]
    pub const fn constrains(&self) -> bool {
        match self {
            ProjectedMembers::Unconditional => false,
            ProjectedMembers::AnyOf { .. } | ProjectedMembers::AnyBesides { .. } => true,
        }
    }

    /// The expanded member names whose presence this requirement reads.
    ///
    /// For [`ProjectedMembers::AnyOf`] the Attributes that keep an Entity; for
    /// [`ProjectedMembers::AnyBesides`] the names that do not, which are the
    /// omitted Attributes together with the members clause 5.2.4 defines beside
    /// Attributes. Empty for [`ProjectedMembers::Unconditional`].
    pub fn names(&self) -> impl Iterator<Item = &str> {
        let (attributes, besides): (&[IriBuf], &[Iri<&'static str>]) = match self {
            ProjectedMembers::Unconditional => (&[], &[]),
            ProjectedMembers::AnyOf { attributes, .. } => (attributes, &[]),
            ProjectedMembers::AnyBesides { attributes, .. } => (attributes, &NON_ATTRIBUTE_MEMBERS),
        };
        attributes.iter().map(IriBuf::as_str).chain(besides.iter().map(Iri::as_str))
    }
}

/// The expanded name of an Attribute term; `None` for a clause 4.21 keyword.
const fn attribute_iri(term: &ProjectionTerm) -> Option<&IriBuf> {
    match term {
        ProjectionTerm::Attribute { iri, .. } => Some(iri),
        ProjectionTerm::Id | ProjectionTerm::Type | ProjectionTerm::Scope => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::projection::{
        compiled::Projection,
        projected_members::{ProjectedMembers, ScopeRetention},
        term::ProjectionTerm,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn attribute(name: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(&format!("https://example.com/{name}")),
            linked: Vec::new(),
        }
    }

    fn linked(name: &str, inner: &str) -> ProjectionTerm {
        ProjectionTerm::Attribute {
            iri: iri(&format!("https://example.com/{name}")),
            linked: vec![attribute(inner)],
        }
    }

    /// A request that named no projection leaves every Entity whole.
    #[test]
    fn no_projection_selects_every_entity() {
        assert_eq!(ProjectedMembers::of(&Projection::default()), ProjectedMembers::Unconditional);
        assert!(!ProjectedMembers::Unconditional.constrains());
    }

    /// Clause 4.21: every Entity has an `id` and a `type`, so a projection that
    /// keeps either leaves every Entity a member.
    #[test]
    fn a_projection_keeping_id_or_type_selects_every_entity() {
        for projection in [
            Projection::builder().pick(vec![ProjectionTerm::Id]).build(),
            Projection::builder().pick(vec![attribute("name"), ProjectionTerm::Type]).build(),
            Projection::builder().omit(vec![ProjectionTerm::Id, attribute("name")]).build(),
            Projection::builder().omit(vec![attribute("name")]).build(),
        ] {
            assert_eq!(ProjectedMembers::of(&projection), ProjectedMembers::Unconditional, "{projection:?}");
        }
    }

    /// ETSI TP `019_17_01`: `pick=name` keeps an Entity only through `name`.
    #[test]
    fn a_pick_of_attributes_selects_the_entities_carrying_one_of_them() {
        let projection = Projection::builder().pick(vec![attribute("name"), linked("owner", "name")]).build();
        let members = ProjectedMembers::of(&projection);
        assert_eq!(
            members,
            ProjectedMembers::AnyOf {
                attributes: vec![iri("https://example.com/name"), iri("https://example.com/owner")],
                scope: ScopeRetention::Removed,
            }
        );
        assert_eq!(
            members.names().collect::<Vec<_>>(),
            vec!["https://example.com/name", "https://example.com/owner"]
        );
    }

    /// A pick naming `scope` keeps an Entity through its scope as well.
    #[test]
    fn a_pick_naming_scope_keeps_the_scope() {
        let projection = Projection::builder().pick(vec![ProjectionTerm::Scope]).build();
        assert_eq!(
            ProjectedMembers::of(&projection),
            ProjectedMembers::AnyOf {
                attributes: Vec::new(),
                scope: ScopeRetention::Kept,
            }
        );
    }

    /// An `omit` removing `id` and `type` keeps an Entity through any other
    /// Attribute, and a linked `omit` term removes nothing from the Entity
    /// itself.
    #[test]
    fn an_omit_of_id_and_type_selects_the_entities_carrying_another_attribute() {
        let projection = Projection::builder()
            .omit(vec![ProjectionTerm::Id, ProjectionTerm::Type, attribute("name"), linked("owner", "name")])
            .build();
        let members = ProjectedMembers::of(&projection);
        assert_eq!(
            members,
            ProjectedMembers::AnyBesides {
                attributes: vec![iri("https://example.com/name")],
                scope: ScopeRetention::Kept,
            }
        );
        let names: Vec<&str> = members.names().collect();
        assert_eq!(names.first(), Some(&"https://example.com/name"));
        for member in [properties::SCOPE, properties::EXPIRES_AT, properties::CREATED_AT, properties::MODIFIED_AT] {
            assert!(names.contains(&member.as_str()), "{member} is no Attribute: {names:?}");
        }
    }

    /// An `omit` naming `scope` too takes the scope away as a member.
    #[test]
    fn an_omit_naming_scope_removes_the_scope() {
        let projection = Projection::builder()
            .omit(vec![ProjectionTerm::Id, ProjectionTerm::Type, ProjectionTerm::Scope])
            .build();
        assert!(matches!(
            ProjectedMembers::of(&projection),
            ProjectedMembers::AnyBesides {
                scope: ScopeRetention::Removed,
                ..
            }
        ));
    }
}
