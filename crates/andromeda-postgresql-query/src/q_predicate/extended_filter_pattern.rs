//! RFC 4647 section 3.3.2 extended filtering restated as a POSIX regular
//! expression over a `languageMap` entry's `@language`.
//!
//! RFC 4647 section 2 compares subtags case-insensitively, and RFC 5646
//! section 2.1.1 compares whole tags the same way, so the pattern carries the
//! case folding ([`CaseFolding`]). The first range subtag anchors the tag's
//! first subtag, `*` admitting any. Each later range subtag that is not `*` is
//! found further along the tag after skipping zero or more subtags, none of
//! them a singleton: the skipped run is `(-[a-z0-9]{2,8})*`, so a singleton
//! such as the `x` of a private-use sequence is never stepped over. Whatever
//! the tag holds after the last matched subtag is admitted. A validated range
//! or tag holds letters, digits and `-` only, so every subtag is written into
//! the pattern as itself, and the pattern is bound as a parameter.

use std::fmt::Write as _;

use andromeda_ngsi_ld_query::filter::target_language::LanguageSelector;

/// The range wildcard (RFC 4647 section 2.2).
const WILDCARD: &str = "*";

/// Where a pattern folds the case of the tags it admits.
///
/// RFC 5646 tags and RFC 4647 ranges are ASCII, so the fold both standards
/// define is ASCII-only. Under a database whose default locale folds case
/// Turkish-style, an engine-side fold maps `it-IT` to `it-ıt` and loses it
/// against `it-it`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseFolding {
    /// The pattern is written lowercase and the operator folds the case:
    /// `~*` reads the collation of its operands, so the statement collates the
    /// stored tag with `C` and every fold stays ASCII.
    InTheOperator,
    /// The pattern admits both cases itself: every literal letter is written
    /// as a bracket pair and every class holds both cases. jsonpath's
    /// `like_regex` takes no collation, so a fold the pattern carries is the
    /// only one that stays ASCII whatever the database locale is.
    InThePattern,
}

impl CaseFolding {
    /// The subtag-character class of the fold: `InThePattern` holds both
    /// cases, so the range admits uppercase without an operator-side fold.
    fn subtag_run(self, width: &str) -> String {
        match self {
            CaseFolding::InTheOperator => format!("[a-z0-9]{width}"),
            CaseFolding::InThePattern => format!("[a-zA-Z0-9]{width}"),
        }
    }

    /// One literal subtag of the fold: lowercase under `InTheOperator`, where
    /// the operator folds; both cases written out under `InThePattern`.
    fn subtag(self, text: &str) -> String {
        match self {
            CaseFolding::InTheOperator => text.to_ascii_lowercase(),
            CaseFolding::InThePattern => text
                .chars()
                .map(|c| {
                    if c.is_ascii_alphabetic() {
                        format!("[{}{}]", c.to_ascii_uppercase(), c.to_ascii_lowercase())
                    } else {
                        c.to_string()
                    }
                })
                .collect(),
        }
    }
}

/// The POSIX pattern a tag matches, with the case fold `folding` carries,
/// exactly when `language`, read as an extended language range, admits it.
#[must_use]
pub fn extended_filter_pattern(language: LanguageSelector<'_>, folding: CaseFolding) -> String {
    let mut subtags = language.subtags();
    let mut pattern = String::from("^");
    match subtags.next() {
        Some(WILDCARD) | None => pattern.push_str(&folding.subtag_run("{1,8}")),
        Some(first) => pattern.push_str(&folding.subtag(first)),
    }
    for subtag in subtags.filter(|subtag| *subtag != WILDCARD) {
        let _ = write!(pattern, "(-{})*", folding.subtag_run("{2,8}"));
        pattern.push('-');
        pattern.push_str(&folding.subtag(subtag));
    }
    let _ = write!(pattern, "(-{})*$", folding.subtag_run("{1,8}"));
    pattern
}

