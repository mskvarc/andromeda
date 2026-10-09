//! Which clause 4.9 target value a local attribute path addresses.
//!
//! Clause 4.9 defines the target element as the last name of the attribute
//! path, and the target value as whatever that element carries. A path may
//! stop at the attribute itself, descend a `.`-separated sub-attribute chain,
//! or carry one trailing `[…]` group. Only the first of those is the typed
//! value of an attribute instance alone, and a target element that
//! is a `LanguageProperty` (at the top level or below a sub-attribute) may
//! be read with or without a target language. The routing decision is this
//! classification, made once, rather than a boolean re-derived at every call
//! site.

use crate::q_predicate::language_map::LanguageSelection;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::path::AttrPath;

/// What a local `q` attribute path addresses, and therefore which compilation
/// can answer a predicate over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalTarget<'a> {
    /// The attribute's own value, which a row holds in one typed column per
    /// attribute instance.
    OwnValue,
    /// One trailing member, written `attr[member]`.
    ///
    /// Clause 4.9 gives that spelling two readings and the stored row decides
    /// which applies: on a `LanguageProperty` the member is the target language
    /// and selects one `languageMap` key, and on every other family it is a
    /// member of a compound value. The two are indistinguishable in the query
    /// text (a language tag and a JSON member name are both bare names), so
    /// both readings are compiled and disjoined.
    TrailingMember(&'a AttributeName),
    /// A compound member nested below another, which lives only inside a
    /// structured value and has no language reading.
    NestedMember,
    /// A sub-attribute, held in the sub-attribute tree of its attribute's
    /// instance.
    ///
    /// The sub-attribute is the target element, and when it is a
    /// `LanguageProperty` clause 4.9 reads it through its `languageMap` exactly
    /// as it reads a top-level one: with no trailing group (or `[*]`) every
    /// string is a candidate, and a one-member group is the target language.
    /// `None` when the trailing group is a nested compound path, which has no
    /// such reading.
    SubAttribute(Option<LanguageSelection<'a>>),
}

/// Classify what a local attribute path addresses.
///
/// The trailing member is read in its pre-expansion spelling: a target language
/// is a BCP 47 tag written by the client and stored verbatim as the `@language`
/// of an expanded `languageMap` entry, never a term the request `@context`
/// expands to an IRI.
#[must_use]
pub const fn local_target(path: &AttrPath) -> LocalTarget<'_> {
    let at_top_level = path.sub_attrs.is_empty();
    // `[*]` carries no member at all: it suppresses language selection rather
    // than naming one, which is the same target as a bare path.
    let trailing = if path.value_path.is_empty() {
        [].as_slice()
    } else {
        path.value_path_compact.as_slice()
    };
    match (at_top_level, trailing) {
        (true, []) => LocalTarget::OwnValue,
        (true, [member]) => LocalTarget::TrailingMember(member),
        (true, [_, _, ..]) => LocalTarget::NestedMember,
        (false, []) => LocalTarget::SubAttribute(Some(LanguageSelection::AnyLanguage)),
        (false, [member]) => LocalTarget::SubAttribute(Some(LanguageSelection::TargetLanguage(member))),
        (false, [_, _, ..]) => LocalTarget::SubAttribute(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        language_map::LanguageSelection,
        local_target::{LocalTarget, local_target},
    };
    use andromeda_ngsi_ld_query::filter::{
        expression::Expr,
        parser::parse,
        path::{AttrPath, QueryPath},
    };

    /// The local attribute path of a filter holding one predicate.
    fn path_of(q: &str) -> AttrPath {
        let expr = parse(q).expect("parse q");
        let Expr::Pred(pred) = expr else {
            panic!("a single predicate");
        };
        let QueryPath::Local(local) = pred.path() else {
            panic!("a local path");
        };
        // clone: the test owns the path beyond the parsed expression it borrows from
        local.clone()
    }

    #[test]
    fn a_bare_attribute_addresses_its_own_value() {
        assert_eq!(local_target(&path_of("\"https://example.com/color\"==\"red\"")), LocalTarget::OwnValue);
    }

    /// Clause 4.9: `[*]` matches a `LanguageProperty` against every language's
    /// value, which is the same target the unqualified path addresses: "a
    /// value from any of the key-value pairs held within the languageMap".
    #[test]
    fn the_language_suppression_marker_addresses_the_attributes_own_value() {
        assert_eq!(local_target(&path_of("\"https://example.com/color\"[*]==\"red\"")), LocalTarget::OwnValue);
    }

    #[test]
    fn one_trailing_member_is_carried_in_its_pre_expansion_spelling() {
        let path = path_of("\"https://example.com/color\"[en]==\"red\"");
        match local_target(&path) {
            LocalTarget::TrailingMember(member) => pretty_assertions::assert_eq!(member.as_str(), "en"),
            other @ (LocalTarget::OwnValue | LocalTarget::NestedMember | LocalTarget::SubAttribute(_)) => {
                panic!("expected a trailing member, got {other:?}")
            }
        }
    }

    /// Clause 4.9: the target element is the last name of the dotted path, so
    /// a sub-attribute that is a `LanguageProperty` reads every string with no
    /// trailing group.
    #[test]
    fn a_sub_attribute_path_reads_every_language_of_the_sub_attribute() {
        assert_eq!(
            local_target(&path_of(
                "\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">\"2024-01-01T00:00:00Z\""
            )),
            LocalTarget::SubAttribute(Some(LanguageSelection::AnyLanguage))
        );
    }

    /// One trailing member below a sub-attribute is that sub-attribute's
    /// target language, in the spelling the client wrote.
    #[test]
    fn one_member_below_a_sub_attribute_is_its_target_language() {
        let path = path_of("\"https://example.com/door\".\"https://example.com/label\"[en]==\"Front\"");
        match local_target(&path) {
            LocalTarget::SubAttribute(Some(LanguageSelection::TargetLanguage(member))) => pretty_assertions::assert_eq!(member.as_str(), "en"),
            other @ (LocalTarget::OwnValue
            | LocalTarget::TrailingMember(_)
            | LocalTarget::NestedMember
            | LocalTarget::SubAttribute(None | Some(LanguageSelection::AnyLanguage))) => panic!("expected a sub-attribute target language, got {other:?}"),
        }
    }

    #[test]
    fn a_nested_compound_path_below_a_sub_attribute_has_no_language_reading() {
        assert_eq!(
            local_target(&path_of("\"https://example.com/sensor\".\"https://example.com/rawdata\"[a.b]==40")),
            LocalTarget::SubAttribute(None)
        );
    }

    #[test]
    fn a_nested_compound_member_has_no_language_reading() {
        assert_eq!(local_target(&path_of("\"https://example.com/sensor\"[a.b]==40")), LocalTarget::NestedMember);
    }
}
