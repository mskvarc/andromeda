//! The clause 4.9 `LinkedEntityRelation` production: the axis that crosses a
//! Relationship into the entity it names.
//!
//! ```text
//! LinkedEntityRelation = AttrName %x7B LinkedEntityPath %x7D   ; AttrName{LinkedEntityPath}
//! LinkedEntityPath     = *1(EntityType 1*(%x2C EntityType) %x3A) AttrName %x7B LinkedEntityPath %x7D
//! LinkedEntityPath    =/ ValuePath
//! ```
//!
//! Only the head of the hop — the optional entity-type guard and how it is told
//! apart from a prefixed attribute name — lives here. What follows the guard is
//! a whole predicate, which the expression parser reads and wraps the hop
//! around.

use crate::{
    error::QueryError,
    filter::{
        lexer::Token,
        quoting::Quoting,
        token_cursor::{TokenCursor, attribute_name, describe},
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// What the head of a linked-entity hop contributed before its inner path.
#[derive(Debug)]
pub struct HopGuard<'a> {
    /// The entity types the hop is constrained to, empty when it carried none.
    pub types: Vec<AttributeName>,
    /// The inner path's leading name when the tokeniser glued it to the guard's
    /// terminating colon; `None` when the inner path starts at the next token.
    pub inner_leading: Option<&'a str>,
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
///
/// # Errors
///
/// Returns [`QueryError::QExpected`] when a type name is missing, when the
/// guard's terminating colon is absent, or when a bare last name carries two
/// colons and so is ambiguous, and [`QueryError::AttributeNameInvalid`] when a
/// type name is outside the clause 4.6.2 grammar.
pub fn parse_hop_guard<'a>(cursor: &mut TokenCursor<'a>) -> Result<HopGuard<'a>, QueryError> {
    let opens_guard = matches!(cursor.peek(), Some(Token::Ident(_) | Token::QuotedStr(_))) && cursor.peek_ahead(1) == Some(&Token::Comma);
    if !opens_guard {
        return Ok(HopGuard {
            types: Vec::new(),
            inner_leading: None,
        });
    }

    let mut types = Vec::new();
    loop {
        let (name, quoting) = parse_guard_name(cursor)?;
        if let Some((last, inner_leading)) = split_guard_terminator(cursor, name, quoting)? {
            types.push(attribute_name(last)?);
            return Ok(HopGuard { types, inner_leading });
        }
        types.push(attribute_name(name)?);
        cursor.expect(&Token::Comma)?;
    }
}

/// Consume one name of a type guard, reporting whether it was written bare.
///
/// The distinction decides how the guard's terminating colon is found: only
/// a bare identifier can have the colon glued to it, because a quoted string
/// ends at its closing quote — and a quoted name is an expanded IRI, whose
/// own colons are part of the IRI rather than the grammar.
fn parse_guard_name<'a>(cursor: &mut TokenCursor<'a>) -> Result<(&'a str, Quoting), QueryError> {
    match cursor.advance() {
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
fn split_guard_terminator<'a>(cursor: &mut TokenCursor<'a>, name: &'a str, quoting: Quoting) -> Result<Option<(&'a str, Option<&'a str>)>, QueryError> {
    if cursor.peek() == Some(&Token::Comma) {
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
    if let Some(Token::Ident(next)) = cursor.peek()
        && let Some(rest) = next.strip_prefix(':')
    {
        cursor.skip();
        let inner_leading = (!rest.is_empty()).then_some(rest);
        return Ok(Some((name, inner_leading)));
    }
    Err(QueryError::QExpected {
        expected: ":".to_string(),
        found: cursor.found_description(),
    })
}
