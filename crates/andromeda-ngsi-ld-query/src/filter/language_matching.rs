//! How a target language selects the keys of a languageMap.
//!
//! Clause 4.9 selects "the matching key-value pair ... where the key matches
//! the target language", and clause 4.5.18.2 makes the keys RFC 5646 tags,
//! whose comparison RFC 5646 section 2.1.1 makes case-insensitive. That is
//! [`LanguageMatching::TagEquality`], the only reading the specification
//! gives. The `language-ranges` level of the `q_target_languages` extension
//! replaces it with RFC 4647 section 3.3.2 extended filtering, under which
//! `[en]` also selects `en-GB`. The mode is a parameter of every evaluation
//! and compilation rather than a mark on the parsed filter, because a filter
//! round-trips through its clause 4.9 text, which has no place to carry one.

/// The comparison a target language selects languageMap keys by.
///
/// The default is the specification's own reading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum LanguageMatching {
    /// Case-insensitive tag equality (clause 4.9, RFC 5646 section 2.1.1).
    #[default]
    TagEquality,
    /// RFC 4647 section 3.3.2 extended filtering, the `language-ranges` level
    /// of the `q_target_languages` extension.
    ExtendedFiltering,
}
