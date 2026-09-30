//! Parser for the NGSI-LD Entity Type Selection Language (S4.17).
//!
//! The grammar supports boolean expressions over type names:
//! - `|` and `,` are OR operators (lowest precedence)
//! - `;` is the AND operator (higher precedence)
//! - `()` for grouping
//!
//! Example: `(Home;Vehicle)|Motorhome` matches entities that have both
//! `Home` and `Vehicle` types, or entities with `Motorhome` type.

use crate::{error::QueryError, type_selection::grammar::TypeExprRaw};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;

/// Parses a type selection expression string into a raw AST.
///
/// # Errors
///
/// Returns [`QueryError::TypeExprEmpty`] for blank input,
/// [`QueryError::TypeExprUnexpectedEnd`] and [`QueryError::TypeExprUnbalanced`]
/// for an expression that ends early or leaves a parenthesis open,
/// [`QueryError::TypeExprUnexpected`] for a character outside the grammar, and
/// [`QueryError::TypeExprEmptyName`] or [`QueryError::TypeExprInvalidName`] when
/// a type-name production yields no valid clause 4.6.2 name.
pub fn parse(input: &str) -> Result<TypeExprRaw, QueryError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(QueryError::TypeExprEmpty);
    }
    if trimmed == "*" {
        return Ok(TypeExprRaw::Wildcard);
    }

    let mut parser = Parser::new(trimmed);
    let expr = parser.parse_or()?;

    if parser.pos < parser.input.len() {
        let ch = parser.input[parser.pos..].chars().next().unwrap_or(' ');
        return Err(QueryError::TypeExprUnexpected { ch, position: parser.pos });
    }

    Ok(expr)
}

/// Cursor over a type selection expression.
struct Parser<'a> {
    /// The expression being consumed.
    input: &'a str,
    /// Byte offset of the next character to consume.
    pos: usize,
}

impl<'a> Parser<'a> {
    /// Start at the beginning of an expression.
    const fn new(input: &'a str) -> Parser<'a> {
        Parser { input, pos: 0 }
    }

    /// The next character without consuming it.
    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    /// Consume the next character.
    fn advance(&mut self) {
        if let Some(c) = self.peek() {
            self.pos += c.len_utf8();
        }
    }

    /// Parses an OR expression: `and_expr (('|' | ',') and_expr)*`
    ///
    /// A single operand is returned as itself rather than wrapped in a
    /// one-element `Or`. The first operand is held separately from the rest so
    /// that "was there more than one?" is a property of the remainder, not a
    /// length test on a vector that the code then has to prove is non-empty.
    fn parse_or(&mut self) -> Result<TypeExprRaw, QueryError> {
        let first = self.parse_and()?;
        let mut rest = Vec::new();

        while matches!(self.peek(), Some('|' | ',')) {
            self.advance();
            rest.push(self.parse_and()?);
        }

        if rest.is_empty() {
            return Ok(first);
        }
        let mut terms = Vec::with_capacity(rest.len() + 1);
        terms.push(first);
        terms.append(&mut rest);
        Ok(TypeExprRaw::Or(terms))
    }

    /// Parses an AND expression: `primary (';' primary)*`
    ///
    /// Single operands collapse to themselves, as in [`Parser::parse_or`].
    fn parse_and(&mut self) -> Result<TypeExprRaw, QueryError> {
        let first = self.parse_primary()?;
        let mut rest = Vec::new();

        while self.peek() == Some(';') {
            self.advance();
            rest.push(self.parse_primary()?);
        }

        if rest.is_empty() {
            return Ok(first);
        }
        let mut terms = Vec::with_capacity(rest.len() + 1);
        terms.push(first);
        terms.append(&mut rest);
        Ok(TypeExprRaw::And(terms))
    }

    /// Parses a primary: either a parenthesized group or a bare type name.
    fn parse_primary(&mut self) -> Result<TypeExprRaw, QueryError> {
        self.skip_whitespace();

        match self.peek() {
            Some('(') => {
                self.advance();
                let expr = self.parse_or()?;
                self.skip_whitespace();
                if self.peek() != Some(')') {
                    return Err(QueryError::TypeExprUnbalanced(self.pos));
                }
                self.advance();
                Ok(expr)
            }
            Some(c) if is_type_char(c) => self.parse_type_name(),
            Some(c) => Err(QueryError::TypeExprUnexpected { ch: c, position: self.pos }),
            None => Err(QueryError::TypeExprUnexpectedEnd),
        }
    }

    /// Parses a bare type name (any non-delimiter characters).
    fn parse_type_name(&mut self) -> Result<TypeExprRaw, QueryError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if is_type_char(c) {
                self.advance();
            } else {
                break;
            }
        }
        let name = self.input[start..self.pos].trim();
        if name.is_empty() {
            return Err(QueryError::TypeExprEmptyName(start));
        }
        let name = name
            .parse::<EntityTypeName>()
            .map_err(|source| QueryError::TypeExprInvalidName { position: start, source })?;
        Ok(TypeExprRaw::Type(name))
    }

    /// Skip over any run of whitespace.
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }
}

