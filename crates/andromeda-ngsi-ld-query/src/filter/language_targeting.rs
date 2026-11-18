//! Which target languages a `q` bracket may name.
//!
//! The clause 4.9 ABNF writes the bracket as `[DottedPath]`, and its
//! `AttrName` admits no `-`, so the specification's reading takes `color[en]`
//! and refuses `marque[en-GB]`, the target language the informative Annex
//! C.5.13.2 prints. The `q_target_languages` extension widens what the bracket
//! may name one level at a time, each level admitting everything the one below
//! it does: [`LanguageTargeting::LanguageTags`] admits every RFC 5646 tag and
//! keeps the clause 4.9 tag equality, and [`LanguageTargeting::LanguageRanges`]
//! also admits RFC 4647 extended ranges and reads every target language as
//! one. A filter is parsed whole under every level, so a stored filter keeps
//! round-tripping through its text, and the level is held to it on admission.

use crate::filter::{
    Filter,
    filter_error::FilterError,
    language_matching::LanguageMatching,
    target_language::TargetLanguage,
    trailing_group::TrailingGroup,
    value_path::ValuePath,
};
use strum::Display;

/// What a `q` bracket may name as a target language, in increasing order.
///
/// The default is the specification's own grammar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum LanguageTargeting {
    /// The clause 4.9 ABNF: a target language is a one-member bracket an
    /// `AttrName` spells (`[en]`, `[de]`).
    #[default]
    Specification,
    /// Every RFC 5646 language tag (`[en-GB]`, `[zh-Hant-TW]`), matched by
    /// tag equality.
    LanguageTags,
    /// Every RFC 5646 tag and every RFC 4647 extended range (`[de-*-DE]`),
    /// matched by RFC 4647 section 3.3.2 extended filtering.
    LanguageRanges,
}

impl LanguageTargeting {
    /// How a target language admitted under this level selects languageMap
    /// keys.
    #[must_use]
    pub const fn matching(self) -> LanguageMatching {
        match self {
            LanguageTargeting::Specification | LanguageTargeting::LanguageTags => LanguageMatching::TagEquality,
            LanguageTargeting::LanguageRanges => LanguageMatching::ExtendedFiltering,
        }
    }

    /// The least level admitting every target language `expr` names.
    #[must_use]
    pub fn required_by(expr: &Filter) -> LanguageTargeting {
        expr.terminal_paths().into_iter().map(required_by_path).max().unwrap_or_default()
    }

    /// Admit a filter a request carries under this level.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::LanguageTagNotAdmitted`] for a target language
    /// only a full tag spells below [`LanguageTargeting::LanguageTags`], and
    /// [`FilterError::LanguageRangeNotAdmitted`] for an extended range below
    /// [`LanguageTargeting::LanguageRanges`].
    pub fn admit(self, expr: &Filter) -> Result<(), FilterError> {
        expr.terminal_paths().into_iter().try_for_each(|path| self.admit_path(path))
    }

    /// Admit one attribute path, a `q` Query Term's or an `orderBy` term's,
    /// under this level.
    ///
    /// # Errors
    ///
    /// As [`LanguageTargeting::admit`].
    pub fn admit_path(self, path: &ValuePath) -> Result<(), FilterError> {
        if required_by_path(path) <= self {
            return Ok(());
        }
        match &path.trailing {
            // clone: the error outlives the filter that names the language
            Some(TrailingGroup::Language(TargetLanguage::Tag(tag))) => Err(FilterError::LanguageTagNotAdmitted(tag.clone())),
            // clone: the error outlives the filter that names the language
            Some(TrailingGroup::Language(TargetLanguage::Range(range))) => Err(FilterError::LanguageRangeNotAdmitted(range.clone())),
            Some(TrailingGroup::Members(_) | TrailingGroup::AllLanguages) | None => Ok(()),
        }
    }
}

