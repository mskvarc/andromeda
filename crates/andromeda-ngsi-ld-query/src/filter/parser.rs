//! Recursive-descent parser for the NGSI-LD Query Language (clause 4.9).
//!
//! The grammar this parser implements is reproduced verbatim in
//! `grammar/query.abnf` at the root of this crate. Nothing derives from that
//! file — the language needs operator precedence and value typing that a pure
//! ABNF recogniser does not express — so it is the reference this parser is
//! checked against, and the two are kept in step by hand.
//!
//! All three attribute axes are implemented: `.` sub-attributes and `[]`
//! compound values, which stay inside one entity and build an [`AttrPath`], and
//! `{}` linked-entity traversal (`LinkedEntityRelation`), which crosses a
//! Relationship and builds a [`QueryPath::Linked`] hop around the predicate
//! parsed inside the braces.

use crate::{
    error::QueryError,
    filter::{
        expression::Expr,
        lexer::{Token, tokenize},
        path::{AttrPath, QueryPath},
        predicate::{OrderingOp, Predicate, SetMembership},
        scalar::{CompareValue, Scalar},
    },
    pattern::Pattern,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde_json::Number;

/// The most segments an attribute path may nest, counting sub-attributes and
/// linked-entity hops alike.
///
/// Far above any legitimate NGSI-LD nesting (an attribute's sub-attributes are
/// rarely more than one deep, and a linked-entity traversal rarely more than two
/// hops), this only rejects a pathological path whose backend rendering would
/// otherwise grow with its depth — a hop compiles to a correlated subquery per
/// level, so `a{a{a{…}}}` is as expensive as it is deep.
const MAX_SUB_ATTRIBUTE_DEPTH: usize = 16;

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

/// How a name was written in the query text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quoting {
    /// Written bare, so the tokeniser may have glued following `:` characters
    /// to it — `:` continues an identifier.
    Bare,
    /// Written inside double quotes, so it ends exactly where the client ended
    /// it and any colon it contains belongs to the name.
    Quoted,
}

/// What the head of a linked-entity hop contributed before its inner path.
struct HopGuard<'a> {
    /// The entity types the hop is constrained to, empty when it carried none.
    types: Vec<AttributeName>,
    /// The inner path's leading name when the tokeniser glued it to the guard's
    /// terminating colon; `None` when the inner path starts at the next token.
    inner_leading: Option<&'a str>,
}

/// Read an identifier as an attribute name.
///
/// The clause 4.6.2 name grammar accepts a short name, a prefixed name and an
/// expanded IRI, which is the whole range a filter can carry before and after
/// expansion, so validating here rejects nothing a client may write.
fn attribute_name(raw: &str) -> Result<AttributeName, QueryError> {
    raw.parse().map_err(|source| QueryError::AttributeNameInvalid {
        value: raw.to_string(),
        source,
    })
}

