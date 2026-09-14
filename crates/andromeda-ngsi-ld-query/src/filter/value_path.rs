//! The clause 4.9 `ValuePath` production, shared by every parameter that
//! addresses an attribute by path.
//!
//! Clause 4.9 gives the production as
//!
//! ```text
//! ValuePath  = DottedPath *1(%x5B DottedPath %x5D)   ; DottedPath *1([DottedPath])
//! DottedPath = AttrName *(%x2E AttrName)             ; AttrName *(.AttrName)
//! ```
//!
//! — a dot-separated list of names, optionally followed by one bracketed
//! dot-separated list that navigates into a compound Property value. The prose
//! reads the first list as a walk through "the matching entities subjacent
//! graph" of (sub)Properties and (sub)Relationships, and the bracketed list as
//! a `MemberExpression` evaluated against the seed target value (EXAMPLE 9
//! `address[city]`, EXAMPLE 10 `sensor.rawdata[airquality.particulate]`).
//!
//! Two parameters derive from it. `q` reaches it through `Attribute`, and
//! `orderBy` reaches it because clause 4.23.3 says "See clause 4.9 for the
//! definition of `AttrName`" and settles the shapes with its own EXAMPLE 4
//! (`?orderBy=address[city]`) and EXAMPLE 5 (`?orderBy=name.observedAt`, "an
//! attribute path that is defined by the production rule Attribute, as a
//! dot-separated list of names"). Both therefore parse here, so the two cannot
//! drift into accepting different paths.
//!
//! What is *not* here is the `%x7B…%x7D` `LinkedEntityRelation` axis: it
//! crosses an entity boundary, and clause 4.23.3's grammar is `AttrName`-based
//! with prose naming only the dotted and bracketed paths.

use crate::{
    error::QueryError,
    filter::{
        lexer::{Token, tokenize},
        path::AttrPath,
        token_cursor::{TokenCursor, attribute_name},
    },
};

/// The most segments an attribute path may nest, counting sub-attributes and
/// linked-entity hops alike.
///
/// Far above any legitimate NGSI-LD nesting (an attribute's sub-attributes are
/// rarely more than one deep, and a linked-entity traversal rarely more than two
/// hops), this only rejects a pathological path whose backend rendering would
/// otherwise grow with its depth — a hop compiles to a correlated subquery per
/// level, so `a{a{a{…}}}` is as expensive as it is deep. An `orderBy` path is
/// bounded by the same budget, for the same reason: each segment is another
/// hop inside the jsonpath its lateral evaluates.
pub const MAX_SUB_ATTRIBUTE_DEPTH: usize = 16;

/// Reject a path that nests deeper than the budget.
///
/// # Errors
///
/// Returns [`QueryError::QAttributePathTooDeep`] beyond
/// [`MAX_SUB_ATTRIBUTE_DEPTH`].
pub const fn check_depth(depth: usize) -> Result<(), QueryError> {
    if depth > MAX_SUB_ATTRIBUTE_DEPTH {
        return Err(QueryError::QAttributePathTooDeep {
            depth,
            limit: MAX_SUB_ATTRIBUTE_DEPTH,
        });
    }
    Ok(())
}

