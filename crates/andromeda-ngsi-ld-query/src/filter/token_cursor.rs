//! A position in a tokenised clause 4.9 expression.
//!
//! Every production of the NGSI-LD Query Language is read by advancing over the
//! same token stream, so the cursor that does the advancing is stated once here
//! rather than being a private field of one parser. That is what lets the
//! `ValuePath` production be shared: clause 4.23.3 delegates its `AttrName` to
//! clause 4.9, and an `orderBy` term is read with the same cursor over the same
//! tokens as a `q` predicate.

use crate::{
    error::QueryError,
    filter::{
        expectation::{QExpectation, QFound},
        lexer::{Token, TokenKind},
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// Cursor over a tokenised clause 4.9 expression.
#[derive(Debug)]
pub struct TokenCursor<'a> {
    /// The token stream being consumed.
    tokens: &'a [Token],
    /// Index of the next token to consume.
    position: usize,
}

impl<'a> TokenCursor<'a> {
    /// Start at the beginning of a token stream.
    #[must_use]
    pub const fn new(tokens: &'a [Token]) -> TokenCursor<'a> {
        TokenCursor { tokens, position: 0 }
    }

    /// The next token without consuming it.
    ///
    /// The borrow is of the token stream, not of the cursor, so a caller may
    /// hold the token while it advances past it.
    #[must_use]
    pub fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.position)
    }

    /// The token `offset` positions ahead without consuming anything.
    #[must_use]
    pub fn peek_ahead(&self, offset: usize) -> Option<&'a Token> {
        self.tokens.get(self.position + offset)
    }

    /// Consume and return the next token.
    pub fn advance(&mut self) -> Option<&'a Token> {
        let token = self.tokens.get(self.position);
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    /// Whether every token has been consumed.
    #[must_use]
    pub const fn is_exhausted(&self) -> bool {
        self.position >= self.tokens.len()
    }

    /// How many tokens have been consumed.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.position
    }

    /// Step over the token at the cursor without inspecting it.
    ///
    /// Used where a production has already read the token through
    /// [`TokenCursor::peek_ahead`] and only needs the cursor moved past it.
    pub const fn skip(&mut self) {
        self.position += 1;
    }

    /// Consume the next token, requiring it to equal `expected`.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::QExpected`] when the stream holds something else,
    /// or nothing at all.
    pub fn expect(&mut self, expected: &Token) -> Result<(), QueryError> {
        if self.peek() == Some(expected) {
            self.advance();
            Ok(())
        } else {
            Err(QueryError::QExpected {
                expected: QExpectation::Token(TokenKind::from(expected)),
                found: self.found(),
            })
        }
    }

    /// What stands where a token was required.
    #[must_use]
    pub fn found(&self) -> QFound {
        QFound::of(self.peek())
    }

    /// Consume an identifier, quoted or bare.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::QExpected`] when the next token carries no name.
    pub fn parse_ident(&mut self) -> Result<&'a str, QueryError> {
        match self.advance() {
            Some(Token::Ident(text) | Token::QuotedStr(text)) => Ok(text.as_str()),
            other => Err(QueryError::QExpected {
                expected: QExpectation::Token(TokenKind::Ident),
                found: QFound::of(other),
            }),
        }
    }

    /// Consume an identifier naming an attribute, quoted or bare.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::QExpected`] when the next token carries no name,
    /// and [`QueryError::AttributeNameInvalid`] when the name it carries is
    /// outside the clause 4.6.2 grammar.
    pub fn parse_attribute_name(&mut self) -> Result<AttributeName, QueryError> {
        attribute_name(self.parse_ident()?)
    }
}

/// Read an identifier as an attribute name.
///
/// The clause 4.6.2 name grammar accepts a short name, a prefixed name and an
/// expanded IRI, which is the whole range a filter can carry before and after
/// expansion, so validating here rejects nothing a client may write.
///
/// # Errors
///
/// Returns [`QueryError::AttributeNameInvalid`] when the name is outside that
/// grammar.
pub fn attribute_name(raw: &str) -> Result<AttributeName, QueryError> {
    raw.parse().map_err(QueryError::AttributeNameInvalid)
}
