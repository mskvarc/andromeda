//! The clause 4.9 `ValuePath`: an attribute, its sub-attributes and its trailing `[…]` group.

use crate::filter::{
    instance_metadata::InstanceMetadata,
    target_language::LanguageSelector,
    trailing_group::{MemberPath, TrailingGroup},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
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
/// [`AttributePath::Linked`], whose terminal segment is an `ValuePath` again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValuePath {
    /// The top-level attribute name (compact or expanded IRI).
    pub attribute: AttributeName,
    /// Sub-attribute names for navigating nested attributes.
    pub sub_attributes: Vec<AttributeName>,
    /// The trailing `[…]` group, `None` when the path has none.
    pub trailing: Option<TrailingGroup>,
}

impl ValuePath {
    /// Whether this path names one attribute and navigates no further.
    ///
    /// The clause 4.23 keywords `id`, `type` and `scope` name structural
    /// members of the entity representation, and the grammar spells each as a
    /// bare `AttrName`. A path that descends into one of them is therefore an
    /// attribute path that happens to start with the keyword's spelling, not
    /// the keyword.
    #[must_use]
    pub const fn is_simple(&self) -> bool {
        self.sub_attributes.is_empty() && self.trailing.is_none()
    }

    /// Build a path addressing an attribute's own value, with no sub-attribute
    /// and no trailing group.
    #[must_use]
    pub const fn simple(attr: AttributeName) -> ValuePath {
        ValuePath {
            attribute: attr,
            sub_attributes: Vec::new(),
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
            None | Some(TrailingGroup::AllLanguages) => {
                let (last, above) = self.sub_attributes.split_last()?;
                Some((InstanceMetadata::named(last)?, above))
            }
            Some(TrailingGroup::Language(_) | TrailingGroup::Members(_)) => None,
        }
    }

    /// The compound-value members the trailing group addresses, `None` when
    /// it holds a target language only or the path has no group.
    #[must_use]
    pub const fn members(&self) -> Option<&MemberPath> {
        match &self.trailing {
            Some(TrailingGroup::Members(members)) => Some(members),
            Some(TrailingGroup::Language(_) | TrailingGroup::AllLanguages) | None => None,
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
            Some(TrailingGroup::Language(language)) => Some(language.selector()),
            Some(TrailingGroup::Members(members)) => match members.written.as_slice() {
                [only] => LangTag::new(only.as_str()).ok().map(LanguageSelector::Tag),
                [] | [_, _, ..] => None,
            },
            Some(TrailingGroup::AllLanguages) | None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{Filter, instance_metadata::InstanceMetadata, value_path::ValuePath};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

    /// The local path the one Query Term of `q` ends on.
    fn terminal(q: &str) -> ValuePath {
        match q.parse::<Filter>().expect("a clause 4.9 filter") {
            Filter::Term(predicate) => predicate.path().terminal().clone(),
            other @ (Filter::And(_) | Filter::Or(_)) => panic!("{q} is no single term: {other:?}"),
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
