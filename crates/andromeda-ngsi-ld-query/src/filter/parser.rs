//! Recursive-descent parser for the NGSI-LD Query Language (clause 4.9).
//!
//! The grammar this parser implements is reproduced verbatim as ABNF at the root of this crate. Nothing derives from it -- the language needs operator precedence and value typing that a pure ABNF recogniser does not express -- so it is the reference this parser is checked against, and the two are kept in step by hand.
//!
//! This module owns the expression grammar: the boolean structure of a `Query`,
//! and the `QueryTerm` that combines an attribute axis with an operator and an
//! operand. The productions it composes each live with the concept they belong
//! to, because `q` is not the only parameter that derives from them —
//! [`value_path`] holds the `ValuePath` axis clause 4.23.3 shares,
//! [`linked_entity`] the `LinkedEntityRelation` hop, [`operand`] the
//! right-hand-side literals, and [`token_cursor`] the position all of them
//! advance.

use crate::{
    error::QueryError,
    filter::{
        expression::Expr,
        lexer::{Token, tokenize},
        linked_entity::parse_linked_entity_relation,
        operand::{parse_comparable_value, parse_compare_value, parse_pattern_string},
        path::QueryPath,
        predicate::{OrderingOp, Predicate, SetMembership},
        token_cursor::TokenCursor,
    },
};

/// Parses an NGSI-LD `q` filter expression string into an AST (clause 4.9).
///
/// # Errors
///
/// Returns [`QueryError::QEmpty`] when the input is blank, the tokeniser's
/// [`QueryError::QUnterminatedString`] or [`QueryError::QUnexpectedChar`] when
/// it cannot be lexed, and [`QueryError::QExpected`],
/// [`QueryError::QUnexpectedToken`], [`QueryError::QAttributePathTooDeep`] or
/// one of the literal-typing variants when it is outside the clause 4.9
/// grammar.
pub fn parse(input: &str) -> Result<Expr, QueryError> {
    if input.trim().is_empty() {
        return Err(QueryError::QEmpty);
    }
    let tokens = tokenize(input)?;
    let mut cursor = TokenCursor::new(&tokens);
    let expr = parse_or(&mut cursor)?;
    if !cursor.is_exhausted() {
        return Err(QueryError::QUnexpectedToken {
            position: cursor.position(),
            token: cursor.found(),
        });
    }
    Ok(expr)
}

/// Parse a disjunction: `and_expr ('|' and_expr)*`.
fn parse_or(cursor: &mut TokenCursor<'_>) -> Result<Expr, QueryError> {
    let mut left = parse_and(cursor)?;
    let mut terms = Vec::new();

    while cursor.peek() == Some(&Token::Pipe) {
        cursor.advance();
        terms.push(left);
        left = parse_and(cursor)?;
    }

    if terms.is_empty() {
        Ok(left)
    } else {
        terms.push(left);
        Ok(Expr::Or(terms))
    }
}

/// Parse a conjunction: `primary (';' primary)*`.
fn parse_and(cursor: &mut TokenCursor<'_>) -> Result<Expr, QueryError> {
    let mut left = parse_primary(cursor)?;
    let mut terms = Vec::new();

    while cursor.peek() == Some(&Token::Semi) {
        cursor.advance();
        terms.push(left);
        left = parse_primary(cursor)?;
    }

    if terms.is_empty() {
        Ok(left)
    } else {
        terms.push(left);
        Ok(Expr::And(terms))
    }
}

/// Parse a primary: either a parenthesised group or a single predicate.
fn parse_primary(cursor: &mut TokenCursor<'_>) -> Result<Expr, QueryError> {
    if cursor.peek() == Some(&Token::LParen) {
        cursor.advance();
        let expr = parse_or(cursor)?;
        cursor.expect(&Token::RParen)?;
        Ok(expr)
    } else {
        Ok(Expr::Pred(parse_predicate(cursor)?))
    }
}

/// Parse one Query Term: its attribute axis (clause 4.9 `Attribute`, a hop's
/// braces included), optionally followed by an operator and its operand.
fn parse_predicate(cursor: &mut TokenCursor<'_>) -> Result<Predicate, QueryError> {
    let leading = cursor.parse_ident()?;
    let path = parse_linked_entity_relation(cursor, leading, 0)?;
    parse_operator_and_operand(cursor, path)
}

