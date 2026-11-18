//! The clause 4.9 `LinkedEntityRelation` production: the axis that crosses a
//! Relationship into the entity it names.
//!
//! ```text
//! LinkedEntityRelation = AttrName %x7B LinkedEntityPath %x7D   ; AttrName{LinkedEntityPath}
//! LinkedEntityPath     = *1(EntityType 1*(%x2C EntityType) %x3A) AttrName %x7B LinkedEntityPath %x7D
//! LinkedEntityPath    =/ ValuePath
//! ```
//!
//! The braces enclose a path and nothing else: clause 4.9 makes the
//! `Attribute` of a Query Term this production, and the Operator and its value
//! follow it, so a comparison is written after the closing brace
//! (`sensor{humidity}==40`, EXAMPLE 13).

use crate::filter::{
    attribute_path::AttributePath,
    expectation::{Expectation, Found},
    filter_error::FilterError,
    parser::{
        lexer::Token,
        token_cursor::{TokenCursor, attribute_name},
        value_path::{check_depth, parse_value_path},
    },
    quoting::Quoting,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// Parse the attribute axis of a Query Term whose leading name `leading` has
/// already been read: a `ValuePath`, or a hop `leading{LinkedEntityPath}`.
///
/// `depth` counts the path segments consumed so far (sub-attributes and
/// linked-entity hops together), so a chain of hops is bounded by the same
/// budget a chain of sub-attributes is.
///
/// # Errors
///
/// Returns [`FilterError::Expected`] when a hop is empty, left unclosed or
/// holds anything after its path (a comparison written inside the braces), and
/// the errors of [`parse_value_path`], the entity-type guard and
/// [`check_depth`].
pub(super) fn parse_linked_entity_relation<'a>(cursor: &mut TokenCursor<'a>, leading: &'a str, depth: usize) -> Result<AttributePath, FilterError> {
    if cursor.peek() != Some(&Token::LBrace) {
        return parse_value_path(cursor, leading, depth).map(AttributePath::Local);
    }
    let relationship = attribute_name(leading)?;
    cursor.advance();
    check_depth(depth + 1)?;
    let guard = parse_hop_guard(cursor)?;
    let inner_leading = match guard.inner_leading {
        Some(text) => text,
        None => cursor.parse_ident()?,
    };
    let inner = parse_linked_entity_relation(cursor, inner_leading, depth + 1)?;
    cursor.expect(&Token::RBrace)?;
    Ok(AttributePath::Linked {
        relationship,
        guard_types: guard.types,
        inner: Box::new(inner),
    })
}

/// What the head of a linked-entity hop contributed before its inner path.
#[derive(Debug)]
struct HopGuard<'a> {
    /// The entity types the hop is constrained to, empty when it carried none.
    types: Vec<AttributeName>,
    /// The inner path's leading name when the tokeniser glued it to the guard's
    /// terminating colon; `None` when the inner path starts at the next token.
    inner_leading: Option<&'a str>,
}

