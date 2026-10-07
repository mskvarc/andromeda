//! The collation keywords of the Unicode locale extension (RFC 6067).
//!
//! Clauses 5.7.2.4 and 5.7.4.4 make a preferred collation that "does not conform to a valid ICU collation (see IETF RFC 6067)" `BadRequestData`. RFC 6067 defines the `-u-` extension as attributes followed by keywords (a two-character key and the types after it), both registered in the Unicode locale extension registry, so a key or type outside that registry states no collation preference.
//!
//! Only keys whose registered types are a closed list are decided here. The reordering key `kr` and the deprecated `vt` take open-ended sequences that ICU refuses outright when unusable, so restating those sets would only risk a stale copy refusing a valid request.

use strum::{Display, IntoStaticStr};

/// The BCP 47 singleton introducing the Unicode locale extension (RFC 6067); clause 4.23.3's examples (`und-u-ks-identic`, `de-u-co-phonebk`) use it.
const UNICODE_EXTENSION_SINGLETON: &str = "u";

/// The length of a BCP47 singleton subtag, which introduces an extension.
const SINGLETON_LENGTH: usize = 1;

/// RFC 6067 fixes keys at two characters and keyword types and attributes at three or more, so length alone tells a key from a value.
const KEYWORD_KEY_LENGTH: usize = 2;

/// The types RFC 6067 registers for the boolean collation keys.
///
/// A key written with no type at all selects `true`. The registry's `yes` alias
/// for `true` is spelled here beside it; its `no` alias for `false` is not,
/// because RFC 6067 fixes a type at three to eight characters and `no` is two,
/// so a tag spelling it declares a second keyword key rather than a type.
const BOOLEAN_TYPES: &[&str] = &["true", "false", "yes"];

/// A collation key of the Unicode locale extension, spelled as RFC 6067
/// registers it.
#[derive(Clone, Copy, Debug, Display, Eq, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "lowercase")]
pub enum CollationKeyword {
    /// Collation type.
    Co,
    /// Alternate handling.
    Ka,
    /// Backward second level.
    Kb,
    /// Case level.
    Kc,
    /// Case first.
    Kf,
    /// Hiragana quaternary.
    Kh,
    /// Normalisation.
    Kk,
    /// Numeric ordering.
    Kn,
    /// Reordering.
    Kr,
    /// Strength.
    Ks,
    /// Maximum variable.
    Kv,
    /// Variable top.
    Vt,
}

/// One collation key of the Unicode locale extension and the types it admits.
struct CollationKey {
    /// The two-character key.
    key: CollationKeyword,
    /// The registered types, or `None` where the registry admits an open-ended sequence that ICU itself refuses when unusable.
    types: Option<&'static [&'static str]>,
}

