//! The collation keywords of the Unicode locale extension (IETF RFC 6067).
//!
//! Clause 4.23.1 lets an `orderBy` name a preferred collation, and clauses
//! 5.7.2.4 and 5.7.4.4 both say that one which "does not conform to a valid ICU
//! collation (see IETF RFC 6067)" is `BadRequestData`. RFC 6067 defines the
//! `-u-` extension as a sequence of attributes followed by keywords, each
//! keyword a two-character key and the types that follow it, and registers both
//! in the Unicode CLDR registry: "the keys and types are those registered in
//! the Unicode locale extension registry". A key or a type outside that
//! registry therefore states no collation preference, which is what this module
//! decides.
//!
//! Only the keys whose registered types are a closed list are decided here. The
//! reordering key `kr` and the deprecated `vt` take open-ended sequences — a
//! run of script and category codes, a run of code points — and ICU refuses an
//! unusable one outright rather than ignoring it, so nothing is gained by
//! restating those two sets and a stale copy would refuse a valid request.

/// The BCP47 singleton that introduces the Unicode locale extension, `u`.
///
/// IETF RFC 6067 defines this extension, and clause 4.23.3 spells its examples
/// (`und-u-ks-identic`, `de-u-co-phonebk`) with it.
const UNICODE_EXTENSION_SINGLETON: &str = "u";

/// The length of a BCP47 singleton subtag, which introduces an extension.
const SINGLETON_LENGTH: usize = 1;

/// The subtag length that distinguishes a Unicode keyword *key* from a value.
///
/// RFC 6067 fixes keys at two characters and keyword types and attributes at
/// three or more, so the length alone decides which of the two a subtag is.
const KEYWORD_KEY_LENGTH: usize = 2;

/// The types RFC 6067 registers for the boolean collation keys.
///
/// A key written with no type at all selects `true`. The registry's `yes` alias
/// for `true` is spelled here beside it; its `no` alias for `false` is not,
/// because RFC 6067 fixes a type at three to eight characters and `no` is two,
/// so a tag spelling it declares a second keyword key rather than a type.
const BOOLEAN_TYPES: &[&str] = &["true", "false", "yes"];

/// One collation key of the Unicode locale extension and the types it admits.
struct CollationKey {
    /// The two-character key, as RFC 6067 registers it.
    key: &'static str,
    /// The types registered for the key, or `None` where the registry admits an
    /// open-ended sequence that ICU itself refuses when it cannot be used.
    types: Option<&'static [&'static str]>,
}

