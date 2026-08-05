//! Recursive-descent parser for the NGSI-LD Query Language (clause 4.9).

use crate::{
    error::QueryError,
    filter::{
        expression::Expr,
        lexer::{Token, tokenize},
        path::AttrPath,
        predicate::{CmpOp, Predicate},
        scalar::{CompareValue, Scalar, parse_date, parse_date_time, parse_time},
    },
    pattern::Pattern,
};
use andromeda_domain::attribute::name::AttributeName;
use serde_json::Number;

/// Parses an NGSI-LD `q` filter expression string into an AST.
///
/// The grammar follows NGSI-LD 4.9, supporting boolean combinators (`;` for AND,
/// `|` for OR), comparison operators, range/list values, regex patterns, and
/// nested attribute paths.
///
/// Returns a typed [`QueryError`] variant if the input is syntactically invalid.
pub fn parse(input: &str) -> Result<Expr, QueryError> {
    if input.trim().is_empty() {
        return Err(QueryError::QEmpty);
    }
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(&tokens);
    let expr = parser.parse_or()?;
    if parser.pos < parser.tokens.len() {
        return Err(QueryError::QUnexpectedToken {
            position: parser.pos,
            token: format!("{:?}", parser.tokens[parser.pos]),
        });
    }
    Ok(expr)
}

/// Cursor over a tokenised `q` expression.
struct Parser<'a> {
    /// The token stream being consumed.
    tokens: &'a [Token],
    /// Index of the next token to consume.
    pos: usize,
}

