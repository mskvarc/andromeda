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
//! `ValuePath` is a dot-separated list of names, optionally followed by one bracketed
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

use crate::filter::{
    complexity::MAX_SUB_ATTRIBUTE_DEPTH,
    filter_error::FilterError,
    parser::{
        lexer::{Token, tokenize},
        token_cursor::{TokenCursor, attribute_name},
    },
    target_language::TargetLanguage,
    trailing_group::{MemberPath, TrailingGroup},
    value_path::ValuePath,
};
use std::str::FromStr;

/// Reject a path that nests deeper than the budget.
///
/// # Errors
///
/// Returns [`FilterError::PathTooDeep`] beyond
/// [`MAX_SUB_ATTRIBUTE_DEPTH`].
pub(super) const fn check_depth(depth: usize) -> Result<(), FilterError> {
    if depth > MAX_SUB_ATTRIBUTE_DEPTH {
        return Err(FilterError::PathTooDeep {
            depth,
            limit: MAX_SUB_ATTRIBUTE_DEPTH,
        });
    }
    Ok(())
}

/// Parse a `ValuePath` whose leading name has already been read:
/// `('.' ident)* ('[' trailing ']')?`.
///
/// `depth` counts the segments already consumed by an enclosing attribute path, so a
/// path reached through linked-entity hops shares one budget with them.
///
/// # Errors
///
/// Returns [`FilterError::InvalidAttributeName`] when a segment is outside the
/// clause 4.6.2 name grammar, [`FilterError::InvalidTrailingMember`] when a
/// one-member group is neither a name nor a language tag or range,
/// [`FilterError::Expected`] when a separator is not followed by a name or a
/// bracket is left open, and [`FilterError::PathTooDeep`] beyond
/// [`MAX_SUB_ATTRIBUTE_DEPTH`].
pub(super) fn parse_value_path(cursor: &mut TokenCursor<'_>, leading: &str, depth: usize) -> Result<ValuePath, FilterError> {
    let attribute = attribute_name(leading)?;
    let mut sub_attributes = Vec::new();

    while cursor.peek() == Some(&Token::Dot) {
        cursor.advance();
        sub_attributes.push(cursor.parse_attribute_name()?);
        check_depth(depth + sub_attributes.len())?;
    }

    let trailing_group = if cursor.peek() == Some(&Token::LBracket) {
        cursor.advance();
        let group = parse_trailing_group(cursor, depth + sub_attributes.len())?;
        cursor.expect(&Token::RBracket)?;
        Some(group)
    } else {
        None
    };

    Ok(ValuePath {
        attribute,
        sub_attributes,
        trailing: trailing_group,
    })
}

/// Parse the inside of a trailing `[…]` group.
///
/// `[*]` is the every-language marker. Otherwise the group is the clause 4.9
/// `DottedPath` of members, unless its one name is outside the clause 4.6.2
/// name grammar: Annex C.5.13.2 writes a target language there
/// (`marque[en-GB]`), which only an RFC 5646 tag or an RFC 4647 range can spell.
/// Every reading parses; which ones a request may carry is the
/// `q_target_languages` level's to say.
fn parse_trailing_group(cursor: &mut TokenCursor<'_>, depth: usize) -> Result<TrailingGroup, FilterError> {
    if cursor.peek() == Some(&Token::Star) {
        cursor.advance();
        return Ok(TrailingGroup::AllLanguages);
    }
    let first = cursor.parse_ident()?;
    let member = match attribute_name(first) {
        Ok(member) => member,
        Err(_) if cursor.peek() == Some(&Token::RBracket) => return first.parse::<TargetLanguage>().map(TrailingGroup::Language),
        Err(name_error) => return Err(name_error),
    };
    let mut members = vec![member];
    while cursor.peek() == Some(&Token::Dot) {
        cursor.advance();
        members.push(cursor.parse_attribute_name()?);
    }
    check_depth(depth + members.len())?;
    Ok(TrailingGroup::Members(MemberPath::new(members)))
}

impl FromStr for ValuePath {
    type Err = FilterError;