/// Read the comparison operator and operand that follow an attribute axis,
/// or accept the axis on its own as an existence predicate.
fn parse_operator_and_operand(cursor: &mut TokenCursor<'_>, path: QueryPath) -> Result<Predicate, QueryError> {
    match cursor.peek() {
        Some(Token::Eq) => {
            cursor.advance();
            let rhs = parse_compare_value(cursor)?;
            Ok(Predicate::Equality {
                path,
                membership: SetMembership::Within,
                rhs,
            })
        }
        Some(Token::Neq) => {
            cursor.advance();
            let rhs = parse_compare_value(cursor)?;
            Ok(Predicate::Equality {
                path,
                membership: SetMembership::Outside,
                rhs,
            })
        }
        Some(Token::Gt) => {
            cursor.advance();
            let rhs = parse_comparable_value(cursor)?;
            Ok(Predicate::Ordering {
                path,
                op: OrderingOp::Greater,
                rhs,
            })
        }
        Some(Token::Ge) => {
            cursor.advance();
            let rhs = parse_comparable_value(cursor)?;
            Ok(Predicate::Ordering {
                path,
                op: OrderingOp::GreaterEq,
                rhs,
            })
        }
        Some(Token::Lt) => {
            cursor.advance();
            let rhs = parse_comparable_value(cursor)?;
            Ok(Predicate::Ordering {
                path,
                op: OrderingOp::Less,
                rhs,
            })
        }
        Some(Token::Le) => {
            cursor.advance();
            let rhs = parse_comparable_value(cursor)?;
            Ok(Predicate::Ordering {
                path,
                op: OrderingOp::LessEq,
                rhs,
            })
        }
        Some(Token::Match) => {
            cursor.advance();
            let pattern = parse_pattern_string(cursor)?;
            Ok(Predicate::MatchPattern { path, pattern })
        }
        Some(Token::NotMatch) => {
            cursor.advance();
            let pattern = parse_pattern_string(cursor)?;
            Ok(Predicate::NotMatchPattern { path, pattern })
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
        | None => Ok(Predicate::Exists(path)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{
        expectation::{QExpectation, QFound},
        lexer::TokenKind,
        path::AttrPath,
        scalar::{CompareValue, Scalar},
        value_path::MAX_SUB_ATTRIBUTE_DEPTH,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::rendering::instant_text};
    use andromeda_ngsi_ld_number::JsonNumber;

    #[test]
    fn parse_existence() {
        let expr = parse("temperature").unwrap();
        pretty_assertions::assert_eq!(
            expr,
            Expr::Pred(Predicate::Exists(QueryPath::Local(AttrPath::simple(
                "temperature".parse().expect("valid attribute name")
            ))))
        );
    }

    #[test]
    fn parse_equality_number() {
        let expr = parse("temperature==20").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { path, membership, rhs }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "temperature");
                pretty_assertions::assert_eq!(membership, SetMembership::Within);
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Number(20.into())));
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn an_attribute_path_within_the_depth_limit_parses() {
        let at_limit = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH));
        assert!(parse(&at_limit).is_ok(), "a path at the limit must parse");
    }

    #[test]
    fn an_attribute_path_deeper_than_the_limit_is_rejected() {
        // A defence against a pathological path whose backend rendering grows
        // with its depth; the limit is far above any legitimate nesting.
        let too_deep = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH + 1));
        assert!(
            matches!(parse(&too_deep), Err(QueryError::QAttributePathTooDeep { .. })),
            "an over-deep path must be rejected"
        );
    }

    #[test]
    fn parse_inequality_string() {
        let expr = parse("brandName!=\"Mercedes\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { path, membership, rhs }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "brandName");
                pretty_assertions::assert_eq!(membership, SetMembership::Outside);
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Str("Mercedes".to_string())));
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_greater_than() {
        let expr = parse("speed>50").unwrap();
        match expr {
            Expr::Pred(Predicate::Ordering { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "speed");
                pretty_assertions::assert_eq!(op, OrderingOp::Greater);
                pretty_assertions::assert_eq!(rhs, Scalar::Number(50.into()));
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_expression() {
        let expr = parse("speed>50;brandName!=\"Mercedes\"").unwrap();
        match expr {
            Expr::And(terms) => pretty_assertions::assert_eq!(terms.len(), 2),
            other @ (Expr::Or(_) | Expr::Pred(_)) => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parse_or_expression() {
        let expr = parse("speed>50|temperature<10").unwrap();
        match expr {
            Expr::Or(terms) => pretty_assertions::assert_eq!(terms.len(), 2),
            other @ (Expr::And(_) | Expr::Pred(_)) => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_takes_precedence_over_or() {
        let expr = parse("a==1|b==2;c==3").unwrap();
        match expr {
            Expr::Or(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match &terms[1] {
                    Expr::And(and_terms) => pretty_assertions::assert_eq!(and_terms.len(), 2),
                    other @ (Expr::Or(_) | Expr::Pred(_)) => panic!("expected And as second OR term, got {other:?}"),
                }
            }
            other @ (Expr::And(_) | Expr::Pred(_)) => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_parenthesized_group() {
        let expr = parse("(speed>50|rpm>3000);brandName==\"Mercedes\"").unwrap();
        match expr {
            Expr::And(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match &terms[0] {
                    Expr::Or(_) => {}
                    other @ (Expr::And(_) | Expr::Pred(_)) => panic!("expected Or inside parens, got {other:?}"),
                }
            }
            other @ (Expr::Or(_) | Expr::Pred(_)) => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parse_range() {
        let expr = parse("temperature==10..20").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { rhs, .. }) => match rhs {
                CompareValue::Range { min, max } => {
                    pretty_assertions::assert_eq!(min, Scalar::Number(10.into()));
                    pretty_assertions::assert_eq!(max, Scalar::Number(20.into()));
                }
                other @ (CompareValue::Scalar(_) | CompareValue::ValueList(_)) => panic!("expected Range, got {other:?}"),
            },
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_value_list() {
        let expr = parse("color==\"red\",\"blue\",\"green\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { rhs, .. }) => match rhs {
                CompareValue::ValueList(vals) => {
                    pretty_assertions::assert_eq!(vals.len(), 3);
                }
                other @ (CompareValue::Scalar(_) | CompareValue::Range { .. }) => panic!("expected ValueList, got {other:?}"),
            },
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_match_pattern() {
        let expr = parse("brandName~=\"Merc.*\"").unwrap();
        match expr {
            Expr::Pred(Predicate::MatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "brandName");
                pretty_assertions::assert_eq!(pattern.as_str(), "Merc.*");
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected MatchPattern, got {other:?}"),
        }
    }

    #[test]
    fn parse_not_match_pattern() {
        let expr = parse("brandName!~=\"Merc.*\"").unwrap();
        match expr {
            Expr::Pred(Predicate::NotMatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "brandName");
                pretty_assertions::assert_eq!(pattern.as_str(), "Merc.*");
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected NotMatchPattern, got {other:?}"),
        }
    }

    #[test]
    fn a_date_time_scalar_is_parsed_once_at_filter_parse_time() {
        // Clause 4.6.3: a temporal operand is an RFC 3339 value, so a bare
        // literal that has the shape of one but is not a real instant is
        // derivable from no production of the clause 4.9 grammar -- `quotedStr`
        // carries the quotes it lacks -- and the request is malformed.
        match parse("observed>=2017-12-24T12:00:00Z").expect("parses") {
            Expr::Pred(Predicate::Ordering { rhs: Scalar::DateTime(_), .. }) => {}
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected a date-time operand, got {other:?}"),
        }

        let error = parse("observed>=2017-13-45").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidDate { .. }), "got {error:?}");

        let error = parse("observed>=2017-12-24T99:00:00Z").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidDateTime { .. }), "got {error:?}");
    }

    /// Clause 4.9's `quotedStr` is the IETF RFC 8259 `String` production
    /// *including its quotes*, so the quoted spelling of the same literal is a
    /// well-formed Query Term whatever it contains, and the value compares as
    /// the text the write path stored for it.
    #[test]
    fn a_quoted_operand_that_satisfies_no_temporal_form_compares_as_text() {
        match parse("observed>=\"2017-12-24T99:00:00Z\"").expect("a quotedStr is always derivable") {
            Expr::Pred(Predicate::Ordering { rhs: Scalar::Str(text), .. }) => {
                pretty_assertions::assert_eq!(text, "2017-12-24T99:00:00Z");
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected a string operand, got {other:?}"),
        }
    }

    /// Clause 4.9: `quotedStr` is a String whatever it contains, so a quoted
    /// literal in a `DateTime` form is a String, and only the bare one is a
    /// `dateTime`.
    #[test]
    fn a_quoted_operand_in_a_temporal_form_is_a_string_and_a_bare_one_that_value() {
        match parse("observed>=\"2017-12-24T12:00:00Z\"").expect("parses") {
            Expr::Pred(Predicate::Ordering { rhs: Scalar::Str(_), .. }) => {}
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected a string operand, got {other:?}"),
        }
        match parse("observed>=2017-12-24T12:00:00Z").expect("parses") {
            Expr::Pred(Predicate::Ordering { rhs: Scalar::DateTime(_), .. }) => {}
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected a date-time operand, got {other:?}"),
        }
    }

    #[test]
    fn an_invalid_regex_pattern_is_rejected_when_the_filter_is_parsed() {
        // Clause 4.9: the operand of `~=` is a regular expression. An
        // unterminated character class is not one, and the request is rejected
        // rather than turned into a filter that matches nothing.
        let error = parse("name~=\"[\"").expect_err("must reject");
        assert!(matches!(error, QueryError::PatternRegex(_)), "got {error:?}");

        let error = parse("name!~=\"[\"").expect_err("must reject");
        assert!(matches!(error, QueryError::PatternRegex(_)), "got {error:?}");
    }

    #[test]
    fn parse_dotted_attr_path() {
        let expr = parse("temperature.observedAt>=2017-12-24T12:00:00Z").unwrap();
        match expr {
            Expr::Pred(Predicate::Ordering { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "temperature");
                pretty_assertions::assert_eq!(
                    path.terminal().sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
                    vec!["observedAt"]
                );
                pretty_assertions::assert_eq!(op, OrderingOp::GreaterEq);
                match rhs {
                    Scalar::DateTime(dt) => {
                        pretty_assertions::assert_eq!(instant_text(dt), "2017-12-24T12:00:00Z");
                    }
                    other @ (Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::Date(_) | Scalar::Time(_)) => {
                        panic!("expected DateTime scalar, got {other:?}")
                    }
                }
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_compound_value_path() {
        let expr = parse("address[city]==\"Berlin\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { path, .. }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "address");
                pretty_assertions::assert_eq!(path.terminal().value_path.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["city"]);
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_nested_compound_value_path() {
        let expr = parse("sensor.rawdata[airquality.particulate]==40").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { path, .. }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "sensor");
                pretty_assertions::assert_eq!(path.terminal().sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["rawdata"]);
                pretty_assertions::assert_eq!(
                    path.terminal().value_path.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
                    vec!["airquality", "particulate"]
                );
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_less_equal() {
        let expr = parse("capacity<=10").unwrap();
        match expr {
            Expr::Pred(Predicate::Ordering { op, .. }) => {
                pretty_assertions::assert_eq!(op, OrderingOp::LessEq);
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_complex_mixed() {
        let expr = parse("(temperature>=20;temperature<=25)|capacity<=10").unwrap();
        match expr {
            Expr::Or(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match &terms[0] {
                    Expr::And(and_terms) => pretty_assertions::assert_eq!(and_terms.len(), 2),
                    other @ (Expr::Or(_) | Expr::Pred(_)) => panic!("expected And, got {other:?}"),
                }
            }
            other @ (Expr::And(_) | Expr::Pred(_)) => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_uri_equality() {
        let expr = parse("isParked==\"urn:ngsi-ld:OffStreetParking:Downtown1\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Str(s)) => {
                    pretty_assertions::assert_eq!(s, "urn:ngsi-ld:OffStreetParking:Downtown1");
                }
                other @ (CompareValue::Scalar(_) | CompareValue::Range { .. } | CompareValue::ValueList(_)) => panic!("expected Str, got {other:?}"),
            },
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_negative_number() {
        let expr = parse("temperature>-5").unwrap();
        match expr {
            Expr::Pred(Predicate::Ordering { rhs, .. }) => match rhs {
                Scalar::Number(n) => {
                    pretty_assertions::assert_eq!(n, JsonNumber::from(-5));
                }
                other @ (Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) => {
                    panic!("expected Number, got {other:?}")
                }
            },
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_float_number() {
        let expr = parse("temperature==23.5").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Number(n)) => {
                    pretty_assertions::assert_eq!(n, JsonNumber::try_from(23.5).expect("a finite number"));
                }
                other @ (CompareValue::Scalar(_) | CompareValue::Range { .. } | CompareValue::ValueList(_)) => panic!("expected Number, got {other:?}"),
            },
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn exact_fraction_and_range_failure_are_typed() {
        let expr = parse("temperature==0.100000000000000005").expect("supported exact number");
        let Expr::Pred(Predicate::Equality {
            rhs: CompareValue::Scalar(Scalar::Number(number)),
            ..
        }) = expr
        else {
            panic!("expected numeric equality");
        };
        pretty_assertions::assert_eq!(number.to_string(), "0.100000000000000005");
        assert!(matches!(parse("temperature==1e-16384"), Err(QueryError::QInvalidNumber { .. })));
    }

    #[test]
    fn parse_boolean_value() {
        let expr = parse("active==true").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { rhs, .. }) => {
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Bool(true)));
            }
            other @ (Expr::And(_) | Expr::Or(_) | Expr::Pred(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn error_on_unterminated_string() {
        let result = parse("name==\"hello");
        assert!(result.is_err());
    }

    #[test]
    fn error_on_unexpected_token() {
        let result = parse("==");
        assert!(result.is_err());
    }

    /// The hop this axis descends: its Relationship, its type guard, and the
    /// inner axis, so a test states the shape it expects without matching a
    /// nested enum by hand.
    fn hop_of(path: &QueryPath) -> (&str, Vec<&str>, &QueryPath) {
        match path {
            QueryPath::Linked {
                relationship,
                guard_types,
                inner,
            } => (
                relationship.as_str(),
                guard_types.iter().map(AttributeName::as_str).collect::<Vec<&str>>(),
                inner,
            ),
            QueryPath::Local(local) => panic!("expected a linked-entity hop, got the local path {local:?}"),
        }
    }

    fn local_of(path: &QueryPath) -> &AttrPath {
        match path {
            QueryPath::Local(local) => local,
            QueryPath::Linked { relationship, .. } => panic!("expected a local path, got a hop through {relationship}"),
        }
    }

    fn predicate_of(expr: &Expr) -> &Predicate {
        match expr {
            Expr::Pred(pred) => pred,
            other @ (Expr::And(_) | Expr::Or(_)) => panic!("expected a single predicate, got {other:?}"),
        }
    }

    #[test]
    fn a_comparison_after_a_hop_applies_to_the_linked_entity() {
        // Clause 4.9 `QueryTerm =/ Attribute equal CompEqualityValue` with
        // `Attribute = LinkedEntityRelation`: the braces close over the path and
        // the comparison follows them (EXAMPLE 13, `sensor{humidity}==40`).
        let expr = parse("isParkedIn{category}==\"commercial\"").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, membership, rhs } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
                assert!(guard.is_empty(), "no type guard was written: {guard:?}");
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "category");
                pretty_assertions::assert_eq!(*membership, SetMembership::Within);
                pretty_assertions::assert_eq!(*rhs, CompareValue::Scalar(Scalar::Str("commercial".to_string())));
            }
            other @ (Predicate::Exists(_) | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_carrying_two_comma_separated_types_parses_them_as_its_guard() {
        // The clause 4.9 guard production is `*1(EntityType 1*("," EntityType) ":")`,
        // so two or more types followed by a colon constrain the linked entity.
        let expr = parse("isParkedIn{Building,Store:category}==\"x\"").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
                pretty_assertions::assert_eq!(guard, vec!["Building", "Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "category");
            }
            other @ (Predicate::Exists(_) | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn hops_nest_and_the_innermost_one_carries_the_comparison() {
        let expr = parse("a{b{c}}==5").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, rhs, .. } => {
                let (outer, outer_guard, inner_path) = hop_of(path);
                pretty_assertions::assert_eq!(outer, "a");
                assert!(outer_guard.is_empty(), "{outer_guard:?}");
                let (inner, inner_guard, terminal) = hop_of(inner_path);
                pretty_assertions::assert_eq!(inner, "b");
                assert!(inner_guard.is_empty(), "{inner_guard:?}");
                pretty_assertions::assert_eq!(local_of(terminal).attr.as_str(), "c");
                pretty_assertions::assert_eq!(*rhs, CompareValue::Scalar(Scalar::Number(5.into())));
            }
            other @ (Predicate::Exists(_) | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_with_no_operator_inside_is_an_existence_check_on_the_linked_entity() {
        let expr = parse("a{b}").expect("parses");
        match predicate_of(&expr) {
            Predicate::Exists(path) => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "a");
                assert!(guard.is_empty(), "{guard:?}");
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "b");
            }
            other @ (Predicate::Equality { .. } | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. }) => {
                panic!("expected an existence predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_single_prefixed_name_inside_a_hop_is_an_attribute_name_not_a_type_guard() {
        // This is what makes the clause 4.9 grammar unambiguous: a guard needs
        // two or more comma-separated types, so a lone `prefix:name` can only be
        // a prefixed attribute name.
        let expr = parse("a{prefix:name}==\"v\"").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "a");
                assert!(guard.is_empty(), "a lone prefixed name is not a guard: {guard:?}");
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "prefix:name");
            }
            other @ (Predicate::Exists(_) | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_traversal_predicate_combines_with_a_local_one_through_the_conjunction() {
        let expr = parse("speed>50;isParkedIn{category}==\"x\"").expect("parses");
        match expr {
            Expr::And(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match predicate_of(&terms[0]) {
                    Predicate::Ordering { path, .. } => pretty_assertions::assert_eq!(local_of(path).attr.as_str(), "speed"),
                    other @ (Predicate::Exists(_) | Predicate::Equality { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. }) => {
                        panic!("expected an ordering predicate, got {other:?}")
                    }
                }
                let (relationship, _, _) = hop_of(predicate_of(&terms[1]).path());
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
            }
            other @ (Expr::Or(_) | Expr::Pred(_)) => panic!("expected a conjunction, got {other:?}"),
        }
    }

    #[test]
    fn a_guard_type_that_is_not_the_last_one_keeps_the_colons_it_carries() {
        // Only the final type can carry the guard's terminating colon, because a
        // comma says another type follows. A prefixed type written before the
        // last one is therefore unambiguous and must not be split.
        let expr = parse("rel{ex:A,Store:category}==\"x\"").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "rel");
                pretty_assertions::assert_eq!(guard, vec!["ex:A", "Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "category");
            }
            other @ (Predicate::Exists(_) | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. }) => {
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
        let error = parse("rel{Building,ex:Store:category}==\"x\"").expect_err("must reject");
        assert!(matches!(error, QueryError::QAmbiguousTypeGuard), "got {error:?}");
    }

    #[test]
    fn quoting_a_guard_type_resolves_the_ambiguity_its_colons_would_create() {
        let expr = parse("rel{Building,\"ex:Store\":category}==\"x\"").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, .. } => {
                let (_, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(guard, vec!["Building", "ex:Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "category");
            }
            other @ (Predicate::Exists(_) | Predicate::Ordering { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. }) => {
                panic!("expected an equality predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_left_unclosed_is_rejected() {
        let error = parse("a{b").expect_err("must reject");
        assert!(
            matches!(
                error,
                QueryError::QExpected {
                    expected: QExpectation::Token(TokenKind::RBrace),
                    found: QFound::EndOfInput
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
            let error = parse(written).expect_err("must reject");
            assert!(
                matches!(
                    error,
                    QueryError::QExpected {
                        expected: QExpectation::Token(TokenKind::RBrace),
                        found: QFound::Token(_)
                    }
                ),
                "{written}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_hop_ending_in_a_compound_member_compares_that_member() {
        let expr = parse("a{b[c.d]}>=1").expect("parses");
        match predicate_of(&expr) {
            Predicate::Ordering { path, op, .. } => {
                let (_, _, inner) = hop_of(path);
                pretty_assertions::assert_eq!(local_of(inner).value_path.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["c", "d"]);
                pretty_assertions::assert_eq!(*op, OrderingOp::GreaterEq);
            }
            other @ (Predicate::Exists(_) | Predicate::Equality { .. } | Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. }) => {
                panic!("expected an ordering predicate, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_with_nothing_inside_it_is_rejected() {
        // Clause 4.9 requires a `LinkedEntityPath` between the braces.
        let error = parse("a{}").expect_err("must reject");
        assert!(matches!(error, QueryError::QExpected { .. }), "got {error:?}");
    }

    #[test]
    fn hop_nesting_is_bounded_by_the_same_budget_as_sub_attribute_nesting() {
        // A hop compiles to a correlated subquery per level, so an unbounded
        // chain is a denial-of-service vector rather than a legitimate query.
        let chain = |hops: usize| format!("{}z{}", "a{".repeat(hops), "}".repeat(hops));
        assert!(parse(&chain(MAX_SUB_ATTRIBUTE_DEPTH)).is_ok(), "a chain at the limit must parse");
        assert!(
            matches!(parse(&chain(MAX_SUB_ATTRIBUTE_DEPTH + 1)), Err(QueryError::QAttributePathTooDeep { .. })),
            "an over-deep hop chain must be rejected"
        );
    }
}