/// The collation keys of the Unicode BCP47 extension registry RFC 6067 governs:
/// collation type, alternate handling, backward second level, case level, case
/// first, hiragana quaternary, normalisation, numeric ordering, reordering,
/// strength, maximum variable and variable top.
///
/// The type lists carry the registry's aliases beside its canonical names —
/// `ks` accepts `primary` for `level1` and `tertiary` for `level3` — because a
/// client may spell either and ICU canonicalises one to the other. Aliases the
/// registry spells with more than eight characters (`secondary`, `quaternary`,
/// `identical`) are absent: BCP47 caps a subtag at eight characters, so such a
/// tag never parses in the first place.
const COLLATION_KEYS: [CollationKey; 12] = [
    CollationKey {
        key: "co",
        types: Some(&[
            "big5han", "compat", "dict", "direct", "ducet", "emoji", "eor", "gb2312", "phonebk", "phonetic", "pinyin", "reformed", "search", "searchjl",
            "standard", "stroke", "trad", "unihan", "zhuyin",
        ]),
    },
    CollationKey {
        key: "ka",
        types: Some(&["noignore", "shifted"]),
    },
    CollationKey {
        key: "kb",
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey {
        key: "kc",
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey {
        key: "kf",
        types: Some(&["upper", "lower", "false"]),
    },
    CollationKey {
        key: "kh",
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey {
        key: "kk",
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey {
        key: "kn",
        types: Some(BOOLEAN_TYPES),
    },
    CollationKey { key: "kr", types: None },
    CollationKey {
        key: "ks",
        types: Some(&["level1", "level2", "level3", "level4", "identic", "primary", "tertiary"]),
    },
    CollationKey {
        key: "kv",
        types: Some(&["space", "punct", "symbol", "currency"]),
    },
    CollationKey { key: "vt", types: None },
];

/// The registry entry for `key`, if it names a collation setting.
///
/// BCP47 subtags are case-insensitive, so a client that spells its keyword key
/// in upper case names the same setting.
fn collation_key(key: &str) -> Option<&'static CollationKey> {
    COLLATION_KEYS.iter().find(|entry| key.eq_ignore_ascii_case(entry.key))
}

/// What the Unicode extension of a tag says that RFC 6067 does not register.
#[derive(Debug, PartialEq, Eq)]
pub enum UnregisteredKeyword<'a> {
    /// A keyword key that names no collation setting at all — a numbering
    /// system or a calendar, say, which expresses no collation preference.
    Key(&'a str),
    /// A collation key carrying a type the registry does not list for it. ICU
    /// silently falls back to the key's default for such a type, so a request
    /// naming one would otherwise be answered with an order it did not ask for.
    Type {
        /// The collation key the type was written under.
        key: &'a str,
        /// The type the registry does not list.
        value: &'a str,
    },
}

/// The first thing `tag`'s Unicode extension declares that RFC 6067 does not
/// register, if it declares one.
///
/// The already-validated tag text is walked subtag by subtag rather than
/// through the language-tag crate's extension accessor, which indexes one byte
/// past the end of any tag that carries no extension at all — `de-DE` among
/// them — and so cannot be asked whether an extension is present.
///
/// A singleton opens an extension and closes the previous one, so the keywords
/// are exactly the subtags between the `u` singleton and the next singleton. A
/// two-character subtag there opens a keyword and the longer subtags that
/// follow it are its types; longer subtags seen before any key are the
/// extension's attributes, which carry no collation setting of their own.
#[must_use]
pub fn unregistered_collation_keyword(tag: &str) -> Option<UnregisteredKeyword<'_>> {
    let mut in_unicode_extension = false;
    let mut current: Option<(&str, &'static CollationKey)> = None;
    for subtag in tag.split('-') {
        if subtag.len() == SINGLETON_LENGTH {
            in_unicode_extension = subtag.eq_ignore_ascii_case(UNICODE_EXTENSION_SINGLETON);
            current = None;
        } else if in_unicode_extension && subtag.len() == KEYWORD_KEY_LENGTH {
            let Some(entry) = collation_key(subtag) else {
                return Some(UnregisteredKeyword::Key(subtag));
            };
            current = Some((subtag, entry));
        } else if in_unicode_extension
            && let Some((key, entry)) = current
            && let Some(types) = entry.types
            && !types.iter().any(|registered| subtag.eq_ignore_ascii_case(registered))
        {
            return Some(UnregisteredKeyword::Type { key, value: subtag });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::ordering::unicode_collation_keyword::{UnregisteredKeyword, unregistered_collation_keyword};

    /// Clause 4.23.3 EXAMPLE 6 and EXAMPLE 7 are the two collations the
    /// specification demonstrates the parameter with.
    #[test]
    fn the_collations_the_specification_gives_as_examples_declare_only_registered_keywords() {
        for example in ["und-u-ks-identic", "de-u-co-phonebk", "und-x-icu", "de-DE"] {
            pretty_assertions::assert_eq!(unregistered_collation_keyword(example), None, "{example}");
        }
    }

    /// A keyword key selecting a numbering system or a calendar states no
    /// collation preference, so the tag names no collation.
    #[test]
    fn a_keyword_key_that_is_not_a_collation_setting_is_reported() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-nu-latn"), Some(UnregisteredKeyword::Key("nu")));
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-ca-buddhist"), Some(UnregisteredKeyword::Key("ca")));
    }

    /// RFC 6067 registers the types as well as the keys, and ICU silently
    /// substitutes the key's default for an unregistered one, so the tag has to
    /// be refused here or the request is answered with the wrong order.
    #[test]
    fn a_collation_key_carrying_an_unregistered_type_is_reported() {
        pretty_assertions::assert_eq!(
            unregistered_collation_keyword("de-u-co-nonsense"),
            Some(UnregisteredKeyword::Type { key: "co", value: "nonsense" })
        );
        pretty_assertions::assert_eq!(
            unregistered_collation_keyword("de-u-ks-zzz"),
            Some(UnregisteredKeyword::Type { key: "ks", value: "zzz" })
        );
    }

    /// The registry's own aliases are types too: `primary` names `level1` and
    /// `yes` names `true`, and ICU canonicalises both.
    #[test]
    fn the_registry_aliases_are_admitted_beside_the_canonical_type_names() {
        for accepted in ["de-u-ks-primary", "de-u-ks-tertiary", "de-u-kn-yes", "de-u-kn-true", "de-u-kf-false"] {
            pretty_assertions::assert_eq!(unregistered_collation_keyword(accepted), None, "{accepted}");
        }
    }

    /// RFC 6067 fixes a keyword type at three to eight characters, so the
    /// registry's two-character `no` alias for `false` cannot be spelled as one:
    /// a tag that writes it declares a second keyword key, and that key names no
    /// collation setting.
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

    /// `kr` reorders by a run of script and category codes and `vt` names a run
    /// of code points; neither is a closed list, and ICU refuses an unusable
    /// one rather than ignoring it.
    #[test]
    fn the_open_ended_keys_admit_their_sequences() {
        for accepted in ["de-u-kr-latn-digit", "de-u-kr-grek", "de-u-vt-0020"] {
            pretty_assertions::assert_eq!(unregistered_collation_keyword(accepted), None, "{accepted}");
        }
    }

    /// BCP47 subtags are case-insensitive, so neither the key nor the type may
    /// depend on the casing a client happened to send.
    #[test]
    fn keys_and_types_are_recognised_whatever_case_they_are_written_in() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-U-CO-PHONEBK"), None);
    }

    /// A subtag before the first key is an extension attribute, not a type, and
    /// carries no collation setting of its own.
    #[test]
    fn an_extension_attribute_before_the_first_key_is_not_read_as_a_type() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-attr-co-phonebk"), None);
    }

    /// A singleton closes the extension before it, so a subtag under a
    /// different extension is not read as a collation type.
    #[test]
    fn a_later_extension_closes_the_unicode_one() {
        pretty_assertions::assert_eq!(unregistered_collation_keyword("de-u-co-phonebk-x-private"), None);
    }
}
