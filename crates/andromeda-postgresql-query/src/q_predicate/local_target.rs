//! Which clause 4.9 target value the trailing group of a local attribute path
//! addresses.
//!
//! Clause 4.9 defines the target element as the last name of the attribute
//! path, and the target value as whatever that element carries. A path may
//! stop at the element itself or carry one trailing `[…]` group, and a target
//! element that is a `LanguageProperty` (at the top level or below a
//! sub-attribute) may be read with or without a target language. The routing
//! decision is this classification, made once, rather than a boolean
//! re-derived at every call site. Whether the element is an attribute or a
//! sub-attribute is the path's `.` axis, which the caller reads off the path.

use andromeda_ngsi_ld_query::filter::{
    path::AttrPath,
    target_language::LanguageSelector,
    trailing_path::{MemberPath, TrailingPath},
};

/// What the trailing group of a local `q` attribute path addresses, and
/// therefore which compilation can answer a predicate over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalTarget<'a> {
    /// The element's own value, which a row holds in one typed column per
    /// instance: no trailing group, or `[*]`, which suppresses language
    /// selection rather than naming one and so addresses the target the bare
    /// path addresses.
    OwnValue,
    /// One trailing member, written `attr[member]`, with the target language
    /// it also spells when it is a language tag.
    ///
    /// Clause 4.9 gives that spelling two readings and the stored row decides
    /// which applies: on a `LanguageProperty` the member is the target language
    /// and selects `languageMap` keys, and on every other family it is a member
    /// of a compound value. The two are indistinguishable in the query text, so
    /// both readings are compiled and disjoined.
    TrailingMember {
        /// The one member, in both its spellings.
        members: &'a MemberPath,
        /// The target language the member spells, `None` when it is no tag.
        language: Option<LanguageSelector<'a>>,
    },
    /// A target language no member name spells (`[en-GB]`, `[de-*-DE]`), which
    /// only a `LanguageProperty` can hold.
    TargetLanguage(LanguageSelector<'a>),
    /// A compound member nested below another, which lives only inside a
    /// structured value and has no language reading.
    NestedMember(&'a MemberPath),
}

/// Classify what the trailing group of a local attribute path addresses.
///
/// The target language is read in its pre-expansion spelling: a language tag
/// is written by the client and stored verbatim as the `@language` of an
/// expanded `languageMap` entry, never a term the request `@context` expands
/// to an IRI.
#[must_use]
pub fn local_target(path: &AttrPath) -> LocalTarget<'_> {
    match &path.trailing {
        None | Some(TrailingPath::AllLanguages) => LocalTarget::OwnValue,
        Some(TrailingPath::Language(language)) => LocalTarget::TargetLanguage(language.selector()),
        Some(TrailingPath::Members(members)) => match members.addressed.as_slice() {
            [_] => LocalTarget::TrailingMember {
                members,
                language: path.target_language(),
            },
            [] | [_, _, ..] => LocalTarget::NestedMember(members),
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::local_target::{LocalTarget, local_target};
    use andromeda_ngsi_ld_query::filter::{
        expression::Expr,
        parser::parse,
        path::{AttrPath, QueryPath},
        target_language::LanguageSelector,
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
        local.clone()
    }

    /// The language `q`'s trailing group spells, as text.
    fn language_of(target: LocalTarget<'_>) -> Option<&str> {
        match target {
            LocalTarget::TrailingMember { language, .. } => language.map(LanguageSelector::as_str),
            LocalTarget::TargetLanguage(language) => Some(language.as_str()),
            LocalTarget::OwnValue | LocalTarget::NestedMember(_) => None,
        }
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
    fn one_trailing_member_spelling_a_tag_carries_its_target_language() {
        let path = path_of("\"https://example.com/color\"[en]==\"red\"");
        let target = local_target(&path);
        assert!(matches!(target, LocalTarget::TrailingMember { language: Some(_), .. }), "{target:?}");
        pretty_assertions::assert_eq!(language_of(target), Some("en"));
    }

    #[test]
    fn one_trailing_member_spelling_no_tag_has_no_language_reading() {
        let path = path_of("\"https://example.com/address\"[city_name]==\"x\"");
        assert!(matches!(local_target(&path), LocalTarget::TrailingMember { language: None, .. }));
    }

    /// Annex C.5.13.2: `marque[en-GB]` is a target language no member name
    /// spells.
    #[test]
    fn a_hyphenated_tag_is_a_target_language_alone() {
        let path = path_of("\"https://example.com/marque\"[en-GB]==\"x\"");
        let target = local_target(&path);
        assert!(matches!(target, LocalTarget::TargetLanguage(_)), "{target:?}");
        pretty_assertions::assert_eq!(language_of(target), Some("en-GB"));
    }

    /// One trailing member below a sub-attribute is that sub-attribute's
    /// target language, in the spelling the client wrote.
    #[test]
    fn one_member_below_a_sub_attribute_is_its_target_language() {
        let path = path_of("\"https://example.com/door\".\"https://example.com/label\"[en]==\"Front\"");
        pretty_assertions::assert_eq!(language_of(local_target(&path)), Some("en"));
    }

    #[test]
    fn a_nested_compound_member_has_no_language_reading() {
        for q in [
            "\"https://example.com/sensor\"[a.b]==40",
            "\"https://example.com/sensor\".\"https://example.com/rawdata\"[a.b]==40",
        ] {
            let path = path_of(q);
            assert!(matches!(local_target(&path), LocalTarget::NestedMember(_)), "{q}");
        }
    }
}