/// The least level admitting the target language `path` names.
///
/// A one-member group an `AttrName` spells is clause 4.9's own target
/// language, and `[*]` is clause 4.9's every-language marker.
const fn required_by_path(path: &ValuePath) -> LanguageTargeting {
    match &path.trailing {
        Some(TrailingGroup::Language(TargetLanguage::Tag(_))) => LanguageTargeting::LanguageTags,
        Some(TrailingGroup::Language(TargetLanguage::Range(_))) => LanguageTargeting::LanguageRanges,
        Some(TrailingGroup::Members(_) | TrailingGroup::AllLanguages) | None => LanguageTargeting::Specification,
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{Filter, filter_error::FilterError, language_matching::LanguageMatching, language_targeting::LanguageTargeting};
    use pretty_assertions::assert_eq;

    fn filter(q: &str) -> Filter {
        q.parse().expect("a clause 4.9 filter")
    }

    const LEVELS: [LanguageTargeting; 3] = [
        LanguageTargeting::Specification,
        LanguageTargeting::LanguageTags,
        LanguageTargeting::LanguageRanges,
    ];

    /// Clause 4.9 `AttrName` spells `en`, and `[*]` is the clause's own marker.
    #[test]
    fn a_target_language_the_abnf_spells_is_admitted_at_every_level() {
        for q in ["name[en]==\"x\"", "name[*]==\"x\"", "address[city]==\"Berlin\"", "speed>1", "rel{name[de]}"] {
            for level in LEVELS {
                assert!(level.admit(&filter(q)).is_ok(), "{q} at {level}");
            }
            assert_eq!(LanguageTargeting::required_by(&filter(q)), LanguageTargeting::Specification, "{q}");
        }
    }

    #[test]
    fn a_full_tag_needs_the_language_tags_level() {
        let tagged = filter("name[en-GB]==\"x\";speed>1");
        let error = LanguageTargeting::Specification.admit(&tagged).expect_err("no AttrName spells en-GB");
        assert!(
            matches!(&error, FilterError::LanguageTagNotAdmitted(tag) if tag.as_str() == "en-GB"),
            "{error:?}"
        );
        assert!(LanguageTargeting::LanguageTags.admit(&tagged).is_ok());
        assert!(LanguageTargeting::LanguageRanges.admit(&tagged).is_ok());
        assert_eq!(LanguageTargeting::required_by(&tagged), LanguageTargeting::LanguageTags);
    }

    #[test]
    fn a_range_needs_the_language_ranges_level() {
        let ranged = filter("name[de-*-DE]==\"x\";label[en-GB]==\"y\"");
        for level in [LanguageTargeting::Specification, LanguageTargeting::LanguageTags] {
            let error = level.admit(&ranged).expect_err("a range needs the ranges level");
            assert!(
                matches!(error, FilterError::LanguageRangeNotAdmitted(_) | FilterError::LanguageTagNotAdmitted(_)),
                "{error:?}"
            );
        }
        let error = LanguageTargeting::LanguageTags.admit(&ranged).expect_err("a range needs the ranges level");
        assert!(matches!(error, FilterError::LanguageRangeNotAdmitted(_)), "{error:?}");
        assert!(LanguageTargeting::LanguageRanges.admit(&ranged).is_ok());
        assert_eq!(LanguageTargeting::required_by(&ranged), LanguageTargeting::LanguageRanges);
    }

    #[test]
    fn a_target_language_behind_a_hop_is_held_to_the_level_too() {
        assert!(LanguageTargeting::Specification.admit(&filter("rel{name[en-GB]}")).is_err());
        assert!(LanguageTargeting::LanguageTags.admit(&filter("rel{name[*-GB]}")).is_err());
    }

    #[test]
    fn only_the_ranges_level_filters_by_range() {
        assert_eq!(LanguageTargeting::Specification.matching(), LanguageMatching::TagEquality);
        assert_eq!(LanguageTargeting::LanguageTags.matching(), LanguageMatching::TagEquality);
        assert_eq!(LanguageTargeting::LanguageRanges.matching(), LanguageMatching::ExtendedFiltering);
    }

    #[test]
    fn the_levels_are_ordered_and_spelled_in_kebab_case() {
        assert!(LanguageTargeting::Specification < LanguageTargeting::LanguageTags);
        assert!(LanguageTargeting::LanguageTags < LanguageTargeting::LanguageRanges);
        assert_eq!(LanguageTargeting::LanguageTags.to_string(), "language-tags");
        assert_eq!(LanguageTargeting::default(), LanguageTargeting::Specification);
    }
}