/// The POSIX pattern a tag matches, with the case fold `folding` carries,
/// exactly when the tag equals `tag` case-insensitively and holds no further
/// subtag: clause 4.9's tag equality.
#[must_use]
pub fn exact_tag_pattern(tag: &str, folding: CaseFolding) -> String {
    format!("^{}$", folding.subtag(tag))
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::extended_filter_pattern::{CaseFolding, exact_tag_pattern, extended_filter_pattern};
    use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, target_language::TargetLanguage};
    use langtag::LangTag;
    use pretty_assertions::assert_eq;
    use regex::RegexBuilder;

    #[test]
    fn the_pattern_anchors_the_first_subtag_and_skips_non_singletons_before_each_later_one() {
        let range = TargetLanguage::parse("de-*-DE").expect("a range");
        assert_eq!(
            extended_filter_pattern(range.selector(), CaseFolding::InTheOperator),
            "^de(-[a-z0-9]{2,8})*-de(-[a-z0-9]{1,8})*$"
        );
        let leading = TargetLanguage::parse("*-GB").expect("a range");
        assert_eq!(
            extended_filter_pattern(leading.selector(), CaseFolding::InTheOperator),
            "^[a-z0-9]{1,8}(-[a-z0-9]{2,8})*-gb(-[a-z0-9]{1,8})*$"
        );
        let tag = TargetLanguage::parse("en").expect("a tag");
        assert_eq!(extended_filter_pattern(tag.selector(), CaseFolding::InTheOperator), "^en(-[a-z0-9]{1,8})*$");
    }

    /// The fold the pattern carries writes both cases of every letter it
    /// names and of every class, so a case-sensitive read of the pattern
    /// admits either case of a subtag.
    #[test]
    fn the_pattern_carried_fold_writes_both_cases_of_every_letter() {
        let range = TargetLanguage::parse("de-*-DE").expect("a range");
        assert_eq!(
            extended_filter_pattern(range.selector(), CaseFolding::InThePattern),
            "^[Dd][Ee](-[a-zA-Z0-9]{2,8})*-[Dd][Ee](-[a-zA-Z0-9]{1,8})*$"
        );
        assert_eq!(exact_tag_pattern("en-GB", CaseFolding::InThePattern), "^[Ee][Nn]-[Gg][Bb]$");
        assert_eq!(exact_tag_pattern("en-GB", CaseFolding::InTheOperator), "^en-gb$");
    }

    /// The statement's `~*` and the evaluator's RFC 4647 section 3.3.2 walk
    /// decide every pair alike. The Rust `regex` crate reads this pattern's
    /// constructs (anchors, classes, bounded repetition, groups) as POSIX does.
    #[test]
    fn the_pattern_agrees_with_extended_filtering_on_every_pair() {
        let ranges = ["de-*-DE", "*-GB", "en-*", "en", "en-GB", "de-DE", "*-Latn", "zh-*-TW", "x-private"];
        let tags = [
            "de",
            "de-DE",
            "de-de",
            "de-Latn-DE",
            "de-Latf-DE",
            "de-DE-x-goethe",
            "de-Latn-DE-1996",
            "de-Deva-DE",
            "de-x-DE",
            "de-Deva",
            "en",
            "en-GB",
            "en-US",
            "EN-gb",
            "sr-Latn-RS",
            "zh-Hant-TW",
            "x-private",
            "i-klingon",
            "en-US-u-ca-gregory",
        ];
        for range in ranges {
            let selector_holder = TargetLanguage::parse(range).expect("a range or tag");
            let selector = selector_holder.selector();
            let folded = RegexBuilder::new(&extended_filter_pattern(selector, CaseFolding::InTheOperator))
                .case_insensitive(true)
                .build()
                .expect("a valid pattern");
            let closed = RegexBuilder::new(&extended_filter_pattern(selector, CaseFolding::InThePattern))
                .build()
                .expect("a valid pattern");
            for tag in tags {
                let parsed = LangTag::new(tag).expect("a valid tag");
                let admitted = selector.admits(parsed, LanguageMatching::ExtendedFiltering);
                assert_eq!(folded.is_match(tag), admitted, "{range} against {tag}");
                assert_eq!(closed.is_match(tag), admitted, "{range} against {tag}, case folded in the pattern");
            }
        }
    }

    /// Clause 4.9 tag equality: the exact pattern admits the tag in either
    /// case and no further subtag, whatever side carries the fold.
    #[test]
    fn the_exact_pattern_admits_the_tag_alone_in_either_case() {
        let holder = TargetLanguage::parse("en-GB").expect("a tag");
        let folded = RegexBuilder::new(&exact_tag_pattern(holder.selector().as_str(), CaseFolding::InTheOperator))
            .case_insensitive(true)
            .build()
            .expect("a valid pattern");
        let closed = RegexBuilder::new(&exact_tag_pattern("en-GB", CaseFolding::InThePattern))
            .build()
            .expect("a valid pattern");
        for tag in ["en-GB", "en-gb", "EN-GB", "en", "en-GB-x", "en-US"] {
            let expected = LangTag::new(tag).is_ok_and(|parsed| holder.selector().admits(parsed, LanguageMatching::TagEquality));
            assert_eq!(folded.is_match(tag), expected, "{tag}");
            assert_eq!(closed.is_match(tag), expected, "{tag}, case folded in the pattern");
        }
    }
}