    /// Parses a whole value path standing on its own, with no enclosing
    /// filter.
    ///
    /// This is the entry clause 4.23.3 uses: an `orderBy` term is one `ValuePath`
    /// and nothing else, so the token stream has to be exhausted by it. A `q`
    /// filter instead reads a path with an operator and an operand still to
    /// come.
    ///
    /// # Errors
    ///
    /// Returns the tokeniser's [`FilterError::UnterminatedString`] or
    /// [`FilterError::UnexpectedCharacter`], [`FilterError::InvalidAttributeName`],
    /// [`FilterError::InvalidTrailingMember`], [`FilterError::Expected`] and
    /// [`FilterError::PathTooDeep`] for a path outside the production, and
    /// [`FilterError::UnexpectedToken`] when anything follows the path, which is
    /// what refuses the clause 4.9 `rel{…}` linked-entity axis to a parameter whose
    /// own grammar has no hop.
    fn from_str(text: &str) -> Result<ValuePath, FilterError> {
        let tokens = tokenize(text)?;
        let mut cursor = TokenCursor::new(&tokens);
        let leading = cursor.parse_ident()?;
        let path = parse_value_path(&mut cursor, leading, 0)?;
        if !cursor.is_exhausted() {
            return Err(FilterError::UnexpectedToken {
                position: cursor.position(),
                token: cursor.found(),
            });
        }
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        complexity::MAX_SUB_ATTRIBUTE_DEPTH,
        filter_error::FilterError,
        target_language::{LanguageSelector, TargetLanguage},
        trailing_group::TrailingGroup,
        value_path::ValuePath,
    };
    use pretty_assertions::assert_eq;

    /// The names of a parsed path, as the client spelled them.
    fn spelled(text: &str) -> (String, Vec<String>, Vec<String>) {
        let path = text.parse::<ValuePath>().expect("a clause 4.9 ValuePath");
        (
            path.attribute.as_str().to_owned(),
            path.sub_attributes.iter().map(|name| name.as_str().to_owned()).collect(),
            path.members()
                .map_or_default(|members| members.addressed.iter().map(|name| name.as_str().to_owned()).collect()),
        )
    }

    /// `DottedPath = AttrName *(%x2E AttrName)` with no dots at all.
    #[test]
    fn a_bare_name_is_a_path_of_one_segment() {
        let (attribute, sub_attributes, members) = spelled("name");
        assert_eq!(attribute, "name");
        assert_eq!(sub_attributes, Vec::<String>::new());
        assert_eq!(members, Vec::<String>::new());
    }

    /// Clause 4.23.3 EXAMPLE 5: `?orderBy=name.observedAt`.
    #[test]
    fn a_dotted_path_walks_the_sub_attribute_axis() {
        let (attribute, sub_attributes, members) = spelled("name.observedAt");
        assert_eq!(attribute, "name");
        assert_eq!(sub_attributes, vec!["observedAt".to_owned()]);
        assert_eq!(members, Vec::<String>::new());
    }

    /// Clause 4.23.3 EXAMPLE 4: `?orderBy=address[city]`.
    #[test]
    fn a_bracketed_path_navigates_into_a_compound_value() {
        let (attribute, sub_attributes, members) = spelled("address[city]");
        assert_eq!(attribute, "address");
        assert_eq!(sub_attributes, Vec::<String>::new());
        assert_eq!(members, vec!["city".to_owned()]);
    }

    /// Clause 4.9 EXAMPLE 10: `sensor.rawdata[airquality.particulate]`: the
    /// two axes combine, and the bracketed list is itself dot-separated.
    #[test]
    fn the_two_axes_combine_in_one_path() {
        let (attribute, sub_attributes, members) = spelled("sensor.rawdata[airquality.particulate]");
        assert_eq!(attribute, "sensor");
        assert_eq!(sub_attributes, vec!["rawdata".to_owned()]);
        assert_eq!(members, vec!["airquality".to_owned(), "particulate".to_owned()]);
    }

    /// `*1(%x5B DottedPath %x5D)` admits at most one bracketed group, and it
    /// comes last: nothing may follow the closing bracket.
    #[test]
    fn a_second_bracketed_group_is_outside_the_production() {
        assert!("a[b][c]".parse::<ValuePath>().is_err());
        assert!("a[b].c".parse::<ValuePath>().is_err());
    }

    /// The `%x7B…%x7D` linked-entity axis crosses an entity boundary, which
    /// clause 4.23.3's `AttrName`-based grammar does not reach.
    #[test]
    fn the_linked_entity_axis_is_not_part_of_a_value_path() {
        let error = "rel{attr}".parse::<ValuePath>().expect_err("a hop is not a ValuePath");
        assert!(matches!(error, FilterError::UnexpectedToken { .. }), "got {error:?}");
    }

    /// A prefixed name is one `AttrName`, so the dot after it still separates
    /// segments rather than being swallowed into the name.
    #[test]
    fn a_prefixed_name_stays_one_segment_of_a_dotted_path() {
        let (attribute, sub_attributes, members) = spelled("ex:address[city]");
        assert_eq!(attribute, "ex:address");
        assert_eq!(sub_attributes, Vec::<String>::new());
        assert_eq!(members, vec!["city".to_owned()]);

        let (attribute, sub_attributes, _) = spelled("ex:a.b");
        assert_eq!(attribute, "ex:a");
        assert_eq!(sub_attributes, vec!["b".to_owned()]);
    }

