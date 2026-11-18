//! Recursive-descent parser for the NGSI-LD Query Language (clause 4.9).
//!
//! The grammar this parser implements is reproduced verbatim as ABNF at the root of this crate. Nothing derives from it: the language needs operator precedence and value typing that a pure ABNF recogniser does not express, so it is the reference this parser is checked against, and the two are kept in step by hand.
//!
//! This module owns the expression grammar: the boolean structure of a `Query`,
//! and the `QueryTerm` that combines an attribute axis with an operator and an
//! operand. The productions it composes each live with the concept they belong
//! to, because `q` is not the only parameter that derives from them:
//! [`value_path`] holds the `ValuePath` axis clause 4.23.3 shares,
//! [`attribute_path`] the `LinkedEntityRelation` hop, [`operand`] the
//! right-hand-side literals, and [`token_cursor`] the position all of them
//! advance.

mod attribute_path;
pub(in crate::filter) mod lexer;
mod operand;
mod quoted_string;
mod token_cursor;
mod value_path;

use crate::filter::{
    Filter,
    attribute_path::AttributePath,
    complexity::WRITTEN_NESTING_CEILING,
    filter_error::FilterError,
    parser::{
        attribute_path::parse_linked_entity_relation,
        lexer::{Token, tokenize},
        operand::{parse_comparable_value, parse_compare_value, parse_pattern_string},
        token_cursor::TokenCursor,
    },
    query_term::{OrderingOperator, QueryTerm, SetMembership},
};
use std::str::FromStr;

impl FromStr for Filter {
    type Err = FilterError;

