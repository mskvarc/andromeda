//! Parser for the NGSI-LD Entity Type Selection Language (clause 4.17).
//!
//! The grammar is flat:
//!
//! ```text
//! EntityTypes  = OrEntityType *(orOp OrEntityType)
//! OrEntityType = "(" EntityType *(";" EntityType) ")" / EntityType
//! orOp         = "|" / ","
//! ```
//!
//! A conjunction exists only inside one pair of parentheses around plain type
//! names, so `Home;Vehicle`, `((Home;Vehicle))` and `(Building|House)` are all
//! outside the language and refused.

use crate::{error::QueryError, type_selection::grammar::TypeExprRaw};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use std::iter;

/// Parses a type selection expression string into a raw AST.
///
/// # Errors
///
/// Returns [`QueryError::TypeExprEmpty`] for blank input,
/// [`QueryError::TypeExprUnexpectedEnd`] and [`QueryError::TypeExprUnbalanced`]
/// for an expression that ends early or leaves a parenthesis open,
/// [`QueryError::TypeExprUnexpected`] for a character the clause 4.17 grammar
/// does not admit at its position, and [`QueryError::TypeExprInvalidName`]
/// when a type-name production yields no valid clause 4.6.2 name.
pub fn parse(input: &str) -> Result<TypeExprRaw, QueryError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(QueryError::TypeExprEmpty);
    }
    if trimmed == "*" {
        return Ok(TypeExprRaw::Wildcard);
    }

    let mut parser = Parser::new(trimmed);
    let expr = parser.parse_entity_types()?;

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

    /// Parses `EntityTypes = OrEntityType *(orOp OrEntityType)`.
    ///
    /// A single operand is returned as itself rather than wrapped in a
    /// one-element `Or`.
    fn parse_entity_types(&mut self) -> Result<TypeExprRaw, QueryError> {
        let first = self.parse_or_entity_type()?;
        if !matches!(self.peek(), Some('|' | ',')) {
            return Ok(first);
        }
        let mut terms = vec![first];
        while matches!(self.peek(), Some('|' | ',')) {
            self.advance();
            terms.push(self.parse_or_entity_type()?);
        }
        Ok(TypeExprRaw::Or(terms))
    }

    /// Parses `OrEntityType = "(" EntityType *(";" EntityType) ")" / EntityType`.
    ///
    /// A parenthesised group with a single name is that name.
    fn parse_or_entity_type(&mut self) -> Result<TypeExprRaw, QueryError> {
        self.skip_whitespace();
        if self.peek() != Some('(') {
            return self.parse_type_name().map(TypeExprRaw::Type);
        }
        self.advance();
        let first = self.parse_type_name()?;
        let mut rest = Vec::new();
        while self.peek() == Some(';') {
            self.advance();
            rest.push(self.parse_type_name()?);
        }
        if self.peek() != Some(')') {
            return Err(QueryError::TypeExprUnbalanced(self.pos));
        }
        self.advance();
        self.skip_whitespace();
        if rest.is_empty() {
            return Ok(TypeExprRaw::Type(first));
        }
        Ok(TypeExprRaw::And(iter::once(first).chain(rest).map(TypeExprRaw::Type).collect()))
    }

    /// Parses one `EntityType`: a clause 4.6.2 name or a URI.
    fn parse_type_name(&mut self) -> Result<EntityTypeName, QueryError> {
        self.skip_whitespace();
        let start = self.pos;
        match self.peek() {
            Some(c) if is_type_char(c) => {}
            Some(c) => return Err(QueryError::TypeExprUnexpected { ch: c, position: start }),
            None => return Err(QueryError::TypeExprUnexpectedEnd),
        }
        while self.peek().is_some_and(is_type_char) {
            self.advance();
        }
        let name = &self.input[start..self.pos];
        self.skip_whitespace();
        name.parse::<EntityTypeName>()
            .map_err(|source| QueryError::TypeExprInvalidName { position: start, source })
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
#[expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::QueryError,
        type_selection::{grammar::TypeExprRaw, parser::parse},
    };

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
    fn parse_parenthesised_conjunction() {
        let expr = parse("(Home;Vehicle)").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::And(vec![t("Home"), t("Vehicle"),]));
    }

    #[test]
    fn parse_mixed_and_or_with_grouping() {
        let expr = parse("(Home;Vehicle)|Motorhome").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::Or(vec![TypeExprRaw::And(vec![t("Home"), t("Vehicle"),]), t("Motorhome"),]));
    }

    #[test]
    fn a_parenthesised_single_name_is_that_name() {
        pretty_assertions::assert_eq!(parse("(Home)").unwrap(), t("Home"));
    }

    #[test]
    fn a_shape_outside_the_clause_4_17_grammar_is_rejected() {
        for raw in [
            "Home;Vehicle",
            "((Home;Vehicle))",
            "(Building|House)",
            "((A;B)|C)",
            "A|B;C",
            "(A;(B|C))",
            "()",
            "A|",
        ] {
            assert!(parse(raw).is_err(), "{raw} must be rejected");
        }
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
        let result = parse("(A;B))");
        assert!(result.is_err());
    }

    #[test]
    fn multiple_or_terms() {
        let expr = parse("A|B|C").unwrap();
        pretty_assertions::assert_eq!(expr, TypeExprRaw::Or(vec![t("A"), t("B"), t("C"),]));
    }
}