    /// An expanded path round-trips through the tokeniser because its IRIs are
    /// quoted; unquoted, a dot inside an IRI would separate segments.
    #[test]
    fn a_quoted_iri_segment_is_one_name() {
        let (attribute, sub_attributes, _) = spelled("\"https://example.com/a\".\"https://example.com/b\"");
        assert_eq!(attribute, "https://example.com/a");
        assert_eq!(sub_attributes, vec!["https://example.com/b".to_owned()]);
    }

    #[test]
    fn a_separator_with_no_name_after_it_is_rejected() {
        for malformed in ["a.", "a[", "a[]", ".a", "a[b", "a.b."] {
            assert!(malformed.parse::<ValuePath>().is_err(), "{malformed}");
        }
    }

    #[test]
    fn a_path_at_the_depth_limit_parses_and_one_beyond_it_does_not() {
        let at_limit = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH));
        assert!(at_limit.parse::<ValuePath>().is_ok(), "a path at the limit must parse");

        let too_deep = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH + 1));
        let error = too_deep.parse::<ValuePath>().expect_err("beyond the limit");
        assert!(matches!(error, FilterError::PathTooDeep { .. }), "got {error:?}");
    }

    /// The bracketed members count toward the same budget the dotted segments
    /// spend, because each one is another hop in the rendered jsonpath.
    #[test]
    fn the_depth_limit_counts_compound_members_too() {
        let members = (0..=MAX_SUB_ATTRIBUTE_DEPTH).map(|index| format!("m{index}")).collect::<Vec<_>>().join(".");
        let error = format!("a[{members}]").parse::<ValuePath>().expect_err("beyond the limit");
        assert!(matches!(error, FilterError::PathTooDeep { .. }), "got {error:?}");
    }

    /// Annex C.5.13.2: `marque[en-GB]` names a target language no `AttrName`
    /// spells.
    #[test]
    fn a_hyphenated_language_tag_is_a_target_language() {
        let path = "label[en-GB]".parse::<ValuePath>().expect("a target language");
        assert!(matches!(path.trailing, Some(TrailingGroup::Language(TargetLanguage::Tag(_)))), "{path:?}");
        pretty_assertions::assert_eq!(path.target_language().map(LanguageSelector::as_str), Some("en-GB"));
    }

    /// Clause 4.9 `color[en]`: a one-member path that is also a tag keeps its
    /// member reading and offers the language reading beside it.
    #[test]
    fn a_name_that_is_also_a_tag_stays_a_member_path_with_a_target_language() {
        let path = "label[en]".parse::<ValuePath>().expect("a member path");
        assert!(matches!(path.trailing, Some(TrailingGroup::Members(_))), "{path:?}");
        pretty_assertions::assert_eq!(path.target_language().map(LanguageSelector::as_str), Some("en"));
    }

    #[test]
    fn a_multi_member_or_non_tag_path_names_no_target_language() {
        for text in ["label[city_name]", "label[airquality.particulate]"] {
            let path = text.parse::<ValuePath>().expect("a member path");
            assert!(matches!(path.trailing, Some(TrailingGroup::Members(_))), "{text}");
            assert!(path.target_language().is_none(), "{text}");
        }
    }

    #[test]
    fn the_star_marker_selects_every_language() {
        let path = "label[*]".parse::<ValuePath>().expect("the marker");
        assert_eq!(path.trailing, Some(TrailingGroup::AllLanguages));
        assert!(path.target_language().is_none());
    }

    #[test]
    fn a_wildcard_range_is_a_target_language() {
        let path = "label[de-*-DE]".parse::<ValuePath>().expect("a range");
        assert!(matches!(path.trailing, Some(TrailingGroup::Language(TargetLanguage::Range(_)))), "{path:?}");
        assert!(path.target_language().is_some_and(LanguageSelector::is_range));
    }

    #[test]
    fn a_member_neither_name_nor_tag_is_refused() {
        let error = "label[en_GB-x]".parse::<ValuePath>().expect_err("neither");
        assert!(matches!(error, FilterError::InvalidTrailingMember { .. }), "got {error:?}");
        let error = "label[en-GB.x]".parse::<ValuePath>().expect_err("a tag starts no member path");
        assert!(matches!(error, FilterError::InvalidAttributeName(_)), "got {error:?}");
    }
}