impl<'a> Parser<'a> {
    /// Start at the beginning of a token stream.
    const fn new(tokens: &'a [Token]) -> Parser<'a> {
        Parser { tokens, pos: 0 }
    }

    /// The next token without consuming it.
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// Consume and return the next token.
    fn advance(&mut self) -> Option<&'a Token> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    /// Consume the next token, requiring it to equal `expected`.
    fn expect(&mut self, expected: &Token) -> Result<(), QueryError> {
        if self.peek() == Some(expected) {
            self.advance();
            Ok(())
        } else {
            Err(QueryError::QExpected {
                expected: format!("{:?}", expected),
                found: format!("{:?}", self.peek()),
            })
        }
    }

    /// Parse a disjunction: `and_expr ('|' and_expr)*`.
    fn parse_or(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.parse_and()?;
        let mut terms = Vec::new();

        while self.peek() == Some(&Token::Pipe) {
            self.advance();
            terms.push(left);
            left = self.parse_and()?;
        }

        if terms.is_empty() {
            Ok(left)
        } else {
            terms.push(left);
            Ok(Expr::Or(terms))
        }
    }

    /// Parse a conjunction: `primary (';' primary)*`.
    fn parse_and(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.parse_primary()?;
        let mut terms = Vec::new();

        while self.peek() == Some(&Token::Semi) {
            self.advance();
            terms.push(left);
            left = self.parse_primary()?;
        }

        if terms.is_empty() {
            Ok(left)
        } else {
            terms.push(left);
            Ok(Expr::And(terms))
        }
    }

    /// Parse a primary: either a parenthesised group or a single predicate.
    fn parse_primary(&mut self) -> Result<Expr, QueryError> {
        if self.peek() == Some(&Token::LParen) {
            self.advance();
            let expr = self.parse_or()?;
            self.expect(&Token::RParen)?;
            Ok(expr)
        } else {
            Ok(Expr::Pred(self.parse_predicate()?))
        }
    }

    /// Parse one predicate: an attribute path optionally followed by an operator.
    fn parse_predicate(&mut self) -> Result<Predicate, QueryError> {
        let path = self.parse_attr_path()?;

        match self.peek() {
            Some(Token::Eq) => {
                self.advance();
                let rhs = self.parse_compare_value()?;
                Ok(Predicate::Compare { path, op: CmpOp::Equal, rhs })
            }
            Some(Token::Neq) => {
                self.advance();
                let rhs = self.parse_compare_value()?;
                Ok(Predicate::Compare { path, op: CmpOp::Unequal, rhs })
            }
            Some(Token::Gt) => {
                self.advance();
                let rhs = CompareValue::Scalar(self.parse_scalar()?);
                Ok(Predicate::Compare { path, op: CmpOp::Greater, rhs })
            }
            Some(Token::Ge) => {
                self.advance();
                let rhs = CompareValue::Scalar(self.parse_scalar()?);
                Ok(Predicate::Compare {
                    path,
                    op: CmpOp::GreaterEq,
                    rhs,
                })
            }
            Some(Token::Lt) => {
                self.advance();
                let rhs = CompareValue::Scalar(self.parse_scalar()?);
                Ok(Predicate::Compare { path, op: CmpOp::Less, rhs })
            }
            Some(Token::Le) => {
                self.advance();
                let rhs = CompareValue::Scalar(self.parse_scalar()?);
                Ok(Predicate::Compare { path, op: CmpOp::LessEq, rhs })
            }
            Some(Token::Match) => {
                self.advance();
                let pattern = self.parse_pattern_string()?;
                Ok(Predicate::MatchPattern { path, pattern })
            }
            Some(Token::NotMatch) => {
                self.advance();
                let pattern = self.parse_pattern_string()?;
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
                | Token::Dot
                | Token::Comma,
            )
            | None => Ok(Predicate::Exists(path)),
        }
    }

    /// Parse an attribute path: `ident ('.' ident)* ('[' ident ('.' ident)* ']')?`.
    fn parse_attr_path(&mut self) -> Result<AttrPath, QueryError> {
        let attr = self.parse_attribute_name()?;
        let mut sub_attrs = Vec::new();
        let mut value_path = Vec::new();

        while self.peek() == Some(&Token::Dot) {
            self.advance();
            sub_attrs.push(self.parse_attribute_name()?);
        }

        if self.peek() == Some(&Token::LBracket) {
            self.advance();
            value_path.push(self.parse_ident()?.to_string());
            while self.peek() == Some(&Token::Dot) {
                self.advance();
                value_path.push(self.parse_ident()?.to_string());
            }
            self.expect(&Token::RBracket)?;
        }

        Ok(AttrPath { attr, sub_attrs, value_path })
    }

    /// Consume an identifier naming an attribute, quoted or bare.
    ///
    /// The clause 5.2.4 name grammar accepts a short name, a prefixed name and
    /// an expanded IRI, which is the whole range a filter can carry before and
    /// after expansion, so validating here rejects nothing a client may write.
    fn parse_attribute_name(&mut self) -> Result<AttributeName, QueryError> {
        let raw = self.parse_ident()?;
        raw.parse().map_err(|source| QueryError::AttributeNameInvalid {
            value: raw.to_string(),
            source,
        })
    }

    /// Consume an identifier, quoted or bare.
    ///
    /// Used directly only for compound value keys, which the NGSI-LD data model
    /// does not constrain.
    fn parse_ident(&mut self) -> Result<&'a str, QueryError> {
        match self.advance() {
            Some(Token::Ident(s) | Token::QuotedStr(s)) => Ok(s.as_str()),
            other => Err(QueryError::QExpected {
                expected: "identifier".to_string(),
                found: format!("{:?}", other),
            }),
        }
    }

    /// Parse the right-hand side of a comparison: a scalar, a range, or a list.
    fn parse_compare_value(&mut self) -> Result<CompareValue, QueryError> {
        let first = self.parse_scalar()?;

        if self.peek() == Some(&Token::Dots) {
            self.advance();
            let second = self.parse_scalar()?;
            return Ok(CompareValue::Range { min: first, max: second });
        }

        if self.peek() == Some(&Token::Comma) {
            let mut values = vec![first];
            while self.peek() == Some(&Token::Comma) {
                self.advance();
                values.push(self.parse_scalar()?);
            }
            return Ok(CompareValue::ValueList(values));
        }

        Ok(CompareValue::Scalar(first))
    }