/// Returns `true` for characters that may appear in a type name.
/// Excludes the expression operators: `|`, `,`, `;`, `(`, `)`.
const fn is_type_char(c: char) -> bool {
    !matches!(c, '|' | ',' | ';' | '(' | ')') && !c.is_whitespace()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str) -> TypeExprRaw {
        TypeExprRaw::Type(name.parse().expect("valid type name"))
    }

    #[test]
    fn parse_single_type() {
        let expr = parse("Vehicle").unwrap();
        pretty_assertions::assert_eq!(expr, t("Vehicle"));
    }

    #[test]
    fn parse_or_with_pipe() {
        let expr = parse("Building|House").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::Or(vec![t("Building"), t("House"),]));
    }

    #[test]
    fn parse_or_with_comma() {
        let expr = parse("Building,House").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::Or(vec![t("Building"), t("House"),]));
    }

    #[test]
    fn parse_and_with_semicolon() {
        let expr = parse("Home;Vehicle").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::And(vec![t("Home"), t("Vehicle"),]));
    }

    #[test]
    fn parse_mixed_and_or_with_grouping() {
        let expr = parse("(Home;Vehicle)|Motorhome").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::Or(vec![TypeExprRaw::And(vec![t("Home"), t("Vehicle"),]), t("Motorhome"),]));
    }

    #[test]
    fn parse_nested_parens() {
        let expr = parse("((A;B)|C)").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::Or(vec![TypeExprRaw::And(vec![t("A"), t("B"),]), t("C"),]));
    }

    #[test]
    fn parse_uri_type_name() {
        let expr = parse("https://example.com/Vehicle").unwrap();
        pretty_assertions::assert_eq!(expr, t("https://example.com/Vehicle"));
    }

    #[test]
    fn a_type_name_outside_the_clause_4_6_2_grammar_is_rejected() {
        // A leaf must satisfy the NGSI-LD name grammar, so a name that is neither
        // a term, a compact IRI, nor an absolute IRI/URI is a parse error rather
        // than a silently accepted `Type` node.
        assert!(matches!(parse("42bad"), Err(QueryError::TypeExprInvalidName { .. })));
        assert!(matches!(parse("%%%"), Err(QueryError::TypeExprInvalidName { .. })));
    }

    #[test]
    fn error_on_empty_input() {
        let result = parse("");
        assert!(result.is_err());
    }

    #[test]
    fn error_on_whitespace_only() {
        let result = parse("   ");
        assert!(result.is_err());
    }

    #[test]
    fn error_on_unbalanced_open_paren() {
        let result = parse("(A;B");
        assert!(result.is_err());
    }

    #[test]
    fn error_on_unbalanced_close_paren() {
        let result = parse("A;B)");
        assert!(result.is_err());
    }

    #[test]
    fn and_higher_precedence_than_or() {
        // A|B;C should parse as A | (B AND C)
        let expr = parse("A|B;C").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::Or(vec![t("A"), TypeExprRaw::And(vec![t("B"), t("C"),]),]));
    }

    #[test]
    fn multiple_or_terms() {
        let expr = parse("A|B|C").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::Or(vec![t("A"), t("B"), t("C"),]));
    }
}
