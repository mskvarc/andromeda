//! Attribute paths addressed by a `q` predicate (clause 4.9).

use crate::{
    error::QueryError,
    filter::{
        instance_metadata::InstanceMetadata,
        target_language::LanguageSelector,
        trailing_path::{MemberPath, TrailingPath},
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use iri_rs::IriBuf;
use langtag::LangTag;

/// An attribute path within a `q` expression, potentially including sub-attributes
/// and a trailing `[…]` group.
///
/// Example: `address[city].streetName` would have `attr = "address"`,
/// `sub_attrs = ["city"]`, and a trailing member path `["streetName"]`.
///
/// `attr` and `sub_attrs` are [`AttributeName`]s, which accept a compact short
/// name, a prefixed name or an expanded IRI. That is exactly the range this AST
/// spans: a freshly parsed filter carries whatever the client wrote, and an
/// expanded one carries IRIs.
///
/// This type models the two *local* traversal axes: `.` sub-attributes and the
/// `[]` group. The third clause 4.9 axis, the `{}` linked-entity traversal,
/// crosses an entity boundary and is modelled one level up by
/// [`QueryPath::Linked`], whose terminal segment is an `AttrPath` again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttrPath {
    /// The top-level attribute name (compact or expanded IRI).
    pub attr: AttributeName,
    /// Sub-attribute names for navigating nested attributes.
    pub sub_attrs: Vec<AttributeName>,
    /// The trailing `[…]` group, `None` when the path has none.
    pub trailing: Option<TrailingPath>,
}

impl AttrPath {
    /// Whether this path names one attribute and navigates no further.
    ///
    /// The clause 4.23 keywords `id`, `type` and `scope` name structural
    /// members of the entity representation, and the grammar spells each as a
    /// bare `AttrName`. A path that descends into one of them is therefore an
    /// attribute path that happens to start with the keyword's spelling, not
    /// the keyword.
    #[must_use]
    pub const fn is_simple(&self) -> bool {
        self.sub_attrs.is_empty() && self.trailing.is_none()
    }

    /// Build a path addressing an attribute's own value, with no sub-attribute
    /// and no trailing group.
    #[must_use]
    pub const fn simple(attr: AttributeName) -> AttrPath {
        AttrPath {
            attr,
            sub_attrs: Vec::new(),
            trailing: None,
        }
    }

    /// The instance member the last `.` segment names, with the sub-attributes
    /// above it, or `None` when the path ends on an attribute or a
    /// sub-attribute.
    ///
    /// Clause 4.9 makes the last name of the path the target element, and the
    /// members [`InstanceMetadata`] lists are non-reified Properties of the
    /// instance before them (`temperature.observedAt`). The top-level name is
    /// always an attribute. A trailing group other than `[*]` after such a
    /// member is evaluated "against the seed target value", a String or a
    /// `DateTime` with no member to reach, so "the target element shall be
    /// considered as non-existent": the path ends on no member, and the name
    /// stays a sub-attribute name no Attribute carries.
    #[must_use]
    pub fn ending_member(&self) -> Option<(InstanceMetadata, &[AttributeName])> {
        match self.trailing {
            None | Some(TrailingPath::AllLanguages) => {
                let (last, above) = self.sub_attrs.split_last()?;
                Some((InstanceMetadata::named(last)?, above))
            }
            Some(TrailingPath::Language(_) | TrailingPath::Members(_)) => None,
        }
    }

    /// The compound-value members the trailing group addresses, `None` when
    /// it holds a target language only or the path has no group.
    #[must_use]
    pub const fn members(&self) -> Option<&MemberPath> {
        match &self.trailing {
            Some(TrailingPath::Members(members)) => Some(members),
            Some(TrailingPath::Language(_) | TrailingPath::AllLanguages) | None => None,
        }
    }

    /// The target language the trailing group names when the attribute is a
    /// `LanguageProperty`.
    ///
    /// Clause 4.9 gives `attr[member]` two readings, and the target element
    /// decides which applies: a `LanguageProperty`'s value is a language map,
    /// whose one-member group is the target language (`color[en]=="red"`);
    /// every other family's is a compound member. The language is the member as
    /// the client wrote it, because a language tag is never a term a
    /// `@context` expands.
    #[must_use]
    pub fn target_language(&self) -> Option<LanguageSelector<'_>> {
        match &self.trailing {
            Some(TrailingPath::Language(language)) => Some(language.selector()),
            Some(TrailingPath::Members(members)) => match members.written.as_slice() {
                [only] => LangTag::new(only.as_str()).ok().map(LanguageSelector::Tag),
                [] | [_, _, ..] => None,
            },
            Some(TrailingPath::AllLanguages) | None => None,
        }
    }
}