    /// Parse a single scalar operand, classifying temporal literals by shape.
    fn parse_scalar(&mut self) -> Result<Scalar, QueryError> {
        match self.advance() {
            Some(Token::NumberLit(s)) => {
                // clone: only on the error path, where the message owns the literal
                let num: Number = s.parse::<serde_json::Number>().map_err(|_| QueryError::QInvalidNumber(s.clone()))?;
                Ok(Scalar::Number(num))
            }
            // The literal's shape selects its domain; a literal that has a
            // temporal shape but does not parse is a malformed request, not a
            // string that happens to contain colons.
            Some(Token::QuotedStr(s) | Token::Ident(s)) => {
                if is_datetime(s) {
                    Ok(Scalar::DateTime(parse_date_time(s)?))
                } else if is_date(s) {
                    Ok(Scalar::Date(parse_date(s)?))
                } else if is_time(s) {
                    Ok(Scalar::Time(parse_time(s)?))
                } else {
                    // clone: the token stream is borrowed and the AST owns its strings
                    Ok(Scalar::Str(s.clone()))
                }
            }
            Some(Token::True) => Ok(Scalar::Bool(true)),
            Some(Token::False) => Ok(Scalar::Bool(false)),
            other => Err(QueryError::QExpected {
                expected: "value".to_string(),
                found: format!("{:?}", other),
            }),
        }
    }

    /// Consume the operand of `~=` / `!~=`, which may be quoted or bare.
    ///
    /// The operand is compiled here rather than at evaluation time, so a
    /// malformed regular expression is a rejected request instead of a filter
    /// that quietly matches nothing.
    fn parse_pattern_string(&mut self) -> Result<Pattern, QueryError> {
        match self.advance() {
            Some(Token::QuotedStr(s) | Token::Ident(s)) => Pattern::parse(s),
            other => Err(QueryError::QExpected {
                expected: "pattern string".to_string(),
                found: format!("{:?}", other),
            }),
        }
    }
}

/// Whether a literal has the shape of an ISO 8601 date-time.
fn is_datetime(s: &str) -> bool {
    s.len() >= 19 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(10) == Some(&b'T')
}

/// Whether a literal has the shape of an ISO 8601 date.
fn is_date(s: &str) -> bool {
    s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-')
}

