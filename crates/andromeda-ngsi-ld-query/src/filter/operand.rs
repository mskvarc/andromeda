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

    Ok(CompareValue::Scalar(first))
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
        // the pattern as written. Resolving RFC 8259 escapes here would refuse
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

    /// The filter keeps the operand the client wrote, a URI included, so a
    /// stored or forwarded filter reads back as written (clause 5.2.12); the
    /// reading holds the spellings it is compared in.
    #[test]
    fn a_lone_uri_operand_parses_as_written() {
        assert_eq!(
            equality_operand("rel==\"HTTP://Example.com/a\""),
            CompareValue::Scalar(Scalar::Str("HTTP://Example.com/a".to_owned()))
        );
        assert_eq!(
            equality_operand("brandName==\"Mercedes\""),
            CompareValue::Scalar(Scalar::Str("Mercedes".to_owned()))
        );
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