/// The collation keys of the Unicode extension registry that RFC 6067 governs.
///
/// The type lists carry the registry's aliases beside its canonical names (`ks` accepts `primary` for `level1`) because a client may spell either and ICU canonicalises one to the other. Aliases longer than eight characters (`secondary`, `quaternary`, `identical`) are absent: BCP 47 caps a subtag at eight, so such a tag never parses.
const COLLATION_KEYS: [CollationKey; 12] = [
    CollationKey {
        key: CollationKeyword::Co,
        types: Some(&[
            "big5han", "compat", "dict", "direct", "ducet", "emoji", "eor", "gb2312", "phonebk", "phonetic", "pinyin", "reformed", "search", "searchjl",
            "standard", "stroke", "trad", "unihan", "zhuyin",
        ]),
    },
    CollationKey {
        key: CollationKeyword::Ka,
        types: Some(&["noignore", "shifted"]),
    },
    CollationKey {
        key: CollationKeyword::Kb,
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey {
        key: CollationKeyword::Kc,
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey {
        key: CollationKeyword::Kf,
        types: Some(&["upper", "lower", "false"]),
    },
    CollationKey {
        key: CollationKeyword::Kh,
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey {
        key: CollationKeyword::Kk,
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey {
        key: CollationKeyword::Kn,
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey {
        key: CollationKeyword::Kr,
        types: None,
    },
    CollationKey {
        key: CollationKeyword::Ks,
        types: Some(&["level1", "level2", "level3", "level4", "identic", "primary", "tertiary"]),
    },
    CollationKey {
        key: CollationKeyword::Kv,
        types: Some(&["space", "punct", "symbol", "currency"]),
    },
    CollationKey {
        key: CollationKeyword::Vt,
        types: None,
    },
];

/// The registry entry for `key`, if it names a collation setting. BCP 47 subtags are case-insensitive.
fn collation_key(key: &str) -> Option<&'static CollationKey> {
    COLLATION_KEYS.iter().find(|entry| key.eq_ignore_ascii_case(<&'static str>::from(entry.key)))
}

/// What the Unicode extension of a tag says that RFC 6067 does not register.
#[derive(Debug, PartialEq, Eq)]
pub enum UnregisteredKeyword<'a> {
    /// A keyword key that names no collation setting (a numbering system or calendar, say).
    Key(&'a str),
    /// A collation key carrying a type the registry does not list. ICU silently falls back to the key's default for it, so the request would be answered in an order it did not ask for.
    Type {
        /// The collation key the type was written under.
        key: CollationKeyword,
        /// The type the registry does not list.
        value: &'a str,
    },
}

/// The first thing `tag`'s Unicode extension declares that RFC 6067 does not register, if any.
///
/// The tag text is walked subtag by subtag because the language-tag crate's extension accessor indexes one byte past the end of a tag with no extension (`de-DE`, for one). A singleton closes the previous extension, so the keywords are the subtags between the `u` singleton and the next singleton: a two-character subtag opens a keyword and the longer subtags after it are its types, while longer subtags before any key are extension attributes, which carry no collation setting.
#[must_use]
pub fn unregistered_collation_keyword(tag: &str) -> Option<UnregisteredKeyword<'_>> {
    let mut in_unicode_extension = false;
    let mut current: Option<&'static CollationKey> = None;
    for subtag in tag.split('-') {
        if subtag.len() == SINGLETON_LENGTH {
            in_unicode_extension = subtag.eq_ignore_ascii_case(UNICODE_EXTENSION_SINGLETON);
            current = None;
        } else if in_unicode_extension && subtag.len() == KEYWORD_KEY_LENGTH {
            let Some(entry) = collation_key(subtag) else {
                return Some(UnregisteredKeyword::Key(subtag));
            };
            current = Some(entry);
        } else if in_unicode_extension
            && let Some(entry) = current
            && let Some(types) = entry.types
            && !types.iter().any(|registered| subtag.eq_ignore_ascii_case(registered))
        {
            return Some(UnregisteredKeyword::Type { key: entry.key, value: subtag });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::ordering::unicode_collation_keyword::{CollationKeyword, UnregisteredKeyword, unregistered_collation_keyword};

    /// Clause 4.23.3 EXAMPLE 6 and EXAMPLE 7 are the two collations the
    /// specification demonstrates the parameter with.
    #[test]
    fn the_collations_the_specification_gives_as_examples_declare_only_registered_keywords() {
        for example in ["und-u-ks-identic", "de-u-co-phonebk", "und-x-icu", "de-DE"] {
            pretty_assertions::assert_eq!(unregistered_collation_keyword(example), None, "{example}");
        }
    }

    /// A numbering system or calendar key states no collation preference.
    #[test]
    fn a_keyword_key_that_is_not_a_collation_setting_is_reported() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-nu-latn"), Some(UnregisteredKeyword::Key("nu")));
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-ca-buddhist"), Some(UnregisteredKeyword::Key("ca")));
    }

    /// ICU silently substitutes the key's default for an unregistered type, so the tag has to be refused here.
    #[test]
    fn a_collation_key_carrying_an_unregistered_type_is_reported() {
        pretty_assertions::assert_eq!(
            unregistered_collation_keyword("de-u-co-nonsense"),
            Some(UnregisteredKeyword::Type {
                key: CollationKeyword::Co,
                value: "nonsense"
            })
        );
        pretty_assertions::assert_eq!(
            unregistered_collation_keyword("de-u-ks-zzz"),
            Some(UnregisteredKeyword::Type {
                key: CollationKeyword::Ks,
                value: "zzz"
            })
        );
    }

    /// The registry's aliases are types too: `primary` names `level1` and `yes` names `true`.
    #[test]
    fn the_registry_aliases_are_admitted_beside_the_canonical_type_names() {
        for accepted in ["de-u-ks-primary", "de-u-ks-tertiary", "de-u-kn-yes", "de-u-kn-true", "de-u-kf-false"] {
            pretty_assertions::assert_eq!(unregistered_collation_keyword(accepted), None, "{accepted}");
        }
    }

    /// RFC 6067 fixes a type at three to eight characters, so the two-character `no` alias for `false` cannot be spelled as one and reads as a second key.
    #[test]
    fn a_two_character_alias_is_read_as_a_key_because_no_type_is_that_short() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-kf-no"), Some(UnregisteredKeyword::Key("no")));
    }

    /// A boolean key written with no type at all selects `true`, which the
    /// registry admits.
    #[test]
    fn a_boolean_key_written_without_a_type_is_admitted() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-kn"), None);
    }

    /// `kr` and `vt` are open-ended sequences, not closed lists.
    #[test]
    fn the_open_ended_keys_admit_their_sequences() {
        for accepted in ["de-u-kr-latn-digit", "de-u-kr-grek", "de-u-vt-0020"] {
            pretty_assertions::assert_eq!(unregistered_collation_keyword(accepted), None, "{accepted}");
        }
    }

    /// BCP 47 subtags are case-insensitive.
    #[test]
    fn keys_and_types_are_recognised_whatever_case_they_are_written_in() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-U-CO-PHONEBK"), None);
    }

    /// A subtag before the first key is an extension attribute, not a type.
    #[test]
    fn an_extension_attribute_before_the_first_key_is_not_read_as_a_type() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-attr-co-phonebk"), None);
    }

    /// A singleton closes the extension before it.
    #[test]
    fn a_later_extension_closes_the_unicode_one() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-co-phonebk-x-private"), None);
    }
}