/// Parse the optional entity-type guard at the head of a linked-entity hop.
///
/// Clause 4.9 spells the guard `*1(EntityType 1*("," EntityType) ":")` in its
/// ABNF and writes it with one type in EXAMPLE 14 (`sensor{Device:humidity}`),
/// so a guard names one type or more, then a colon. The tokeniser is
/// context-free and `:` is a legal identifier character, so the colon
/// arrives glued to whatever the client wrote next: `Store:category` is one
/// identifier. The guard is therefore recognised from shape, not from a
/// colon token. A comma directly after the first name inside the braces
/// opens a guard of several types, and nothing else in the grammar puts a
/// comma there. A first name holding a colon opens a guard of one type,
/// written bare (`Device:humidity`) or quoted before a colon
/// (`"https://example.com/Device":humidity`): no `AttrName` and no
/// `EntityType` of the grammar holds a colon, so `X:y` inside the braces
/// derives only as a type and an attribute. A prefixed attribute name there is
/// written quoted (`sensor{"ex:humidity"}`).
///
/// # Errors
///
/// Returns [`FilterError::Expected`] when a type name is missing, when the
/// guard's terminating colon is absent, or when a bare last name carries two
/// colons and so is ambiguous, and [`FilterError::InvalidAttributeName`] when a
/// type name is outside the clause 4.6.2 grammar.
fn parse_hop_guard<'a>(cursor: &mut TokenCursor<'a>) -> Result<HopGuard<'a>, FilterError> {
    if !opens_guard(cursor) {
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

/// Whether the tokens at the head of a hop open a type guard: a name followed
/// by a comma, a bare name holding a colon, or a quoted name followed by a
/// colon.
fn opens_guard(cursor: &TokenCursor<'_>) -> bool {
    match (cursor.peek(), cursor.peek_ahead(1)) {
        (Some(Token::Ident(_) | Token::QuotedStr(_)), Some(Token::Comma)) => true,
        (Some(Token::Ident(name)), _) => name.contains(':'),
        (Some(Token::QuotedStr(_)), Some(Token::Ident(next))) => next.starts_with(':'),
        // Any other head is an inner path, or a token no path opens with,
        // which the inner path's parse reports.
        (Some(_) | None, _) => false,
    }
}

/// Consume one name of a type guard, reporting whether it was written bare.
///
/// The distinction decides how the guard's terminating colon is found: only
/// a bare identifier can have the colon glued to it, because a quoted string
/// ends at its closing quote, and a quoted name is an expanded IRI, whose
/// own colons are part of the IRI rather than the grammar.
fn parse_guard_name<'a>(cursor: &mut TokenCursor<'a>) -> Result<(&'a str, Quoting), FilterError> {
    match cursor.advance() {
        Some(Token::Ident(name)) => Ok((name.as_str(), Quoting::Bare)),
        Some(Token::QuotedStr(name)) => Ok((name.as_str(), Quoting::Quoted)),
        other => Err(FilterError::Expected {
            expected: Expectation::EntityType,
            found: Found::of(other),
        }),
    }
}

/// Split the guard's last type from the inner path that follows its colon,
/// or report that this name is not the last one.
///
/// A comma is checked first: it says another type follows, so this name
/// cannot be carrying the terminator and whatever colons it holds are part
/// of the name: `rel{ex:A,ex:B:attr}` has a prefixed type in each position
/// and only the last one is split.
///
/// For the last name the colon reaches the parser in one of two shapes,
/// both of which the tokeniser produces without any state: glued to the type
/// it terminates (`Store:` or `Store:category`, since `:` continues an
/// identifier), or as its own `:`-prefixed identifier when the type was
/// quoted, which is how an expanded filter round-trips, its IRIs being
/// quoted. An empty remainder means the inner path starts at the next token.
///
/// A bare last name holding *two* colons is ambiguous and is rejected rather
/// than resolved by guesswork: `ex:Store:category` reads equally well as the
/// prefixed type `ex:Store` before the attribute `category` or as the type
/// `ex` before the prefixed attribute `Store:category`, and clause 4.9
/// decides neither: its `EntityType` production admits no colon at all,
/// while real filters do carry prefixed names. Quoting the type says which
/// was meant, which is what the error asks for.
fn split_guard_terminator<'a>(cursor: &mut TokenCursor<'a>, name: &'a str, quoting: Quoting) -> Result<Option<(&'a str, Option<&'a str>)>, FilterError> {
    if cursor.peek() == Some(&Token::Comma) {
        return Ok(None);
    }
    if quoting == Quoting::Bare
        && let Some((last, rest)) = name.split_once(':')
    {
        if rest.contains(':') {
            return Err(FilterError::AmbiguousTypeGuard);
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
    Err(FilterError::Expected {
        expected: Expectation::GuardTerminator,
        found: cursor.found(),
    })
}
