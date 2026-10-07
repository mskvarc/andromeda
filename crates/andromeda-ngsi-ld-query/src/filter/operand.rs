//! The right-hand side of a clause 4.9 Query Term.
//!
//! ```text
//! ComparableValue   = Number / quotedStr / dateTime / date / time
//! ValueList         = Value 1*(%x2C Value)
//! Range             = ComparableValue dots ComparableValue
//! CompEqualityValue = OtherValue / ValueList / Range / URI
//! ```
//!
//! What separates these productions from the attribute axis is that they are
//! read entirely from literals: the operand's own lexical form decides which
//! domain it belongs to, because the grammar gives it no type tag.

use crate::{
    error::QueryError,
    filter::{
        expectation::{QExpectation, QFound},
        json_string::decode_quoted_str,
        lexer::Token,
        quoting::Quoting,
        scalar::{CompareValue, Scalar},
        token_cursor::TokenCursor,
    },
    pattern::Pattern,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_number::JsonNumber;

/// Parse the right-hand side of a comparison: a scalar, a range, or a list.
///
/// # Errors
///
/// Returns [`QueryError::QExpected`] when no operand stands where one is
/// required, and one of the literal-typing variants when an operand satisfies
/// no clause 4.9 production.
pub fn parse_compare_value(cursor: &mut TokenCursor<'_>) -> Result<CompareValue, QueryError> {
    if cursor.peek_ahead(1) == Some(&Token::Dots) {
        let min = parse_comparable_value(cursor)?;
        cursor.advance();
        let max = parse_comparable_value(cursor)?;
        return Ok(CompareValue::Range { min, max });
    }

    let first = parse_scalar(cursor)?;

    if cursor.peek() == Some(&Token::Comma) {
        let mut values = vec![first];
        while cursor.peek() == Some(&Token::Comma) {
            cursor.advance();
            values.push(parse_scalar(cursor)?);
        }
        return Ok(CompareValue::ValueList(values));
    }

    Ok(uri_equivalent_spellings(first))
}

/// Read a lone equality operand as the spellings RFC 3986 section 6 makes
/// equivalent to it.
///
/// Clause 4.9 lists `URI` as an alternative of `CompEqualityValue`, and clause
/// 4.23-adjacent guidance requires that "URI comparison should be performed so
/// that the number of false negatives is minimized, as recommended by IETF
/// RFC 3986 section 6".
///
/// A Relationship object is parsed as an [`Identifier`] before it is stored, and that parse applies the RFC 3986 section 6.2.2 syntax-based normalisation and section 6.2.3 scheme-based normalisation for the schemes that define one -- case normalisation of scheme and host, removal of a default port, `.`/`..` resolution, an empty path becoming `/`. A query operand is the bytes the client typed, so an entity written with `HTTP://Example.com/a` is stored as `http://example.com/a` and would not be found by the very URI that created it.
///
/// Both spellings are therefore compared, which is what makes the two sides
/// symmetric: the written one still matches a Property whose string value
/// happens to be that text, and the normalised one matches the object the write
/// path actually stored. A `ValueList` expresses that with no new type — clause
/// 4.9 already gives it "is included in" semantics, and under `unequal` it
/// excludes both, which is correct because the two spellings name one resource.
///
/// An operand that is not a URI, or one already in its normalised spelling, is
/// left exactly as it was.
fn uri_equivalent_spellings(operand: Scalar) -> CompareValue {
    let (Scalar::Str(ref written) | Scalar::Uri(ref written)) = operand else {
        return CompareValue::Scalar(operand);
    };
    let Ok(parsed) = written.parse::<Identifier>() else {
        return CompareValue::Scalar(operand);
    };
    if parsed.as_str() == written {
        return CompareValue::Scalar(operand);
    }
    let normalised = match operand {
        Scalar::Uri(_) => Scalar::Uri(parsed.as_str().to_owned()),
        Scalar::Str(_) | Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => {
            Scalar::Str(parsed.as_str().to_owned())
        }
    };
    CompareValue::ValueList(vec![operand, normalised])
}

/// Parse a single scalar operand, classifying temporal literals by shape.
///
/// The two literal tokens are kept apart rather than collapsed: clause 4.9's
/// `quotedStr` production carries the quotes, so which productions can
/// derive an operand depends on whether the client wrote them.
///
/// # Errors
///
/// Returns [`QueryError::QExpected`] when no operand stands where one is
/// required, [`QueryError::QInvalidNumber`] for a malformed `Number`, and
/// [`QueryError::QInvalidDateTime`], [`QueryError::QInvalidDate`] or
/// [`QueryError::QInvalidTime`] for a bare literal written in a clause 4.6.3
/// form it does not satisfy.
fn parse_scalar(cursor: &mut TokenCursor<'_>) -> Result<Scalar, QueryError> {
    match cursor.advance() {
        Some(Token::NumberLit(literal)) => {
            let number = literal.parse::<JsonNumber>().map_err(QueryError::QInvalidNumber)?;
            Ok(Scalar::Number(number))
        }
        // Clause 4.9 `ComparableValue = Number / quotedStr / dateTime / date /
        // time`, and `quotedStr = String` is the IETF RFC 8259 section 7
        // production, so this is the one operand position whose escapes resolve.
        Some(Token::QuotedStr(body)) => Scalar::from_literal(&decode_quoted_str(body)?, Quoting::Quoted),
        Some(Token::Ident(literal)) => Scalar::from_literal(literal, Quoting::Bare),
        Some(Token::True) => Ok(Scalar::Bool(true)),
        Some(Token::False) => Ok(Scalar::Bool(false)),
        other => Err(QueryError::QExpected {
            expected: QExpectation::Value,
            found: QFound::of(other),
        }),
    }
}

/// Parse a clause 4.9 `ComparableValue`, the operand of an ordering operator
/// and either bound of a `Range`.
///
/// `ComparableValue = Number / quotedStr / dateTime / date / time` leaves out
/// `true`, `false` and a bare `URI`: those derive only from `OtherValue` and
/// `URI`, which the grammar admits after `==` and `!=` alone.
///
/// # Errors
///
/// Returns [`QueryError::QExpected`] naming [`QExpectation::ComparableValue`]
/// when the operand is a boolean or a bare word, and the [`parse_scalar`]
/// failures otherwise.
pub fn parse_comparable_value(cursor: &mut TokenCursor<'_>) -> Result<Scalar, QueryError> {
    let token = cursor.peek();
    let scalar = parse_scalar(cursor)?;
    let refused = || QueryError::QExpected {
        expected: QExpectation::ComparableValue,
        found: QFound::of(token),
    };
    match scalar {
        Scalar::Bool(_) | Scalar::Uri(_) => Err(refused()),
        // A bare literal in none of the clause 4.6.3 forms reads as text, and
        // only `quotedStr` derives text, quotes included.
        Scalar::Str(_) if matches!(token, Some(Token::Ident(_))) => Err(refused()),
        Scalar::Str(_) | Scalar::Number(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => Ok(scalar),
    }
}

/// Consume the operand of `~=` / `!~=`, which may be quoted or bare.
///
/// The operand is compiled here rather than at evaluation time, so a
/// malformed regular expression is a rejected request instead of a filter
/// that quietly matches nothing.
///
/// # Errors
///
/// Returns [`QueryError::QExpected`] when no operand stands where one is
/// required, and the [`Pattern::parse`] failures when the operand is not a
/// regular expression both of the broker's engines run.
pub fn parse_pattern_string(cursor: &mut TokenCursor<'_>) -> Result<Pattern, QueryError> {
    match cursor.advance() {
        // Clause 4.9 `QueryTerm =/ Attribute patternOp RegExp`: `RegExp` is its
        // own terminal, "a regular expression as mandated by IEEE 1003.2", not a
        // `quotedStr`. Its metacharacters include the backslash, so the body is
        // the pattern as written -- resolving RFC 8259 escapes here would refuse
        // `^example\.com$`.
        Some(Token::QuotedStr(literal) | Token::Ident(literal)) => Pattern::parse(literal),
        other => Err(QueryError::QExpected {
            expected: QExpectation::PatternString,
            found: QFound::of(other),
        }),
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        expression::Expr,
        parser::parse,
        predicate::Predicate,
        scalar::{CompareValue, Scalar},
    };
    use pretty_assertions::assert_eq;

    /// The equality operand of a filter holding one predicate.
    fn equality_operand(q: &str) -> CompareValue {
        let Expr::Pred(Predicate::Equality { rhs, .. }) = parse(q).expect("parse q") else {
            panic!("a single equality predicate");
        };
        rhs
    }

    /// The broker normalises a Relationship object through `Identifier` before storing
    /// it, so an operand compared only as written could not find the entity the
    /// same text created. RFC 3986 section 6 asks that such false negatives be
    /// minimised, so both spellings are compared.
    #[test]
    fn a_uri_operand_is_compared_in_the_spelling_the_write_path_stores_as_well() {
        let CompareValue::ValueList(values) = equality_operand("rel==\"HTTP://Example.com/a\"") else {
            panic!("a URI with a distinct normalised spelling compares as both");
        };
        assert_eq!(
            values,
            vec![Scalar::Str("HTTP://Example.com/a".to_owned()), Scalar::Str("http://example.com/a".to_owned()),]
        );
    }

    /// RFC 3986 section 6.2.3 scheme-based normalisation: a default port and an
    /// empty path are both removed by the parse the write path uses.
    #[test]
    fn every_normalisation_the_write_path_applies_is_matched_by_the_operand() {
        for (written, normalised) in [
            ("http://example.com:80/a", "http://example.com/a"),
            ("http://example.com/a/./b/../c", "http://example.com/a/c"),
            ("http://example.com", "http://example.com/"),
        ] {
            let CompareValue::ValueList(values) = equality_operand(&format!("rel==\"{written}\"")) else {
                panic!("{written} normalises to a different spelling");
            };
            assert_eq!(values[0], Scalar::Str(written.to_owned()), "{written}");
            assert_eq!(values[1], Scalar::Str(normalised.to_owned()), "{written}");
        }
    }

    /// An operand already in its normalised spelling gains no second reading,
    /// so the common case binds one value rather than two.
    #[test]
    fn a_uri_already_in_its_normalised_spelling_compares_as_one_value() {
        for unchanged in ["urn:ngsi-ld:OffStreetParking:Downtown1", "http://example.com/a", "tag:example.com,2024:foo"] {
            let operand = equality_operand(&format!("rel==\"{unchanged}\""));
            assert_eq!(
                operand,
                CompareValue::Scalar(Scalar::Str(unchanged.to_owned())),
                "{unchanged} is already normalised"
            );
        }
    }

    /// An operand that is not a URI at all is untouched: clause 4.9 compares a
    /// `quotedStr` as the text it is.
    #[test]
    fn a_string_operand_that_is_not_a_uri_is_left_as_written() {
        assert_eq!(
            equality_operand("brandName==\"Mercedes\""),
            CompareValue::Scalar(Scalar::Str("Mercedes".to_owned()))
        );
    }

    /// An explicit list keeps exactly the values the client wrote, so the
    /// equivalence reading applies to a lone operand alone.
    #[test]
    fn an_explicit_value_list_is_not_expanded_further() {
        let CompareValue::ValueList(values) = equality_operand("status==\"a\",\"b\"") else {
            panic!("a value list");
        };
        assert_eq!(values.len(), 2);
    }

    /// Clause 4.9 `ComparableValue = Number / quotedStr / dateTime / date /
    /// time`: a boolean and a bare word derive only from `OtherValue` and
    /// `URI`, which follow `==` and `!=` alone.
    #[test]
    fn a_boolean_or_a_bare_word_after_an_ordering_operator_or_in_a_range_is_refused() {
        for q in [
            "b>true",
            "b<=false",
            "b>=true",
            "b<false",
            "ref>urn:ngsi-ld:Target:a",
            "n==true..false",
            "n==a..b",
            "n==1..true",
        ] {
            assert!(parse(q).is_err(), "{q} is outside the grammar");
        }
    }

    /// Every `ComparableValue` alternative still stands after an ordering
    /// operator and in a range, and `OtherValue` still stands after `==`.
    #[test]
    fn every_comparable_value_after_an_ordering_operator_or_in_a_range_is_accepted() {
        for q in [
            "n>1",
            "n<\"text\"",
            "n>=2024-01-01T00:00:00Z",
            "n<=2024-01-01",
            "n>12:00:00Z",
            "n==1..2",
            "n==\"a\"..\"b\"",
            "b==true",
            "b!=false",
        ] {
            assert!(parse(q).is_ok(), "{q} is inside the grammar");
        }
    }
}
