//! The shared shape of a validated, `CompactString`-backed NGSI-LD name.

/// Declare a `CompactString`-backed newtype whose only constructor validates its
/// value against the NGSI-LD compacted-name grammar.
///
/// The three named-member types (an entity type name, an attribute name, and a
/// reference) are the same shape: a `CompactString` behind a newtype, a
/// grammar check in [`validate_ngsi_ld_name`](crate::reference::validate_ngsi_ld_name)
/// that no derive can express, and the `FromStr`/`TryFrom<String>` pair that
/// routes every construction through it. This macro is that shape once.
///
/// It expands to the struct (with the caller's `derive` list and `AsRef`
/// targets, plus the shared `serde`, `Display` and `Into<String>` attributes),
/// an `as_str` accessor, and the validating `FromStr` and `TryFrom<String>`
/// impls. Append `borrow_str` to also emit `Borrow<str>`, which lets a set or
/// map keyed by the name be probed with a `&str` without allocating.
///
/// The expansion refers to `CompactString`, the `derive_more`/`serde` derives,
/// `FromStr`, `Borrow`, `validate_ngsi_ld_name` and `NgsiLdReferenceError` by
/// their short names, so each invoking module imports them exactly as a
/// hand-written newtype would.
macro_rules! validated_ngsi_name {
    (
        $(#[$attr:meta])*
        $name:ident, derive($($derive:ident),+ $(,)?), as_ref($($as_ref:tt),+ $(,)?), borrow_str
    ) => {
        validated_ngsi_name!(@define $(#[$attr])* $name, derive($($derive),+), as_ref($($as_ref),+));

        // No derive produces `Borrow` for a target other than the field's own
        // type. Sound because the derived `Hash` forwards to `CompactString`,
        // which hashes exactly as its `str` does.
        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.0.as_str()
            }
        }
    };
    (
        $(#[$attr:meta])*
        $name:ident, derive($($derive:ident),+ $(,)?), as_ref($($as_ref:tt),+ $(,)?)
    ) => {
        validated_ngsi_name!(@define $(#[$attr])* $name, derive($($derive),+), as_ref($($as_ref),+));
    };
    (@define
        $(#[$attr:meta])*
        $name:ident, derive($($derive:ident),+), as_ref($($as_ref:tt),+)
    ) => {
        $(#[$attr])*
        #[derive($($derive),+)]
        #[serde(try_from = "String", into = "String")]
        #[as_ref($($as_ref),+)]
        #[display("{_0}")]
        #[into(String)]
        pub struct $name(CompactString);

        impl $name {
            /// Borrow the name as a string slice.
            pub fn as_str(&self) -> &str {
                self.0.as_str()
            }
        }

        // Validating parser: the grammar check in `validate_ngsi_ld_name` is what
        // makes the newtype worth having, and no derive expresses a fallible
        // parse.
        impl FromStr for $name {
            type Err = NgsiLdReferenceError;

            fn from_str(value: &str) -> Result<$name, NgsiLdReferenceError> {
                validate_ngsi_ld_name(value)?;
                Ok($name(CompactString::from(value)))
            }
        }

        // Parsing lives in `FromStr` so borrowed call sites do not allocate; this
        // adapter exists only because `#[serde(try_from = "String")]` requires
        // it.
        impl TryFrom<String> for $name {
            type Error = NgsiLdReferenceError;

            fn try_from(value: String) -> Result<$name, NgsiLdReferenceError> {
                value.parse()
            }
        }
    };
}

/// Emit the three length-bound tests every validated name type shares.
///
/// Expands, inside the invoking test module, to one test that a name of exactly
/// [`IDENTIFIER_BYTE_LIMIT`](crate::identifier_length::IDENTIFIER_BYTE_LIMIT)
/// octets parses, one that an expanded IRI one octet longer is refused although
/// the IRI grammar alone admits it, and one that a short-name term one octet
/// longer is refused. The expansion names `IDENTIFIER_BYTE_LIMIT`, `IriBuf`
/// and `NgsiLdReferenceError` by their short names, so the invoking module
/// imports them.
#[cfg(test)]
macro_rules! name_length_tests {
    ($name:ty) => {
        #[test]
        fn a_name_of_exactly_the_limit_is_accepted() {
            let prefix = "https://example.com/";
            let raw = format!("{prefix}{}", "a".repeat(IDENTIFIER_BYTE_LIMIT - prefix.len()));
            let parsed: $name = raw.parse().expect("a name at the limit");
            pretty_assertions::assert_eq!(parsed.as_str(), raw);
        }

        #[test]
        fn an_iri_name_past_the_limit_is_too_long_even_though_the_iri_grammar_admits_it() {
            let prefix = "https://example.com/";
            let raw = format!("{prefix}{}", "a".repeat(IDENTIFIER_BYTE_LIMIT + 1 - prefix.len()));
            assert!(raw.parse::<IriBuf>().is_ok(), "the IRI grammar has no length bound");
            let refused = raw.parse::<$name>().expect_err("an over-long IRI name");
            assert!(matches!(refused, NgsiLdReferenceError::TooLong(_)), "{refused:?}");
            assert!(refused.to_string().contains("1025 bytes"), "{refused}");
        }

        #[test]
        fn a_short_name_term_past_the_limit_is_too_long() {
            let raw = "a".repeat(IDENTIFIER_BYTE_LIMIT + 1);
            let refused = raw.parse::<$name>().expect_err("an over-long term");
            assert!(matches!(refused, NgsiLdReferenceError::TooLong(_)), "{refused:?}");
        }
    };
}