    /// Parses an NGSI-LD `q` filter expression string into a tree (clause 4.9).
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::Empty`] when the input is blank, the tokeniser's
    /// [`FilterError::UnterminatedString`] or [`FilterError::UnexpectedCharacter`] when
    /// it cannot be lexed, and [`FilterError::Expected`],
    /// [`FilterError::UnexpectedToken`], [`FilterError::PathTooDeep`] or
    /// one of the literal-typing variants when it is outside the clause 4.9
    /// grammar, and [`FilterError::NestedTooDeep`] when its parentheses nest past
    /// [`WRITTEN_NESTING_CEILING`].
    fn from_str(input: &str) -> Result<Filter, FilterError> {
        if input.trim().is_empty() {
            return Err(FilterError::Empty);
        }
        let tokens = tokenize(input)?;
        let mut cursor = TokenCursor::new(&tokens);
        let filter = parse_or(&mut cursor, 0)?;
        if !cursor.is_exhausted() {
            return Err(FilterError::UnexpectedToken {
                position: cursor.position(),
                token: cursor.found(),
            });
        }
        Ok(filter)
    }
}

/// Parse a disjunction: `and_expr ('|' and_expr)*`, inside `depth`
/// parentheses.
fn parse_or(cursor: &mut TokenCursor<'_>, depth: usize) -> Result<Filter, FilterError> {
    let mut left = parse_and(cursor, depth)?;
    let mut terms = Vec::new();

    while cursor.peek() == Some(&Token::Pipe) {
        cursor.advance();
        terms.push(left);
        left = parse_and(cursor, depth)?;
    }

    if terms.is_empty() {
        Ok(left)
    } else {
        terms.push(left);
        Ok(Filter::Or(terms))
    }
}

/// Parse a conjunction: `primary (';' primary)*`, inside `depth` parentheses.
fn parse_and(cursor: &mut TokenCursor<'_>, depth: usize) -> Result<Filter, FilterError> {
    let mut left = parse_primary(cursor, depth)?;
    let mut terms = Vec::new();

    while cursor.peek() == Some(&Token::Semi) {
        cursor.advance();
        terms.push(left);
        left = parse_primary(cursor, depth)?;
    }

    if terms.is_empty() {
        Ok(left)
    } else {
        terms.push(left);
        Ok(Filter::And(terms))
    }
}

/// Parse a primary: either a parenthesised group or a single predicate, inside
/// `depth` parentheses.
///
/// Each group recurses once, so the group that would nest past
/// [`WRITTEN_NESTING_CEILING`] is refused before it is descended into.
fn parse_primary(cursor: &mut TokenCursor<'_>, depth: usize) -> Result<Filter, FilterError> {
    if cursor.peek() == Some(&Token::LParen) {
        let inner = depth + 1;
        if inner > WRITTEN_NESTING_CEILING.get() {
            return Err(FilterError::NestedTooDeep {
                depth: inner,
                limit: WRITTEN_NESTING_CEILING,
            });
        }
        cursor.advance();
        let expr = parse_or(cursor, inner)?;
        cursor.expect(&Token::RParen)?;
        Ok(expr)
    } else {
        Ok(Filter::Term(parse_predicate(cursor)?))
    }
}

/// Parse one Query Term: its attribute axis (clause 4.9 `Attribute`, a hop's
/// braces included), optionally followed by an operator and its operand.
fn parse_predicate(cursor: &mut TokenCursor<'_>) -> Result<QueryTerm, FilterError> {
    let leading = cursor.parse_ident()?;
    let path = parse_linked_entity_relation(cursor, leading, 0)?;
    parse_operator_and_operand(cursor, path)
}

/// Read the comparison operator and operand that follow an attribute axis,
/// or accept the axis on its own as an existence predicate.
fn parse_operator_and_operand(cursor: &mut TokenCursor<'_>, path: AttributePath) -> Result<QueryTerm, FilterError> {
    match cursor.peek() {
        Some(Token::Eq) => {
            cursor.advance();
            let rhs = parse_compare_value(cursor)?;
            Ok(QueryTerm::Equality {
                path,
                membership: SetMembership::Within,
                value: rhs,
            })
        }
        Some(Token::Neq) => {
            cursor.advance();
            let rhs = parse_compare_value(cursor)?;
            Ok(QueryTerm::Equality {
                path,
                membership: SetMembership::Outside,
                value: rhs,
            })
        }
        Some(Token::Gt) => {
            cursor.advance();
            let rhs = parse_comparable_value(cursor)?;
            Ok(QueryTerm::Ordering {
                path,
                operator: OrderingOperator::Greater,
                value: rhs,
            })
        }
        Some(Token::Ge) => {
            cursor.advance();
            let rhs = parse_comparable_value(cursor)?;
            Ok(QueryTerm::Ordering {
                path,
                operator: OrderingOperator::GreaterEq,
                value: rhs,
            })
        }
        Some(Token::Lt) => {
            cursor.advance();
            let rhs = parse_comparable_value(cursor)?;
            Ok(QueryTerm::Ordering {
                path,
                operator: OrderingOperator::Less,
                value: rhs,
            })
        }
        Some(Token::Le) => {
            cursor.advance();
            let rhs = parse_comparable_value(cursor)?;
            Ok(QueryTerm::Ordering {
                path,
                operator: OrderingOperator::LessEq,
                value: rhs,
            })
        }
        Some(Token::Match) => {
            cursor.advance();
            let pattern = parse_pattern_string(cursor)?;
            Ok(QueryTerm::MatchPattern { path, pattern })
        }
        Some(Token::NotMatch) => {
            cursor.advance();
            let pattern = parse_pattern_string(cursor)?;
            Ok(QueryTerm::NotMatchPattern { path, pattern })
        }
        // A path with no operator is a bare existence predicate; every other
        // token belongs to the enclosing expression and is left for it.
        Some(
            Token::Ident(_)
            | Token::QuotedStr(_)
            | Token::NumberLit(_)
            | Token::True
            | Token::False
            | Token::Dots
            | Token::Semi
            | Token::Pipe
            | Token::LParen
            | Token::RParen
            | Token::LBracket
            | Token::RBracket
            | Token::LBrace
            | Token::RBrace
            | Token::Star
            | Token::Dot
            | Token::Comma,
        )
        | None => Ok(QueryTerm::Exists(path)),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            Filter,
            attribute_path::AttributePath,
            complexity::{MAX_SUB_ATTRIBUTE_DEPTH, WRITTEN_NESTING_CEILING},
            equality_value::EqualityValue,
            expectation::{Expectation, Found},
            filter_error::FilterError,
            parser::lexer::TokenKind,
            query_term::{OrderingOperator, QueryTerm, SetMembership},
            scalar::Scalar,
            value_path::ValuePath,
        },
        pattern::pattern_error::PatternError,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::rendering::instant_text};
    use andromeda_ngsi_ld_number::JsonNumber;

    #[test]
    fn parse_existence() {
        let expr = "temperature".parse::<Filter>().unwrap();
        pretty_assertions::assert_eq!(
            expr,
            Filter::Term(QueryTerm::Exists(AttributePath::Local(ValuePath::simple(
                "temperature".parse().expect("valid attribute name")
            ))))
        );
    }

    #[test]
    fn parse_equality_number() {
        let expr = "temperature==20".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Equality { path, membership, value: rhs }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "temperature");
                pretty_assertions::assert_eq!(membership, SetMembership::Within);
                pretty_assertions::assert_eq!(rhs, EqualityValue::Scalar(Scalar::Number(20.into())));
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn an_attribute_path_within_the_depth_limit_parses() {
        let at_limit = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH));
        assert!(at_limit.parse::<Filter>().is_ok(), "a path at the limit must parse");
    }

    #[test]
    fn an_attribute_path_deeper_than_the_limit_is_rejected() {
        // The limit is the depth a write is admitted to, so a deeper path
        // addresses nothing storable while its backend rendering grows with
        // its depth.
        let too_deep = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH + 1));
        assert!(
            matches!(too_deep.parse::<Filter>(), Err(FilterError::PathTooDeep { .. })),
            "an over-deep path must be rejected"
        );
    }

    /// Clause 5.5.6: parentheses past the ceiling are `TooComplexQuery`, and
    /// the refusal comes before the parser recurses into them.
    #[test]
    fn parentheses_at_the_written_ceiling_parse_and_one_level_deeper_are_too_complex() {
        let grouped = |levels: usize| format!("{}a{}", "(".repeat(levels), ")".repeat(levels));
        assert!(
            grouped(WRITTEN_NESTING_CEILING.get()).parse::<Filter>().is_ok(),
            "parentheses at the ceiling must parse"
        );
        let error = grouped(WRITTEN_NESTING_CEILING.get() + 1).parse::<Filter>().expect_err("must reject");
        assert!(
            matches!(error, FilterError::NestedTooDeep { depth: 65, limit } if limit == WRITTEN_NESTING_CEILING),
            "got {error:?}"
        );
        let unbounded = format!("{}a", "(".repeat(1_000_000));
        assert!(matches!(unbounded.parse::<Filter>(), Err(FilterError::NestedTooDeep { depth: 65, .. })));
    }

    #[test]
    fn parse_inequality_string() {
        let expr = "brandName!=\"Mercedes\"".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Equality { path, membership, value: rhs }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "brandName");
                pretty_assertions::assert_eq!(membership, SetMembership::Outside);
                pretty_assertions::assert_eq!(rhs, EqualityValue::Scalar(Scalar::Str("Mercedes".to_string())));
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_greater_than() {
        let expr = "speed>50".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Ordering {
                path,
                operator: op,
                value: rhs,
            }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "speed");
                pretty_assertions::assert_eq!(op, OrderingOperator::Greater);
                pretty_assertions::assert_eq!(rhs, Scalar::Number(50.into()));
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_expression() {
        let expr = "speed>50;brandName!=\"Mercedes\"".parse::<Filter>().unwrap();
        match expr {
            Filter::And(terms) => pretty_assertions::assert_eq!(terms.len(), 2),
            other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parse_or_expression() {
        let expr = "speed>50|temperature<10".parse::<Filter>().unwrap();
        match expr {
            Filter::Or(terms) => pretty_assertions::assert_eq!(terms.len(), 2),
            other @ (Filter::And(_) | Filter::Term(_)) => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_takes_precedence_over_or() {
        let expr = "a==1|b==2;c==3".parse::<Filter>().unwrap();
        match expr {
            Filter::Or(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match &terms[1] {
                    Filter::And(and_terms) => pretty_assertions::assert_eq!(and_terms.len(), 2),
                    other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected And as second OR term, got {other:?}"),
                }
            }
            other @ (Filter::And(_) | Filter::Term(_)) => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_parenthesized_group() {
        let expr = "(speed>50|rpm>3000);brandName==\"Mercedes\"".parse::<Filter>().unwrap();
        match expr {
            Filter::And(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match &terms[0] {
                    Filter::Or(_) => {}
                    other @ (Filter::And(_) | Filter::Term(_)) => panic!("expected Or inside parens, got {other:?}"),
                }
            }
            other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parse_range() {
        let expr = "temperature==10..20".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Equality { value: rhs, .. }) => match rhs {
                EqualityValue::Range { min, max } => {
                    pretty_assertions::assert_eq!(min, Scalar::Number(10.into()));
                    pretty_assertions::assert_eq!(max, Scalar::Number(20.into()));
                }
                other @ (EqualityValue::Scalar(_) | EqualityValue::ValueList(_)) => panic!("expected Range, got {other:?}"),
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    /// Clause 4.9 admits a range only after `==` and `!=`; an ordering
    /// operator takes a single value.
    #[test]
    fn a_range_after_an_ordering_operator_does_not_parse() {
        assert!("temperature>10..20".parse::<Filter>().is_err());
    }

    #[test]
    fn parse_value_list() {
        let expr = "color==\"red\",\"blue\",\"green\"".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Equality { value: rhs, .. }) => match rhs {
                EqualityValue::ValueList(vals) => {
                    pretty_assertions::assert_eq!(vals.len(), 3);
                }
                other @ (EqualityValue::Scalar(_) | EqualityValue::Range { .. }) => panic!("expected ValueList, got {other:?}"),
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_match_pattern() {
        let expr = "brandName~=\"Merc.*\"".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::MatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "brandName");
                pretty_assertions::assert_eq!(pattern.as_str(), "Merc.*");
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected MatchPattern, got {other:?}"),
        }
    }

    #[test]
    fn parse_not_match_pattern() {
        let expr = "brandName!~=\"Merc.*\"".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::NotMatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "brandName");
                pretty_assertions::assert_eq!(pattern.as_str(), "Merc.*");
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected NotMatchPattern, got {other:?}"),
        }
    }

    #[test]
    fn a_date_time_scalar_is_parsed_once_at_filter_parse_time() {
        // Clause 4.6.3: a temporal operand is an RFC 3339 value, so a bare
        // literal that has the shape of one but is not a real instant is
        // derivable from no production of the clause 4.9 grammar (`quotedStr`
        // carries the quotes it lacks), and the request is malformed.
        match "observed>=2017-12-24T12:00:00Z".parse::<Filter>().expect("parses") {
            Filter::Term(QueryTerm::Ordering {
                value: Scalar::DateTime(_), ..
            }) => {}
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a date-time operand, got {other:?}"),
        }

        let error = "observed>=2017-13-45".parse::<Filter>().expect_err("must reject");
        assert!(matches!(error, FilterError::InvalidDate { .. }), "got {error:?}");

        let error = "observed>=2017-12-24T99:00:00Z".parse::<Filter>().expect_err("must reject");
        assert!(matches!(error, FilterError::InvalidDateTime { .. }), "got {error:?}");
    }

    /// Clause 4.6.3 lets a request write "a comma instead of a decimal point"
    /// before the fraction of the seconds, and clause 4.9 takes its `dateTime`
    /// and `time` operands "as mandated by clause 4.6.3": after an ordering
    /// operator, in a range and in a value list alike, the comma reads as the
    /// point and is written back as one.
    #[test]
    fn a_comma_before_the_fraction_reads_as_the_decimal_point() {
        for (comma, point) in [
            ("tm>2024-01-01T00:00:00,5Z", "tm>2024-01-01T00:00:00.5Z"),
            ("t<=12:00:00,25Z", "t<=12:00:00.25Z"),
            ("tm==2024-01-01T00:00:00,5Z", "tm==2024-01-01T00:00:00.5Z"),
            (
                "tm==2024-01-01T00:00:00,5Z,2024-01-02T00:00:00,123456Z",
                "tm==2024-01-01T00:00:00.5Z,2024-01-02T00:00:00.123456Z",
            ),
            ("t!=11:00:00,5Z..13:00:00,5Z", "t!=11:00:00.5Z..13:00:00.5Z"),
        ] {
            let read = comma.parse::<Filter>().unwrap_or_else(|error| panic!("{comma}: {error}"));
            let written_with_a_point = point.parse::<Filter>().expect("the decimal point form parses");
            pretty_assertions::assert_eq!(read.to_string(), written_with_a_point.to_string(), "{comma}");
            pretty_assertions::assert_eq!(read, written_with_a_point, "{comma}");
        }
    }

    /// Clause 4.9's `quotedStr` is the IETF RFC 8259 `String` production
    /// *including its quotes*, so the quoted spelling of the same literal is a
    /// well-formed Query Term whatever it contains, and the value compares as
    /// the text the write path stored for it.
    #[test]
    fn a_quoted_operand_that_satisfies_no_temporal_form_compares_as_text() {
        match "observed>=\"2017-12-24T99:00:00Z\"".parse::<Filter>().expect("a quotedStr is always derivable") {
            Filter::Term(QueryTerm::Ordering { value: Scalar::Str(text), .. }) => {
                pretty_assertions::assert_eq!(text, "2017-12-24T99:00:00Z");
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a string operand, got {other:?}"),
        }
    }

    /// Clause 4.9: `quotedStr` is a String whatever it contains, so a quoted
    /// literal in a `DateTime` form is a String, and only the bare one is a
    /// `dateTime`.
    #[test]
    fn a_quoted_operand_in_a_temporal_form_is_a_string_and_a_bare_one_that_value() {
        match "observed>=\"2017-12-24T12:00:00Z\"".parse::<Filter>().expect("parses") {
            Filter::Term(QueryTerm::Ordering { value: Scalar::Str(_), .. }) => {}
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a string operand, got {other:?}"),
        }
        match "observed>=2017-12-24T12:00:00Z".parse::<Filter>().expect("parses") {
            Filter::Term(QueryTerm::Ordering {
                value: Scalar::DateTime(_), ..
            }) => {}
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a date-time operand, got {other:?}"),
        }
    }

    #[test]
    fn an_invalid_regex_pattern_is_rejected_when_the_filter_is_parsed() {
        // Clause 4.9: the operand of `~=` is a regular expression. An
        // unterminated character class is not one, and the request is rejected
        // rather than turned into a filter that matches nothing.
        let error = "name~=\"[\"".parse::<Filter>().expect_err("must reject");
        assert!(matches!(error, FilterError::Pattern(PatternError::Regex(_))), "got {error:?}");

        let error = "name!~=\"[\"".parse::<Filter>().expect_err("must reject");
        assert!(matches!(error, FilterError::Pattern(PatternError::Regex(_))), "got {error:?}");
    }

    #[test]
    fn parse_dotted_attr_path() {
        let expr = "temperature.observedAt>=2017-12-24T12:00:00Z".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Ordering {
                path,
                operator: op,
                value: rhs,
            }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "temperature");
                pretty_assertions::assert_eq!(
                    path.terminal().sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
                    vec!["observedAt"]
                );
                pretty_assertions::assert_eq!(op, OrderingOperator::GreaterEq);
                match rhs {
                    Scalar::DateTime(dt) => {
                        pretty_assertions::assert_eq!(instant_text(dt), "2017-12-24T12:00:00Z");
                    }
                    other @ (Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::Date(_) | Scalar::Time(_)) => {
                        panic!("expected DateTime scalar, got {other:?}")
                    }
                }
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_compound_value_path() {
        let expr = "address[city]==\"Berlin\"".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Equality { path, .. }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "address");
                pretty_assertions::assert_eq!(
                    path.terminal()
                        .members()
                        .expect("a member path")
                        .addressed
                        .iter()
                        .map(AttributeName::as_str)
                        .collect::<Vec<_>>(),
                    vec!["city"]
                );
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_nested_compound_value_path() {
        let expr = "sensor.rawdata[airquality.particulate]==40".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Equality { path, .. }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "sensor");
                pretty_assertions::assert_eq!(
                    path.terminal().sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
                    vec!["rawdata"]
                );
                pretty_assertions::assert_eq!(
                    path.terminal()
                        .members()
                        .expect("a member path")
                        .addressed
                        .iter()
                        .map(AttributeName::as_str)
                        .collect::<Vec<_>>(),
                    vec!["airquality", "particulate"]
                );
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_less_equal() {
        let expr = "capacity<=10".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Ordering { operator: op, .. }) => {
                pretty_assertions::assert_eq!(op, OrderingOperator::LessEq);
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_complex_mixed() {
        let expr = "(temperature>=20;temperature<=25)|capacity<=10".parse::<Filter>().unwrap();
        match expr {
            Filter::Or(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match &terms[0] {
                    Filter::And(and_terms) => pretty_assertions::assert_eq!(and_terms.len(), 2),
                    other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected And, got {other:?}"),
                }
            }
            other @ (Filter::And(_) | Filter::Term(_)) => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_uri_equality() {
        let expr = "isParked==\"urn:ngsi-ld:OffStreetParking:Downtown1\"".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Equality { value: rhs, .. }) => match rhs {
                EqualityValue::Scalar(Scalar::Str(s)) => {
                    pretty_assertions::assert_eq!(s, "urn:ngsi-ld:OffStreetParking:Downtown1");
                }
                other @ (EqualityValue::Scalar(_) | EqualityValue::Range { .. } | EqualityValue::ValueList(_)) => panic!("expected Str, got {other:?}"),
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_negative_number() {
        let expr = "temperature>-5".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Ordering { value: rhs, .. }) => match rhs {
                Scalar::Number(n) => {
                    pretty_assertions::assert_eq!(n, JsonNumber::from(-5));
                }
                other @ (Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) => {
                    panic!("expected Number, got {other:?}")
                }
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_float_number() {
        let expr = "temperature==23.5".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Equality { value: rhs, .. }) => match rhs {
                EqualityValue::Scalar(Scalar::Number(n)) => {
                    pretty_assertions::assert_eq!(n, JsonNumber::try_from(23.5).expect("a finite number"));
                }
                other @ (EqualityValue::Scalar(_) | EqualityValue::Range { .. } | EqualityValue::ValueList(_)) => panic!("expected Number, got {other:?}"),
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn exact_fraction_and_range_failure_are_typed() {
        let expr = "temperature==0.100000000000000005".parse::<Filter>().expect("supported exact number");
        let Filter::Term(QueryTerm::Equality {
            value: EqualityValue::Scalar(Scalar::Number(number)),
            ..
        }) = expr
        else {
            panic!("expected numeric equality");
        };
        pretty_assertions::assert_eq!(number.to_string(), "0.100000000000000005");
        assert!(matches!("temperature==1e-16384".parse::<Filter>(), Err(FilterError::InvalidNumber { .. })));
    }

    #[test]
    fn parse_boolean_value() {
        let expr = "active==true".parse::<Filter>().unwrap();
        match expr {
            Filter::Term(QueryTerm::Equality { value: rhs, .. }) => {
                pretty_assertions::assert_eq!(rhs, EqualityValue::Scalar(Scalar::Bool(true)));
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn error_on_unterminated_string() {
        let result = "name==\"hello".parse::<Filter>();
        assert!(result.is_err());
    }

    #[test]
    fn error_on_unexpected_token() {
        let result = "==".parse::<Filter>();
        assert!(result.is_err());
    }

    /// The hop this axis descends: its Relationship, its type guard, and the
    /// inner axis, so a test states the shape it expects without matching a
    /// nested enum by hand.
    fn hop_of(path: &AttributePath) -> (&str, Vec<&str>, &AttributePath) {
        match path {
            AttributePath::Linked {
                relationship,
                guard_types,
                inner,
            } => (
                relationship.as_str(),
                guard_types.iter().map(AttributeName::as_str).collect::<Vec<&str>>(),
                inner,
            ),
            AttributePath::Local(local) => panic!("expected a linked-entity hop, got the local path {local:?}"),
        }
    }

    fn local_of(path: &AttributePath) -> &ValuePath {
        match path {
            AttributePath::Local(local) => local,
            AttributePath::Linked { relationship, .. } => panic!("expected a local path, got a hop through {relationship}"),
        }
    }

    fn predicate_of(expr: &Filter) -> &QueryTerm {
        match expr {
            Filter::Term(pred) => pred,
            other @ (Filter::And(_) | Filter::Or(_)) => panic!("expected a single predicate, got {other:?}"),
        }
    }

    #[test]
    fn a_comparison_after_a_hop_applies_to_the_linked_entity() {
        // Clause 4.9 `QueryTerm =/ Attribute equal CompEqualityValue` with
        // `Attribute = LinkedEntityRelation`: the braces close over the path and
        // the comparison follows them (EXAMPLE 13, `sensor{humidity}==40`).
        let expr = "isParkedIn{category}==\"commercial\"".parse::<Filter>().expect("parses");
        match predicate_of(&expr) {
            QueryTerm::Equality { path, membership, value: rhs } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
                assert!(guard.is_empty(), "no type guard was written: {guard:?}");
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "category");
                pretty_assertions::assert_eq!(*membership, SetMembership::Within);
                pretty_assertions::assert_eq!(*rhs, EqualityValue::Scalar(Scalar::Str("commercial".to_string())));
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_carrying_two_comma_separated_types_parses_them_as_its_guard() {
        // The clause 4.9 guard production is `*1(EntityType 1*("," EntityType) ":")`,
        // so two or more types followed by a colon constrain the linked entity.
        let expr = "isParkedIn{Building,Store:category}==\"x\"".parse::<Filter>().expect("parses");
        match predicate_of(&expr) {
            QueryTerm::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
                pretty_assertions::assert_eq!(guard, vec!["Building", "Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "category");
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn hops_nest_and_the_innermost_one_carries_the_comparison() {
        let expr = "a{b{c}}==5".parse::<Filter>().expect("parses");
        match predicate_of(&expr) {
            QueryTerm::Equality { path, value: rhs, .. } => {
                let (outer, outer_guard, inner_path) = hop_of(path);
                pretty_assertions::assert_eq!(outer, "a");
                assert!(outer_guard.is_empty(), "{outer_guard:?}");
                let (inner, inner_guard, terminal) = hop_of(inner_path);
                pretty_assertions::assert_eq!(inner, "b");
                assert!(inner_guard.is_empty(), "{inner_guard:?}");
                pretty_assertions::assert_eq!(local_of(terminal).attribute.as_str(), "c");
                pretty_assertions::assert_eq!(*rhs, EqualityValue::Scalar(Scalar::Number(5.into())));
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_with_no_operator_inside_is_an_existence_check_on_the_linked_entity() {
        let expr = "a{b}".parse::<Filter>().expect("parses");
        match predicate_of(&expr) {
            QueryTerm::Exists(path) => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "a");
                assert!(guard.is_empty(), "{guard:?}");
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "b");
            }
            other @ (QueryTerm::Equality { .. } | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an existence predicate, got {other:?}")
            }
        }
    }

    /// Clause 4.9 EXAMPLE 14 writes a guard of one type,
    /// `sensor{Device:humidity}`, and no `AttrName` or `EntityType` of its
    /// grammar holds a colon, so a name before a colon at the head of a hop is
    /// the one type of its guard, written bare or quoted.
    #[test]
    fn a_single_name_before_a_colon_inside_a_hop_is_a_type_guard() {
        for (q, relationship, guard, attribute) in [
            ("sensor{Device:humidity}==40", "sensor", "Device", "humidity"),
            ("sensor{Device: humidity}==40", "sensor", "Device", "humidity"),
            ("a{prefix:name}==\"v\"", "a", "prefix", "name"),
            (
                "\"https://example.com/sensor\"{\"https://example.com/Device\":\"https://example.com/humidity\"}==40",
                "https://example.com/sensor",
                "https://example.com/Device",
                "https://example.com/humidity",
            ),
            ("sensor{\"ex:Device\":humidity}==40", "sensor", "ex:Device", "humidity"),
        ] {
            let expr = q.parse::<Filter>().unwrap_or_else(|error| panic!("{q}: {error}"));
            let (read_relationship, read_guard, inner) = hop_of(predicate_of(&expr).path());
            pretty_assertions::assert_eq!(
                (read_relationship, read_guard, local_of(inner).attribute.as_str()),
                (relationship, vec![guard], attribute),
                "{q}"
            );
        }
    }

    /// The guard of one type opens each hop of a chain, outermost first.
    #[test]
    fn each_hop_of_a_chain_takes_a_guard_of_one_type() {
        let expr = "r{T1:s{T2:a}}==1".parse::<Filter>().expect("parses");
        let (relationship, guard, inner) = hop_of(predicate_of(&expr).path());
        pretty_assertions::assert_eq!((relationship, guard), ("r", vec!["T1"]));
        let (relationship, guard, inner) = hop_of(inner);
        pretty_assertions::assert_eq!((relationship, guard), ("s", vec!["T2"]));
        pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "a");
    }

    /// A prefixed attribute name at the head of a hop is written quoted, since
    /// its colon would otherwise end a type guard.
    #[test]
    fn a_quoted_prefixed_name_inside_a_hop_is_an_attribute_name() {
        let expr = "a{\"prefix:name\"}==\"v\"".parse::<Filter>().expect("parses");
        let (relationship, guard, inner) = hop_of(predicate_of(&expr).path());
        pretty_assertions::assert_eq!(relationship, "a");
        assert!(guard.is_empty(), "a quoted name is no guard: {guard:?}");
        pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "prefix:name");
    }

    /// A bare name at the head of a hop holding two colons reads equally well
    /// as a prefixed type before an attribute or a type before a prefixed
    /// attribute, so it is refused and the client quotes what it meant.
    #[test]
    fn a_bare_name_with_two_colons_at_the_head_of_a_hop_is_ambiguous() {
        let error = "rel{ex:Store:category}==\"x\"".parse::<Filter>().expect_err("must reject");
        assert!(matches!(error, FilterError::AmbiguousTypeGuard), "got {error:?}");
    }

    #[test]
    fn a_traversal_predicate_combines_with_a_local_one_through_the_conjunction() {
        let expr = "speed>50;isParkedIn{category}==\"x\"".parse::<Filter>().expect("parses");
        match expr {
            Filter::And(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match predicate_of(&terms[0]) {
                    QueryTerm::Ordering { path, .. } => pretty_assertions::assert_eq!(local_of(path).attribute.as_str(), "speed"),
                    other @ (QueryTerm::Exists(_) | QueryTerm::Equality { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                        panic!("expected an ordering predicate, got {other:?}")
                    }
                }
                let (relationship, _, _) = hop_of(predicate_of(&terms[1]).path());
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
            }
            other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected a conjunction, got {other:?}"),
        }
    }

    #[test]
    fn a_guard_type_that_is_not_the_last_one_keeps_the_colons_it_carries() {
        // Only the final type can carry the guard's terminating colon, because a
        // comma says another type follows. A prefixed type written before the
        // last one is therefore unambiguous and must not be split.
        let expr = "rel{ex:A,Store:category}==\"x\"".parse::<Filter>().expect("parses");
        match predicate_of(&expr) {
            QueryTerm::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "rel");
                pretty_assertions::assert_eq!(guard, vec!["ex:A", "Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "category");
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_final_guard_type_carrying_two_colons_is_rejected_rather_than_split_by_guesswork() {
        // `ex:Store:category` could be the prefixed type `ex:Store` followed by
        // `category`, or the type `ex` followed by the prefixed attribute
        // `Store:category`. Nothing in the clause 4.9 grammar decides between
        // them, so the request is rejected and the client quotes what it meant.
        let error = "rel{Building,ex:Store:category}==\"x\"".parse::<Filter>().expect_err("must reject");
        assert!(matches!(error, FilterError::AmbiguousTypeGuard), "got {error:?}");
    }

    #[test]
    fn quoting_a_guard_type_resolves_the_ambiguity_its_colons_would_create() {
        let expr = "rel{Building,\"ex:Store\":category}==\"x\"".parse::<Filter>().expect("parses");
        match predicate_of(&expr) {
            QueryTerm::Equality { path, .. } => {
                let (_, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(guard, vec!["Building", "ex:Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "category");
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_left_unclosed_is_rejected() {
        let error = "a{b".parse::<Filter>().expect_err("must reject");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Token(TokenKind::RBrace),
                    found: Found::EndOfInput
                }
            ),
            "got {error:?}"
        );
    }

    /// Clause 4.9 puts no Operator inside a hop's braces: `LinkedEntityPath`
    /// ends in a `ValuePath`, so the closing brace is expected where the
    /// comparison starts.
    #[test]
    fn a_comparison_written_inside_the_braces_is_rejected() {
        for written in ["isParkedIn{category==\"commercial\"}", "a{b{c>5}}", "a{b~=\"x\"}"] {
            let error = written.parse::<Filter>().expect_err("must reject");
            assert!(
                matches!(
                    error,
                    FilterError::Expected {
                        expected: Expectation::Token(TokenKind::RBrace),
                        found: Found::Token(_)
                    }
                ),
                "{written}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_hop_ending_in_a_compound_member_compares_that_member() {
        let expr = "a{b[c.d]}>=1".parse::<Filter>().expect("parses");
        match predicate_of(&expr) {
            QueryTerm::Ordering { path, operator: op, .. } => {
                let (_, _, inner) = hop_of(path);
                pretty_assertions::assert_eq!(
                    local_of(inner)
                        .members()
                        .expect("a member path")
                        .addressed
                        .iter()
                        .map(AttributeName::as_str)
                        .collect::<Vec<_>>(),
                    vec!["c", "d"]
                );
                pretty_assertions::assert_eq!(*op, OrderingOperator::GreaterEq);
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Equality { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an ordering predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_with_nothing_inside_it_is_rejected() {
        // Clause 4.9 requires a `LinkedEntityPath` between the braces.
        let error = "a{}".parse::<Filter>().expect_err("must reject");
        assert!(matches!(error, FilterError::Expected { .. }), "got {error:?}");
    }

    #[test]
    fn hop_nesting_is_bounded_by_the_same_budget_as_sub_attribute_nesting() {
        // A hop compiles to a correlated subquery per level, so an unbounded
        // chain is a denial-of-service vector rather than a legitimate query.
        let chain = |hops: usize| format!("{}z{}", "a{".repeat(hops), "}".repeat(hops));
        assert!(chain(MAX_SUB_ATTRIBUTE_DEPTH).parse::<Filter>().is_ok(), "a chain at the limit must parse");
        assert!(
            matches!(chain(MAX_SUB_ATTRIBUTE_DEPTH + 1).parse::<Filter>(), Err(FilterError::PathTooDeep { .. })),
            "an over-deep hop chain must be rejected"
        );
    }
}