/// The attribute axis a `q` predicate compares along (clause 4.9 `Attribute`).
///
/// Clause 4.9 gives an attribute reference three axes. Two of them stay inside
/// one entity and are carried by [`AttrPath`]. The third (the
/// `LinkedEntityRelation` production, written `rel{…}`) crosses a Relationship
/// into the entity it names, and may be chained. This enum is that distinction:
/// a path is either wholly local, or one hop followed by a path that is itself
/// either local or a further hop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryPath {
    /// A path addressing an attribute of the entity under evaluation.
    Local(AttrPath),
    /// A hop across a Relationship into the entity it links to.
    Linked {
        /// The Relationship the hop descends through.
        relationship: AttributeName,
        /// Entity types the linked entity is constrained to, empty when the hop
        /// carried no constraint.
        ///
        /// The clause 4.9 production is
        /// `*1(EntityType 1*("," EntityType) ":")`, so a constraint needs two or
        /// more comma-separated types. That is what keeps the grammar
        /// unambiguous: a lone `prefix:name` inside the braces is a prefixed
        /// attribute name, never a one-element type constraint.
        guard_types: Vec<AttributeName>,
        /// The path evaluated against the linked entity.
        inner: Box<QueryPath>,
    },
}

impl QueryPath {
    /// The local path this axis terminates at, the attribute whose value the
    /// predicate ultimately compares.
    ///
    /// For a hop chain that is the innermost segment, which is the attribute a
    /// value-level concern such as `expandValues` or `jsonKeys` (clause 6.4.3.2)
    /// names: those parameters describe the values being compared, and after a
    /// hop the compared values belong to the linked entity's attribute.
    #[must_use]
    pub fn terminal(&self) -> &AttrPath {
        match self {
            QueryPath::Local(local) => local,
            QueryPath::Linked { inner, .. } => inner.terminal(),
        }
    }

    /// Whether this axis crosses into a linked entity.
    ///
    /// Testing the outermost segment is enough to answer that for the whole
    /// axis: a hop wraps everything after it, so a [`QueryPath::Local`] can
    /// never have one nested inside.
    #[must_use]
    pub const fn is_linked(&self) -> bool {
        matches!(self, QueryPath::Linked { .. })
    }

    /// How many Relationship hops this axis crosses before it lands on a local
    /// attribute.
    ///
    /// Zero for a wholly local path, one for `rel{attr}`, two for
    /// `rel{rel2{attr}}`, and so on. Clause 5.7.2.4 measures a filter against
    /// `joinLevel` by exactly this count, so the number the traversal is bounded
    /// by and the number the rule checks are the same one.
    #[must_use]
    pub fn linked_depth(&self) -> u32 {
        match self {
            QueryPath::Local(_) => 0,
            QueryPath::Linked { inner, .. } => 1 + inner.linked_depth(),
        }
    }

    /// The local path this axis terminates at, mutably.
    pub fn terminal_mut(&mut self) -> &mut AttrPath {
        match self {
            QueryPath::Local(local) => local,
            QueryPath::Linked { inner, .. } => inner.terminal_mut(),
        }
    }
}

/// Read an expanded IRI as the attribute name that addresses it.
///
/// Term expansion yields an absolute IRI, which the clause 4.6.2 name grammar
/// accepts, so the failure path only opens if a context produced something an
/// attribute cannot be keyed by.
///
/// # Errors
///
/// Returns [`QueryError::AttributeNameInvalid`] when the IRI does not satisfy
/// the clause 4.6.2 attribute-name grammar.
pub fn expanded_attribute_name(iri: &IriBuf) -> Result<AttributeName, QueryError> {
    iri.as_str().parse().map_err(QueryError::AttributeNameInvalid)
}

#[cfg(test)]
mod tests {
    use crate::filter::{expression::Expr, instance_metadata::InstanceMetadata, parser::parse, path::AttrPath};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

    /// The local path the one Query Term of `q` ends on.
    fn terminal(q: &str) -> AttrPath {
        match parse(q).expect("a clause 4.9 filter") {
            Expr::Pred(predicate) => predicate.path().terminal().clone(),
            other @ (Expr::And(_) | Expr::Or(_)) => panic!("{q} is no single term: {other:?}"),
        }
    }

    /// A path ends on an instance member with no trailing group or `[*]`
    /// after it, the sub-attributes above it kept.
    #[test]
    fn a_member_with_no_trailing_group_ends_the_path() {
        for q in [
            "speed.\"https://uri.etsi.org/ngsi-ld/observedAt\"",
            "speed.\"https://uri.etsi.org/ngsi-ld/observedAt\"[*]",
        ] {
            pretty_assertions::assert_eq!(terminal(q).ending_member().map(|(member, _)| member), Some(InstanceMetadata::ObservedAt), "{q}");
        }
        let path = terminal("speed.accuracy.\"https://uri.etsi.org/ngsi-ld/unitCode\"");
        let (member, above) = path.ending_member().expect("a member");
        pretty_assertions::assert_eq!(member, InstanceMetadata::UnitCode);
        pretty_assertions::assert_eq!(above.iter().map(AttributeName::as_str).collect::<Vec<_>>(), ["accuracy"]);
    }

    /// Clause 4.9 evaluates a trailing group against the target value, and a
    /// member's String or `DateTime` has no member to reach.
    #[test]
    fn a_member_before_a_trailing_group_ends_no_path() {
        for trailing in ["[x]", "[a.b]", "[en]", "[en-GB]"] {
            let path = terminal(&format!("speed.\"https://uri.etsi.org/ngsi-ld/observedAt\"{trailing}"));
            pretty_assertions::assert_eq!(path.ending_member(), None, "{trailing}");
        }
    }
}