/// Parse a `ValuePath` whose leading name has already been read:
/// `('.' ident)* ('[' ident ('.' ident)* ']')?`.
///
/// `depth` counts the segments already consumed by an enclosing axis, so a
/// path reached through linked-entity hops shares one budget with them.
///
/// # Errors
///
/// Returns [`QueryError::AttributeNameInvalid`] when a segment is outside the
/// clause 4.6.2 name grammar, [`QueryError::QExpected`] when a separator is not
/// followed by a name or a bracket is left open, and
/// [`QueryError::QAttributePathTooDeep`] beyond [`MAX_SUB_ATTRIBUTE_DEPTH`].
pub fn parse_value_path(cursor: &mut TokenCursor<'_>, leading: &str, depth: usize) -> Result<AttrPath, QueryError> {
    let attr = attribute_name(leading)?;
    let mut sub_attrs = Vec::new();
    let mut value_path = Vec::new();
    let mut match_all_languages = false;

    while cursor.peek() == Some(&Token::Dot) {
        cursor.advance();
        sub_attrs.push(cursor.parse_attribute_name()?);
        check_depth(depth + sub_attrs.len())?;
    }

    if cursor.peek() == Some(&Token::LBracket) {
        cursor.advance();
        if cursor.peek() == Some(&Token::Star) {
            // `[*]` is the language-suppression marker, not a value-path
            // key: it matches every language of a LanguageProperty.
            cursor.advance();
            match_all_languages = true;
        } else {
            value_path.push(cursor.parse_attribute_name()?);
            while cursor.peek() == Some(&Token::Dot) {
                cursor.advance();
                value_path.push(cursor.parse_attribute_name()?);
            }
            check_depth(depth + sub_attrs.len() + value_path.len())?;
        }
        cursor.expect(&Token::RBracket)?;
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

/// Parse a whole attribute path standing on its own, with no enclosing
/// expression.
///
/// This is the entry clause 4.23.3 uses: an `orderBy` term is one `ValuePath`
/// and nothing else, so the token stream has to be exhausted by it. A `q`
/// filter instead reaches [`parse_value_path`] mid-expression, with an operator
/// and an operand still to come.
///
/// # Errors
///
/// Returns the tokeniser's [`QueryError::QUnterminatedString`] or
/// [`QueryError::QUnexpectedChar`], the [`parse_value_path`] failures, and
/// [`QueryError::QUnexpectedToken`] when anything follows the path — which is
/// what refuses the clause 4.9 `rel{…}` linked-entity axis to a parameter whose
/// own grammar has no hop.
pub fn parse_attribute_path(text: &str) -> Result<AttrPath, QueryError> {
    let tokens = tokenize(text)?;
    let mut cursor = TokenCursor::new(&tokens);
    let leading = cursor.parse_ident()?;
    let path = parse_value_path(&mut cursor, leading, 0)?;
    if !cursor.is_exhausted() {
        return Err(QueryError::QUnexpectedToken {
            position: cursor.position(),
            token: cursor.found_description(),
        });
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        filter::value_path::{MAX_SUB_ATTRIBUTE_DEPTH, parse_attribute_path},
    };
    use pretty_assertions::assert_eq;

    /// The names of a parsed path, as the client spelled them.
    fn spelled(text: &str) -> (String, Vec<String>, Vec<String>) {
        let path = parse_attribute_path(text).expect("a clause 4.9 ValuePath");
        (
            path.attr.as_str().to_owned(),
            path.sub_attrs.iter().map(|name| name.as_str().to_owned()).collect(),
            path.value_path.iter().map(|name| name.as_str().to_owned()).collect(),
        )
    }

    /// `DottedPath = AttrName *(%x2E AttrName)` with no dots at all.
    #[test]
    fn a_bare_name_is_a_path_of_one_segment() {
        let (attr, subs, members) = spelled("name");
        assert_eq!(attr, "name");
        assert_eq!(subs, Vec::<String>::new());
        assert_eq!(members, Vec::<String>::new());
    }

    /// Clause 4.23.3 EXAMPLE 5: `?orderBy=name.observedAt`.
    #[test]
    fn a_dotted_path_walks_the_sub_attribute_axis() {
        let (attr, subs, members) = spelled("name.observedAt");
        assert_eq!(attr, "name");
        assert_eq!(subs, vec!["observedAt".to_owned()]);
        assert_eq!(members, Vec::<String>::new());
    }

    /// Clause 4.23.3 EXAMPLE 4: `?orderBy=address[city]`.
    #[test]
    fn a_bracketed_path_navigates_into_a_compound_value() {
        let (attr, subs, members) = spelled("address[city]");
        assert_eq!(attr, "address");
        assert_eq!(subs, Vec::<String>::new());
        assert_eq!(members, vec!["city".to_owned()]);
    }

    /// Clause 4.9 EXAMPLE 10: `sensor.rawdata[airquality.particulate]` — the
    /// two axes combine, and the bracketed list is itself dot-separated.
    #[test]
    fn the_two_axes_combine_in_one_path() {
        let (attr, subs, members) = spelled("sensor.rawdata[airquality.particulate]");
        assert_eq!(attr, "sensor");
        assert_eq!(subs, vec!["rawdata".to_owned()]);
        assert_eq!(members, vec!["airquality".to_owned(), "particulate".to_owned()]);
    }

    /// `*1(%x5B DottedPath %x5D)` admits at most one bracketed group, and it
    /// comes last: nothing may follow the closing bracket.
    #[test]
    fn a_second_bracketed_group_is_outside_the_production() {
        assert!(parse_attribute_path("a[b][c]").is_err());
        assert!(parse_attribute_path("a[b].c").is_err());
    }

    /// The `%x7B…%x7D` linked-entity axis crosses an entity boundary, which
    /// clause 4.23.3's `AttrName`-based grammar does not reach.
    #[test]
    fn the_linked_entity_axis_is_not_part_of_a_value_path() {
        let error = parse_attribute_path("rel{attr}").expect_err("a hop is not a ValuePath");
        assert!(matches!(error, QueryError::QUnexpectedToken { .. }), "got {error:?}");
    }

    /// A prefixed name is one `AttrName`, so the dot after it still separates
    /// segments rather than being swallowed into the name.
    #[test]
    fn a_prefixed_name_stays_one_segment_of_a_dotted_path() {
        let (attr, subs, members) = spelled("ex:address[city]");
        assert_eq!(attr, "ex:address");
        assert_eq!(subs, Vec::<String>::new());
        assert_eq!(members, vec!["city".to_owned()]);

        let (attr, subs, _) = spelled("ex:a.b");
        assert_eq!(attr, "ex:a");
        assert_eq!(subs, vec!["b".to_owned()]);
    }

    /// An expanded path round-trips through the tokeniser because its IRIs are
    /// quoted; unquoted, a dot inside an IRI would separate segments.
    #[test]
    fn a_quoted_iri_segment_is_one_name() {
        let (attr, subs, _) = spelled("\"https://example.com/a\".\"https://example.com/b\"");
        assert_eq!(attr, "https://example.com/a");
        assert_eq!(subs, vec!["https://example.com/b".to_owned()]);
    }

    #[test]
    fn a_separator_with_no_name_after_it_is_rejected() {
        for malformed in ["a.", "a[", "a[]", ".a", "a[b", "a.b."] {
            assert!(parse_attribute_path(malformed).is_err(), "{malformed}");
        }
    }

    #[test]
    fn a_path_at_the_depth_limit_parses_and_one_beyond_it_does_not() {
        let at_limit = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH));
        assert!(parse_attribute_path(&at_limit).is_ok(), "a path at the limit must parse");

        let too_deep = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH + 1));
        let error = parse_attribute_path(&too_deep).expect_err("beyond the limit");
        assert!(matches!(error, QueryError::QAttributePathTooDeep { .. }), "got {error:?}");
    }

    /// The bracketed members count toward the same budget the dotted segments
    /// spend, because each one is another hop in the rendered jsonpath.
    #[test]
    fn the_depth_limit_counts_compound_members_too() {
        let members = (0..=MAX_SUB_ATTRIBUTE_DEPTH).map(|index| format!("m{index}")).collect::<Vec<_>>().join(".");
        let error = parse_attribute_path(&format!("a[{members}]")).expect_err("beyond the limit");
        assert!(matches!(error, QueryError::QAttributePathTooDeep { .. }), "got {error:?}");
    }
}
