//! RFC 4647 section 3.3.2 extended filtering restated as a POSIX regular
//! expression over a `languageMap` entry's `@language`.
//!
//! The statement matches the pattern case-insensitively (`~*`), as RFC 4647
//! section 2 compares subtags. The first range subtag anchors the tag's first
//! subtag, `*` admitting any. Each later range subtag that is not `*` is found
//! further along the tag after skipping zero or more subtags, none of them a
//! singleton: the skipped run is `(-[a-z0-9]{2,8})*`, so a singleton such as
//! the `x` of a private-use sequence is never stepped over. Whatever the tag
//! holds after the last matched subtag is admitted. A validated range or tag
//! holds letters, digits and `-` only, so every subtag is written into the
//! pattern as itself, and the pattern is bound as a parameter.

use andromeda_ngsi_ld_query::filter::target_language::LanguageSelector;

/// A first subtag a `*` stands for.
const ANY_FIRST_SUBTAG: &str = "[a-z0-9]{1,8}";

/// The run of non-singleton subtags a later range subtag may be found after.
const SKIPPED_SUBTAGS: &str = "(-[a-z0-9]{2,8})*";

/// Whatever subtags follow the last one the range names.
const TRAILING_SUBTAGS: &str = "(-[a-z0-9]{1,8})*$";

/// The range wildcard (RFC 4647 section 2.2).
const WILDCARD: &str = "*";

/// The POSIX pattern a tag matches, case-insensitively, exactly when
/// `language`, read as an extended language range, admits it.
#[must_use]
pub fn extended_filter_pattern(language: LanguageSelector<'_>) -> String {
    let mut subtags = language.subtags();
    let mut pattern = String::from("^");
    match subtags.next() {
        Some(WILDCARD) | None => pattern.push_str(ANY_FIRST_SUBTAG),
        Some(first) => pattern.push_str(&first.to_ascii_lowercase()),
    }
    for subtag in subtags.filter(|subtag| *subtag != WILDCARD) {
        pattern.push_str(SKIPPED_SUBTAGS);
        pattern.push('-');
        pattern.push_str(&subtag.to_ascii_lowercase());
    }
    pattern.push_str(TRAILING_SUBTAGS);
    pattern
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::extended_filter_pattern::extended_filter_pattern;
    use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, target_language::TargetLanguage};
    use langtag::LangTag;
    use pretty_assertions::assert_eq;
    use regex::RegexBuilder;

    #[test]
    fn the_pattern_anchors_the_first_subtag_and_skips_non_singletons_before_each_later_one() {
        let range = TargetLanguage::parse("de-*-DE").expect("a range");
        assert_eq!(extended_filter_pattern(range.selector()), "^de(-[a-z0-9]{2,8})*-de(-[a-z0-9]{1,8})*$");
        let leading = TargetLanguage::parse("*-GB").expect("a range");
        assert_eq!(
            extended_filter_pattern(leading.selector()),
            "^[a-z0-9]{1,8}(-[a-z0-9]{2,8})*-gb(-[a-z0-9]{1,8})*$"
        );
        let tag = TargetLanguage::parse("en").expect("a tag");
        assert_eq!(extended_filter_pattern(tag.selector()), "^en(-[a-z0-9]{1,8})*$");
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
            let pattern = RegexBuilder::new(&extended_filter_pattern(selector))
                .case_insensitive(true)
                .build()
                .expect("a valid pattern");
            for tag in tags {
                let parsed = LangTag::new(tag).expect("a valid tag");
                assert_eq!(
                    pattern.is_match(tag),
                    selector.admits(parsed, LanguageMatching::ExtendedFiltering),
                    "{range} against {tag}"
                );
            }
        }
    }
}