/// How a parse failure describes the token it found, or its absence.
fn describe(token: Option<&Token>) -> String {
    match token {
        Some(token) => token.spelling().to_string(),
        None => "end of input".to_string(),
    }
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

    /// The token `offset` positions ahead without consuming anything.
    fn peek_ahead(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset)
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
                expected: expected.spelling().to_string(),
                found: self.found_description(),
            })
        }
    }

    /// How a parse failure describes whatever stands where a token was required.
    fn found_description(&self) -> String {
        describe(self.peek())
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

    /// Parse one predicate: an attribute axis optionally followed by an operator.
    fn parse_predicate(&mut self) -> Result<Predicate, QueryError> {
        let leading = self.parse_ident()?;
        self.parse_predicate_from(leading, 0)
    }

    /// Parse one predicate whose leading attribute name has already been read.
    ///
    /// `depth` counts the path segments consumed so far — sub-attributes and
    /// linked-entity hops together — so a chain of hops is bounded by the same
    /// budget a chain of sub-attributes is.
    ///
    /// A `{` after the leading name opens a clause 4.9 `LinkedEntityRelation`.
    /// Its contents are a whole predicate, operator and operand included, so the
    /// inner predicate is parsed first and the hop is wrapped around it.
    fn parse_predicate_from(&mut self, leading: &'a str, depth: usize) -> Result<Predicate, QueryError> {
        if self.peek() != Some(&Token::LBrace) {
            let path = self.parse_attr_path_from(leading, depth)?;
            return self.parse_operator_and_operand(QueryPath::Local(path));
        }

        let relationship = attribute_name(leading)?;
        self.advance();
        Self::check_depth(depth + 1)?;
        let guard = self.parse_hop_guard()?;
        let inner = match guard.inner_leading {
            Some(text) => self.parse_predicate_from(text, depth + 1)?,
            None => {
                let leading = self.parse_ident()?;
                self.parse_predicate_from(leading, depth + 1)?
            }
        };
        self.expect(&Token::RBrace)?;
        Ok(inner.nest_under_hop(relationship, guard.types))
    }

    /// Reject a path that nests deeper than the budget.
    const fn check_depth(depth: usize) -> Result<(), QueryError> {
        if depth > MAX_SUB_ATTRIBUTE_DEPTH {
            return Err(QueryError::QAttributePathTooDeep {
                depth,
                limit: MAX_SUB_ATTRIBUTE_DEPTH,
            });
        }
        Ok(())
    }

    /// Parse the optional entity-type guard at the head of a linked-entity hop.
    ///
    /// Clause 4.9 spells the guard `*1(EntityType 1*("," EntityType) ":")`: two
    /// or more comma-separated types, then a colon. The tokeniser is
    /// context-free and `:` is a legal identifier character, so the colon
    /// arrives glued to whatever the client wrote next — `Store:category` is one
    /// identifier. The guard is therefore recognised from shape, not from a
    /// colon token: a comma directly after the first name inside the braces
    /// means a guard, and nothing else in the grammar puts a comma there. A lone
    /// `prefix:name` has no comma and is an ordinary prefixed attribute name.
    fn parse_hop_guard(&mut self) -> Result<HopGuard<'a>, QueryError> {
        let opens_guard = matches!(self.peek(), Some(Token::Ident(_) | Token::QuotedStr(_))) && self.peek_ahead(1) == Some(&Token::Comma);
        if !opens_guard {
            return Ok(HopGuard {
                types: Vec::new(),
                inner_leading: None,
            });
        }

        let mut types = Vec::new();
        loop {
            let (name, quoting) = self.parse_guard_name()?;
            match self.split_guard_terminator(name, quoting)? {
                Some((last, inner_leading)) => {
                    types.push(attribute_name(last)?);
                    return Ok(HopGuard { types, inner_leading });
                }
                None => {
                    types.push(attribute_name(name)?);
                    self.expect(&Token::Comma)?;
                }
            }
        }
    }

    /// Consume one name of a type guard, reporting whether it was written bare.
    ///
    /// The distinction decides how the guard's terminating colon is found: only
    /// a bare identifier can have the colon glued to it, because a quoted string
    /// ends at its closing quote — and a quoted name is an expanded IRI, whose
    /// own colons are part of the IRI rather than the grammar.
    fn parse_guard_name(&mut self) -> Result<(&'a str, Quoting), QueryError> {
        match self.advance() {
            Some(Token::Ident(name)) => Ok((name.as_str(), Quoting::Bare)),
            Some(Token::QuotedStr(name)) => Ok((name.as_str(), Quoting::Quoted)),
            other => Err(QueryError::QExpected {
                expected: "entity type".to_string(),
                found: describe(other),
            }),
        }
    }

    /// Split the guard's last type from the inner path that follows its colon,
    /// or report that this name is not the last one.
    ///
    /// A comma is checked first: it says another type follows, so this name
    /// cannot be carrying the terminator and whatever colons it holds are part
    /// of the name — `rel{ex:A,ex:B:attr}` has a prefixed type in each position
    /// and only the last one is split.
    ///
    /// For the last name the colon reaches the parser in one of two shapes,
    /// both of which the tokeniser produces without any state: glued to the type
    /// it terminates (`Store:` or `Store:category`, since `:` continues an
    /// identifier), or as its own `:`-prefixed identifier when the type was
    /// quoted — which is how an expanded filter round-trips, its IRIs being
    /// quoted. An empty remainder means the inner path starts at the next token.
    ///
    /// A bare last name holding *two* colons is ambiguous and is rejected rather
    /// than resolved by guesswork: `ex:Store:category` reads equally well as the
    /// prefixed type `ex:Store` before the attribute `category` or as the type
    /// `ex` before the prefixed attribute `Store:category`, and clause 4.9
    /// decides neither — its `EntityType` production admits no colon at all,
    /// while real filters do carry prefixed names. Quoting the type says which
    /// was meant, which is what the error asks for.
    fn split_guard_terminator(&mut self, name: &'a str, quoting: Quoting) -> Result<Option<(&'a str, Option<&'a str>)>, QueryError> {
        if self.peek() == Some(&Token::Comma) {
            return Ok(None);
        }
        if quoting == Quoting::Bare
            && let Some((last, rest)) = name.split_once(':')
        {
            if rest.contains(':') {
                return Err(QueryError::QExpected {
                    expected: "a quoted entity type, so the colon ending the type guard is unambiguous".to_string(),
                    found: name.to_string(),
                });
            }
            let inner_leading = (!rest.is_empty()).then_some(rest);
            return Ok(Some((last, inner_leading)));
        }
        if let Some(Token::Ident(next)) = self.tokens.get(self.pos)
            && let Some(rest) = next.strip_prefix(':')
        {
            self.pos += 1;
            let inner_leading = (!rest.is_empty()).then_some(rest);
            return Ok(Some((name, inner_leading)));
        }
        Err(QueryError::QExpected {
            expected: ":".to_string(),
            found: self.found_description(),
        })
    }

    /// Parse an attribute path whose leading name has already been read:
    /// `('.' ident)* ('[' ident ('.' ident)* ']')?`.
    fn parse_attr_path_from(&mut self, leading: &'a str, depth: usize) -> Result<AttrPath, QueryError> {
        let attr = attribute_name(leading)?;
        let mut sub_attrs = Vec::new();
        let mut value_path = Vec::new();
        let mut match_all_languages = false;

        while self.peek() == Some(&Token::Dot) {
            self.advance();
            sub_attrs.push(self.parse_attribute_name()?);
            Self::check_depth(depth + sub_attrs.len())?;
        }

        if self.peek() == Some(&Token::LBracket) {
            self.advance();
            if self.peek() == Some(&Token::Star) {
                // `[*]` is the language-suppression marker, not a value-path
                // key: it matches every language of a LanguageProperty.
                self.advance();
                match_all_languages = true;
            } else {
                value_path.push(self.parse_attribute_name()?);
                while self.peek() == Some(&Token::Dot) {
                    self.advance();
                    value_path.push(self.parse_attribute_name()?);
                }
            }
            self.expect(&Token::RBracket)?;
        }

        // clone: `value_path_compact` starts as the same terms as `value_path`
        // and stays that way until expansion overwrites `value_path` alone, so
        // each field needs its own owned copy of the freshly parsed names.
        let value_path_compact = value_path.clone();
        Ok(AttrPath {
            attr,
            sub_attrs,
            value_path,
            value_path_compact,
            match_all_languages,
        })
    }

    /// Read the comparison operator and operand that follow an attribute axis,
    /// or accept the axis on its own as an existence predicate.
    fn parse_operator_and_operand(&mut self, path: QueryPath) -> Result<Predicate, QueryError> {
        match self.peek() {
            Some(Token::Eq) => {
                self.advance();
                let rhs = self.parse_compare_value()?;
                Ok(Predicate::Equality {
                    path,
                    membership: SetMembership::Within,
                    rhs,
                })
            }
            Some(Token::Neq) => {
                self.advance();
                let rhs = self.parse_compare_value()?;
                Ok(Predicate::Equality {
                    path,
                    membership: SetMembership::Outside,
                    rhs,
                })
            }
            Some(Token::Gt) => {
                self.advance();
                let rhs = self.parse_scalar()?;
                Ok(Predicate::Ordering {
                    path,
                    op: OrderingOp::Greater,
                    rhs,
                })
            }
            Some(Token::Ge) => {
                self.advance();
                let rhs = self.parse_scalar()?;
                Ok(Predicate::Ordering {
                    path,
                    op: OrderingOp::GreaterEq,
                    rhs,
                })
            }
            Some(Token::Lt) => {
                self.advance();
                let rhs = self.parse_scalar()?;
                Ok(Predicate::Ordering {
                    path,
                    op: OrderingOp::Less,
                    rhs,
                })
            }
            Some(Token::Le) => {
                self.advance();
                let rhs = self.parse_scalar()?;
                Ok(Predicate::Ordering {
                    path,
                    op: OrderingOp::LessEq,
                    rhs,
                })
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
                | Token::LBrace
                | Token::RBrace
                | Token::Star
                | Token::Dot
                | Token::Comma,
            )
            | None => Ok(Predicate::Exists(path)),
        }
    }

    /// Consume an identifier naming an attribute, quoted or bare.
    fn parse_attribute_name(&mut self) -> Result<AttributeName, QueryError> {
        attribute_name(self.parse_ident()?)
    }

    /// Consume an identifier, quoted or bare.
    fn parse_ident(&mut self) -> Result<&'a str, QueryError> {
        match self.advance() {
            Some(Token::Ident(s) | Token::QuotedStr(s)) => Ok(s.as_str()),
            other => Err(QueryError::QExpected {
                expected: "identifier".to_string(),
                found: describe(other),
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
            Some(Token::QuotedStr(s) | Token::Ident(s)) => Scalar::from_literal(s),
            Some(Token::True) => Ok(Scalar::Bool(true)),
            Some(Token::False) => Ok(Scalar::Bool(false)),
            other => Err(QueryError::QExpected {
                expected: "value".to_string(),
                found: describe(other),
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
                found: describe(other),
            }),
        }
    }
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
            other => unreachable!("expected Equality, got {other:?}"),
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
            other => unreachable!("expected Equality, got {other:?}"),
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
            other => unreachable!("expected Ordering, got {other:?}"),
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
            Expr::Pred(Predicate::Equality { rhs, .. }) => match rhs {
                CompareValue::Range { min, max } => {
                    pretty_assertions::assert_eq!(min, Scalar::Number(10.into()));
                    pretty_assertions::assert_eq!(max, Scalar::Number(20.into()));
                }
                other => unreachable!("expected Range, got {other:?}"),
            },
            other => unreachable!("expected Equality, got {other:?}"),
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
                other => unreachable!("expected ValueList, got {other:?}"),
            },
            other => unreachable!("expected Equality, got {other:?}"),
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
            other => unreachable!("expected MatchPattern, got {other:?}"),
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
            other => unreachable!("expected NotMatchPattern, got {other:?}"),
        }
    }

    #[test]
    fn a_date_time_scalar_is_parsed_once_at_filter_parse_time() {
        // Clause 4.6.3: a temporal operand is an RFC 3339 value, so a literal
        // that has the shape of one but is not a real instant is a malformed
        // request rather than a string that quietly compares as text.
        match parse("observed>=2017-12-24T12:00:00Z").expect("parses") {
            Expr::Pred(Predicate::Ordering { rhs: Scalar::DateTime(_), .. }) => {}
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
            Expr::Pred(Predicate::Ordering { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.terminal().attr.as_str(), "temperature");
                pretty_assertions::assert_eq!(
                    path.terminal().sub_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
                    vec!["observedAt"]
                );
                pretty_assertions::assert_eq!(op, OrderingOp::GreaterEq);
                match rhs {
                    Scalar::DateTime(dt) => {
                        pretty_assertions::assert_eq!(dt.to_rfc3339_opts(SecondsFormat::Secs, true), "2017-12-24T12:00:00Z");
                    }
                    other => unreachable!("expected DateTime scalar, got {other:?}"),
                }
            }
            other => unreachable!("expected Ordering, got {other:?}"),
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
            other => unreachable!("expected Equality, got {other:?}"),
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
            other => unreachable!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_less_equal() {
        let expr = parse("capacity<=10").unwrap();
        match expr {
            Expr::Pred(Predicate::Ordering { op, .. }) => {
                pretty_assertions::assert_eq!(op, OrderingOp::LessEq);
            }
            other => unreachable!("expected Ordering, got {other:?}"),
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
            Expr::Pred(Predicate::Equality { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Str(s)) => {
                    pretty_assertions::assert_eq!(s, "urn:ngsi-ld:OffStreetParking:Downtown1");
                }
                other => unreachable!("expected Str, got {other:?}"),
            },
            other => unreachable!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_negative_number() {
        let expr = parse("temperature>-5").unwrap();
        match expr {
            Expr::Pred(Predicate::Ordering { rhs, .. }) => match rhs {
                Scalar::Number(n) => {
                    pretty_assertions::assert_eq!(n.as_f64(), Some(-5.0));
                }
                other => unreachable!("expected Number, got {other:?}"),
            },
            other => unreachable!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_float_number() {
        let expr = parse("temperature==23.5").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Number(n)) => {
                    pretty_assertions::assert_eq!(n.as_f64(), Some(23.5));
                }
                other => unreachable!("expected Number, got {other:?}"),
            },
            other => unreachable!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_boolean_value() {
        let expr = parse("active==true").unwrap();
        match expr {
            Expr::Pred(Predicate::Equality { rhs, .. }) => {
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Bool(true)));
            }
            other => unreachable!("expected Equality, got {other:?}"),
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

    // -- linked-entity traversal (clause 4.9 `LinkedEntityRelation`) ---------

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
            QueryPath::Local(local) => unreachable!("expected a linked-entity hop, got the local path {local:?}"),
        }
    }

    fn local_of(path: &QueryPath) -> &AttrPath {
        match path {
            QueryPath::Local(local) => local,
            QueryPath::Linked { relationship, .. } => unreachable!("expected a local path, got a hop through {relationship}"),
        }
    }

    fn predicate_of(expr: &Expr) -> &Predicate {
        match expr {
            Expr::Pred(pred) => pred,
            other => unreachable!("expected a single predicate, got {other:?}"),
        }
    }

    #[test]
    fn a_linked_entity_hop_wraps_the_predicate_written_inside_its_braces() {
        // Clause 4.9 `LinkedEntityRelation = AttrName %x7B LinkedEntityPath %x7D`:
        // the operator and its operand sit inside the braces, and the whole
        // comparison is what the hop applies to the linked entity.
        let expr = parse("isParkedIn{category==\"commercial\"}").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, membership, rhs } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
                assert!(guard.is_empty(), "no type guard was written: {guard:?}");
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "category");
                pretty_assertions::assert_eq!(*membership, SetMembership::Within);
                pretty_assertions::assert_eq!(*rhs, CompareValue::Scalar(Scalar::Str("commercial".to_string())));
            }
            other => unreachable!("expected an equality predicate, got {other:?}"),
        }
    }

    #[test]
    fn a_hop_carrying_two_comma_separated_types_parses_them_as_its_guard() {
        // The clause 4.9 guard production is `*1(EntityType 1*("," EntityType) ":")`,
        // so two or more types followed by a colon constrain the linked entity.
        let expr = parse("isParkedIn{Building,Store:category==\"x\"}").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
                pretty_assertions::assert_eq!(guard, vec!["Building", "Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "category");
            }
            other => unreachable!("expected an equality predicate, got {other:?}"),
        }
    }

    #[test]
    fn hops_nest_and_the_innermost_one_carries_the_comparison() {
        let expr = parse("a{b{c==5}}").expect("parses");
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
            other => unreachable!("expected an equality predicate, got {other:?}"),
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
            other => unreachable!("expected an existence predicate, got {other:?}"),
        }
    }

    #[test]
    fn a_single_prefixed_name_inside_a_hop_is_an_attribute_name_not_a_type_guard() {
        // This is what makes the clause 4.9 grammar unambiguous: a guard needs
        // two or more comma-separated types, so a lone `prefix:name` can only be
        // a prefixed attribute name.
        let expr = parse("a{prefix:name==\"v\"}").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "a");
                assert!(guard.is_empty(), "a lone prefixed name is not a guard: {guard:?}");
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "prefix:name");
            }
            other => unreachable!("expected an equality predicate, got {other:?}"),
        }
    }

    #[test]
    fn a_traversal_predicate_combines_with_a_local_one_through_the_conjunction() {
        let expr = parse("speed>50;isParkedIn{category==\"x\"}").expect("parses");
        match expr {
            Expr::And(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match predicate_of(&terms[0]) {
                    Predicate::Ordering { path, .. } => pretty_assertions::assert_eq!(local_of(path).attr.as_str(), "speed"),
                    other => unreachable!("expected an ordering predicate, got {other:?}"),
                }
                let (relationship, _, _) = hop_of(predicate_of(&terms[1]).path());
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
            }
            other => unreachable!("expected a conjunction, got {other:?}"),
        }
    }

    #[test]
    fn a_guard_type_that_is_not_the_last_one_keeps_the_colons_it_carries() {
        // Only the final type can carry the guard's terminating colon, because a
        // comma says another type follows. A prefixed type written before the
        // last one is therefore unambiguous and must not be split.
        let expr = parse("rel{ex:A,Store:category==\"x\"}").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "rel");
                pretty_assertions::assert_eq!(guard, vec!["ex:A", "Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "category");
            }
            other => unreachable!("expected an equality predicate, got {other:?}"),
        }
    }

    #[test]
    fn a_final_guard_type_carrying_two_colons_is_rejected_rather_than_split_by_guesswork() {
        // `ex:Store:category` could be the prefixed type `ex:Store` followed by
        // `category`, or the type `ex` followed by the prefixed attribute
        // `Store:category`. Nothing in the clause 4.9 grammar decides between
        // them, so the request is rejected and the client quotes what it meant.
        let error = parse("rel{Building,ex:Store:category==\"x\"}").expect_err("must reject");
        assert!(matches!(error, QueryError::QExpected { .. }), "got {error:?}");
    }

    #[test]
    fn quoting_a_guard_type_resolves_the_ambiguity_its_colons_would_create() {
        let expr = parse("rel{Building,\"ex:Store\":category==\"x\"}").expect("parses");
        match predicate_of(&expr) {
            Predicate::Equality { path, .. } => {
                let (_, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(guard, vec!["Building", "ex:Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attr.as_str(), "category");
            }
            other => unreachable!("expected an equality predicate, got {other:?}"),
        }
    }

    #[test]
    fn a_hop_left_unclosed_is_rejected() {
        let error = parse("a{b==1").expect_err("must reject");
        assert!(matches!(&error, QueryError::QExpected { expected, .. } if expected == "}"), "got {error:?}");
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