/// Whether a literal has the shape of an ISO 8601 time.
fn is_time(s: &str) -> bool {
    s.len() >= 8 && s.as_bytes().get(2) == Some(&b':') && s.as_bytes().get(5) == Some(&b':')
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::SecondsFormat;

    #[test]
    fn parse_existence() {
        let expr = parse("temperature").unwrap();
        pretty_assertions::assert_eq!(
            expr,
            Expr::Pred(Predicate::Exists(AttrPath::simple("temperature".parse().expect("valid attribute name"))))
        );
    }

    #[test]
    fn parse_equality_number() {
        let expr = parse("temperature==20").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.attr.as_str(), "temperature");
                pretty_assertions::assert_eq!(op, CmpOp::Equal);
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Number(20.into())));
            }
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_inequality_string() {
        let expr = parse("brandName!=\"Mercedes\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.attr.as_str(), "brandName");
                pretty_assertions::assert_eq!(op, CmpOp::Unequal);
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Str("Mercedes".to_string())));
            }
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_greater_than() {
        let expr = parse("speed>50").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.attr.as_str(), "speed");
                pretty_assertions::assert_eq!(op, CmpOp::Greater);
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Number(50.into())));
            }
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_expression() {
        let expr = parse("speed>50;brandName!=\"Mercedes\"").unwrap();
        match expr {
            Expr::And(terms) => pretty_assertions::assert_eq!(terms.len(), 2),
            other => unreachable!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parse_or_expression() {
        let expr = parse("speed>50|temperature<10").unwrap();
        match expr {
            Expr::Or(terms) => pretty_assertions::assert_eq!(terms.len(), 2),
            other => unreachable!("expected Or, got {other:?}"),
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
                    other => unreachable!("expected And as second OR term, got {other:?}"),
                }
            }
            other => unreachable!("expected Or, got {other:?}"),
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
                    other => unreachable!("expected Or inside parens, got {other:?}"),
                }
            }
            other => unreachable!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parse_range() {
        let expr = parse("temperature==10..20").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::Range { min, max } => {
                    pretty_assertions::assert_eq!(min, Scalar::Number(10.into()));
                    pretty_assertions::assert_eq!(max, Scalar::Number(20.into()));
                }
                other => unreachable!("expected Range, got {other:?}"),
            },
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_value_list() {
        let expr = parse("color==\"red\",\"blue\",\"green\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::ValueList(vals) => {
                    pretty_assertions::assert_eq!(vals.len(), 3);
                }
                other => unreachable!("expected ValueList, got {other:?}"),
            },
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_match_pattern() {
        let expr = parse("brandName~=\"Merc.*\"").unwrap();
        match expr {
            Expr::Pred(Predicate::MatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.attr.as_str(), "brandName");
                pretty_assertions::assert_eq!(pattern.as_str(), "Merc.*");
            }
            other => unreachable!("expected MatchPattern, got {other:?}"),
        }
    }

    #[test]
    fn parse_not_match_pattern() {
        let expr = parse("brandName!~=\"Merc.*\"").unwrap();
        match expr {
            Expr::Pred(Predicate::NotMatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.attr.as_str(), "brandName");
                pretty_assertions::assert_eq!(pattern.as_str(), "Merc.*");
            }
            other => unreachable!("expected NotMatchPattern, got {other:?}"),
        }
    }

    #[test]
    fn a_date_time_scalar_is_parsed_once_at_filter_parse_time() {
        // Clause 4.6.3: a temporal operand is an RFC 3339 value, so a literal
        // that has the shape of one but is not a real instant is a malformed
        // request rather than a string that quietly compares as text.
        match parse("observed>=2017-12-24T12:00:00Z").expect("parses") {
            Expr::Pred(Predicate::Compare {
                rhs: CompareValue::Scalar(Scalar::DateTime(_)),
                ..
            }) => {}
            other => unreachable!("expected a date-time operand, got {other:?}"),
        }

        let error = parse("observed>=2017-13-45").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidDate { .. }), "got {error:?}");

        let error = parse("observed>=\"2017-12-24T99:00:00Z\"").expect_err("must reject");
        assert!(matches!(error, QueryError::QInvalidDateTime { .. }), "got {error:?}");
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
            Expr::Pred(Predicate::Compare { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.attr.as_str(), "temperature");
                pretty_assertions::assert_eq!(path.sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["observedAt"]);
                pretty_assertions::assert_eq!(op, CmpOp::GreaterEq);
                match rhs {
                    CompareValue::Scalar(Scalar::DateTime(dt)) => {
                        pretty_assertions::assert_eq!(dt.to_rfc3339_opts(SecondsFormat::Secs, true), "2017-12-24T12:00:00Z");
                    }
                    other => unreachable!("expected DateTime scalar, got {other:?}"),
                }
            }
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_compound_value_path() {
        let expr = parse("address[city]==\"Berlin\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, .. }) => {
                pretty_assertions::assert_eq!(path.attr.as_str(), "address");
                pretty_assertions::assert_eq!(path.value_path, vec!["city"]);
            }
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_nested_compound_value_path() {
        let expr = parse("sensor.rawdata[airquality.particulate]==40").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, .. }) => {
                pretty_assertions::assert_eq!(path.attr.as_str(), "sensor");
                pretty_assertions::assert_eq!(path.sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["rawdata"]);
                pretty_assertions::assert_eq!(path.value_path, vec!["airquality", "particulate"]);
            }
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_less_equal() {
        let expr = parse("capacity<=10").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { op, .. }) => {
                pretty_assertions::assert_eq!(op, CmpOp::LessEq);
            }
            other => unreachable!("expected Compare, got {other:?}"),
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
                    other => unreachable!("expected And, got {other:?}"),
                }
            }
            other => unreachable!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_uri_equality() {
        let expr = parse("isParked==\"urn:ngsi-ld:OffStreetParking:Downtown1\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Str(s)) => {
                    pretty_assertions::assert_eq!(s, "urn:ngsi-ld:OffStreetParking:Downtown1");
                }
                other => unreachable!("expected Str, got {other:?}"),
            },
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_negative_number() {
        let expr = parse("temperature>-5").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Number(n)) => {
                    pretty_assertions::assert_eq!(n.as_f64(), Some(-5.0));
                }
                other => unreachable!("expected Number, got {other:?}"),
            },
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_float_number() {
        let expr = parse("temperature==23.5").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Number(n)) => {
                    pretty_assertions::assert_eq!(n.as_f64(), Some(23.5));
                }
                other => unreachable!("expected Number, got {other:?}"),
            },
            other => unreachable!("expected Compare, got {other:?}"),
        }
    }

    #[test]
    fn parse_boolean_value() {
        let expr = parse("active==true").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => {
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Bool(true)));
            }
            other => unreachable!("expected Compare, got {other:?}"),
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
}
