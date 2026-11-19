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

use crate::{
    filter::{
        complexity::MAX_SUB_ATTRIBUTE_DEPTH,
        expectation::{Expectation, Punctuation},
        filter_error::FilterError,
        operator_spelling::RANGE,
        parser::{
            complete,
            entry_refusal,
            expected,
            word::{opens_bare_word, word},
        },
        target_language::TargetLanguage,
        trailing_group::{MemberPath, TrailingGroup},
        value_path::ValuePath,
    },
    name_stage::Written,
    query_text::{grammar_fault::GrammarFault, whitespace::Whitespace},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use std::str::FromStr;
use winnow::{
    Parser,
    stream::{LocatingSlice, Stream},
};

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

/// Read a name as an attribute name.
///
/// The clause 4.6.2 name grammar accepts a short name, a prefixed name and an
/// expanded IRI, which is the whole range a filter can carry before and after
/// expansion, so validating here rejects nothing a client may write.
///
/// # Errors
///
/// Returns [`FilterError::InvalidAttributeName`] when the name is outside that
/// grammar.
pub(super) fn attribute_name(raw: &str) -> Result<AttributeName, FilterError> {
    raw.parse().map_err(FilterError::InvalidAttributeName)
}

/// Read a name as an entity type name, in the same grammar an attribute name is
/// read in.
///
/// # Errors
///
/// Returns [`FilterError::InvalidAttributeName`] when the name is outside the
/// clause 4.6.2 grammar.
pub(super) fn entity_type_name(raw: &str) -> Result<EntityTypeName, FilterError> {
    raw.parse().map_err(FilterError::InvalidAttributeName)
}

/// Reads a `ValuePath` whose leading name has already been read: `('.' name)* ('[' trailing ']')?`.
///
/// `depth` counts the segments already consumed by an enclosing attribute path, so a path reached through linked-entity hops shares one budget with them. The JSON whitespace between any two of its marks is skipped.
///
/// # Errors
///
/// Refuses with [`FilterError::InvalidAttributeName`] when a segment is outside the clause 4.6.2 name grammar, [`FilterError::InvalidTrailingMember`] when a one-member group is neither a name nor a language tag or range, [`FilterError::Expected`] when a separator is not followed by a name or a bracket is left open, and [`FilterError::PathTooDeep`] beyond [`MAX_SUB_ATTRIBUTE_DEPTH`].
pub(super) fn value_path(input: &mut LocatingSlice<&str>, leading: &str, depth: usize) -> Result<ValuePath<Written>, GrammarFault<FilterError>> {
    let attribute = attribute_name(leading).map_err(GrammarFault::Refused)?;
    let mut sub_attributes = Vec::new();
    while let Some(name) = dotted_name(input)? {
        sub_attributes.push(name);
        check_depth(depth + sub_attributes.len()).map_err(GrammarFault::Refused)?;
    }
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    let trailing = if input.peek_token() == Some('[') {
        let _opening = input.next_token();
        let group = trailing_group(input, depth + sub_attributes.len())?;
        expected(Expectation::Punctuation(Punctuation::CloseBracket), ']').parse_next(input)?;
        Some(group)
    } else {
        None
    };
    Ok(ValuePath {
        attribute,
        sub_attributes,
        trailing,
    })
}

/// Reads a whole value path standing on its own: the path, then the end of the text.
fn standalone_path(input: &mut LocatingSlice<&str>) -> Result<ValuePath<Written>, GrammarFault<FilterError>> {
    let leading = expected(Expectation::AttributeName, word).parse_next(input)?;
    let path = value_path(input, leading.text, 0)?;
    complete(input)?;
    Ok(path)
}

/// Reads the next `'.' name` of a dotted list, or `None` when no sub-attribute separator stands.
///
/// A `.` that a second `.` follows is the range separator and ends the list.
fn dotted_name(input: &mut LocatingSlice<&str>) -> Result<Option<AttributeName>, GrammarFault<FilterError>> {
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    let text = input.peek_finish();
    if !text.starts_with('.') || text.starts_with(RANGE) {
        return Ok(None);
    }
    let _separator = input.next_token();
    let name = expected(Expectation::AttributeName, word).parse_next(input)?;
    attribute_name(name.text).map(Some).map_err(GrammarFault::Refused)
}

/// Reads the inside of a trailing `[...]` group.
///
/// `[*]` is the every-language marker. Otherwise the group is the clause 4.9 `DottedPath` of members, unless its one name is outside the clause 4.6.2 name grammar: Annex C.5.13.2 writes a target language there (`marque[en-GB]`), which only an RFC 5646 tag or an RFC 4647 range can spell. Every reading parses; which ones a request may carry is the `q_target_languages` level's to say.
fn trailing_group(input: &mut LocatingSlice<&str>, depth: usize) -> Result<TrailingGroup, GrammarFault<FilterError>> {
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    let text = input.peek_finish();
    if text.starts_with('*') && !opens_bare_word(text) {
        let _marker = input.next_token();
        return Ok(TrailingGroup::AllLanguages);
    }
    let first = expected(Expectation::AttributeName, word).parse_next(input)?;
    let member = match attribute_name(first.text) {
        Ok(member) => member,
        Err(name_error) => {
            Whitespace::Json.skip::<FilterError>().parse_next(input)?;
            if input.peek_token() == Some(']') {
                return first.text.parse::<TargetLanguage>().map(TrailingGroup::Language).map_err(GrammarFault::Refused);
            }
            return Err(GrammarFault::Refused(name_error));
        }
    };
    let mut members = vec![member];
    while let Some(name) = dotted_name(input)? {
        members.push(name);
    }
    check_depth(depth + members.len()).map_err(GrammarFault::Refused)?;
    Ok(TrailingGroup::Members(MemberPath::new(members)))
}

impl FromStr for ValuePath<Written> {
    type Err = FilterError;

    /// Parses a whole value path standing on its own, with no enclosing
    /// filter.
    ///
    /// This is the entry clause 4.23.3 uses: an `orderBy` term is one `ValuePath`
    /// and nothing else, so the text has to end with it. A `q` filter instead
    /// reads a path with an operator and an operand still to come.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::InvalidAttributeName`],
    /// [`FilterError::InvalidTrailingMember`], [`FilterError::Expected`],
    /// [`FilterError::UnterminatedString`] and [`FilterError::PathTooDeep`] for a
    /// path outside the production, and [`FilterError::UnexpectedCharacter`] when
    /// anything follows the path, which is what refuses the clause 4.9
    /// `rel{...}` linked-entity axis to a parameter whose own grammar has no hop.
    fn from_str(text: &str) -> Result<ValuePath<Written>, FilterError> {
        let mut input = LocatingSlice::new(text);
        standalone_path(&mut input).map_err(|fault| entry_refusal(text, fault))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            complexity::MAX_SUB_ATTRIBUTE_DEPTH,
            expectation::{Expectation, Found, Punctuation},
            filter_error::FilterError,
            target_language::{LanguageSelector, TargetLanguage},
            trailing_group::TrailingGroup,
            value_path::ValuePath,
        },
        name_stage::Written,
    };
    use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
    use pretty_assertions::assert_eq;

    /// The names of a parsed path, as the client spelled them.
    fn spelled(text: &str) -> (String, Vec<String>, Vec<String>) {
        let path = text.parse::<ValuePath<Written>>().expect("a clause 4.9 ValuePath");
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
        assert!("a[b][c]".parse::<ValuePath<Written>>().is_err());
        assert!("a[b].c".parse::<ValuePath<Written>>().is_err());
    }

    /// The `%x7B…%x7D` linked-entity axis crosses an entity boundary, which
    /// clause 4.23.3's `AttrName`-based grammar does not reach.
    #[test]
    fn the_linked_entity_axis_is_not_part_of_a_value_path() {
        let error = "rel{attr}".parse::<ValuePath<Written>>().expect_err("a hop is not a ValuePath");
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: '{', position: 3 }),
            "got {error:?}"
        );
    }

    /// The refusal of a text that is no value path.
    fn refusal(text: &str) -> FilterError {
        text.parse::<ValuePath<Written>>().expect_err("outside the ValuePath production")
    }

    /// The name is read and validated where it stands, so it is refused before a later quotation mark that is never closed.
    #[test]
    fn an_invalid_name_is_refused_before_a_later_unterminated_quotation_mark() {
        let error = refusal("a-\"");
        assert!(
            matches!(error, FilterError::InvalidAttributeName(NgsiLdReferenceError::Invalid)),
            "got {error:?}"
        );
        for text in ["\"\"-", "\"\"\""] {
            let error = refusal(text);
            assert!(
                matches!(error, FilterError::InvalidAttributeName(NgsiLdReferenceError::Empty)),
                "{text}: got {error:?}"
            );
        }
    }

    /// A character that opens no name is expected as a name where it stands, not reported after a later fault.
    #[test]
    fn a_character_no_name_opens_with_is_expected_as_a_name_where_it_stands() {
        for (text, character) in [("<==1", '<'), (".-", '.')] {
            let error = refusal(text);
            assert!(
                matches!(error, FilterError::Expected { expected: Expectation::AttributeName, found: Found::Character(found), position: 0 } if found == character),
                "{text}: got {error:?}"
            );
        }
    }

    /// Inside a trailing group, a character that continues no member is refused where the group required a closing bracket or a name.
    #[test]
    fn a_character_inside_a_trailing_group_is_refused_where_the_group_required_something_else() {
        let error = refusal("name[*+]");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Punctuation(Punctuation::CloseBracket),
                    found: Found::Character('+'),
                    position: 6
                }
            ),
            "got {error:?}"
        );
        let error = refusal("name[-*]");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::AttributeName,
                    found: Found::Character('-'),
                    position: 5
                }
            ),
            "got {error:?}"
        );
        let error = refusal("name[en-G+B]");
        assert!(
            matches!(error, FilterError::InvalidAttributeName(NgsiLdReferenceError::Invalid)),
            "got {error:?}"
        );
    }

    /// Input after a complete path is refused at its first character, before any later lexical fault.
    #[test]
    fn input_after_a_complete_path_is_refused_at_its_first_character_before_a_later_lexical_fault() {
        for (text, character, position) in [("ex:q\"", '"', 4), ("a]-", ']', 1), ("a..b@", '.', 1)] {
            let error = refusal(text);
            assert!(
                matches!(error, FilterError::UnexpectedCharacter { character: found, position: at } if found == character && at == position),
                "{text}: got {error:?}"
            );
        }
    }

    /// A path standing alone reads its names, sub-attributes and trailing group from the text, and refuses what follows.
    #[test]
    fn a_standalone_value_path_parses_from_its_text() {
        let path = "address[city]".parse::<ValuePath<Written>>().expect("a path with a trailing group");
        assert_eq!(path.attribute.as_str(), "address");
        assert_eq!(path.sub_attributes.len(), 0);
        assert_eq!(
            path.members()
                .map(|members| members.addressed.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>()),
            Some(vec!["city".to_owned()])
        );

        let path = "a.b.c".parse::<ValuePath<Written>>().expect("a dotted path");
        assert_eq!(path.attribute.as_str(), "a");
        assert_eq!(
            path.sub_attributes.iter().map(|name| name.as_str().to_owned()).collect::<Vec<_>>(),
            vec!["b".to_owned(), "c".to_owned()]
        );
        assert!(path.trailing.is_none());

        let error = "a[b].c".parse::<ValuePath<Written>>().expect_err("nothing follows the trailing group");
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: '.', position: 4 }),
            "got {error:?}"
        );
    }

    /// JSON whitespace stands between any two marks of the path and before its end.
    #[test]
    fn json_whitespace_separates_the_marks_of_a_path() {
        let path = " a . b [ c . d ] \t\n".parse::<ValuePath<Written>>().expect("whitespace is skipped");
        assert_eq!(path.attribute.as_str(), "a");
        assert_eq!(path.sub_attributes.len(), 1);
        assert_eq!(path.members().map(|members| members.addressed.len()), Some(2));
    }

    /// A name that no word opens, or a separator with nothing after it, is refused where the name was required.
    #[test]
    fn a_missing_name_is_refused_where_it_was_required() {
        for (text, found, position) in [
            ("", Found::EndOfInput, 0),
            ("@a", Found::Character('@'), 0),
            ("a.", Found::EndOfInput, 2),
            ("a. 1", Found::Character('1'), 3),
            ("a[", Found::EndOfInput, 2),
            ("a[]", Found::Character(']'), 2),
            ("true", Found::Character('t'), 0),
        ] {
            let error = text.parse::<ValuePath<Written>>().expect_err("a name is required");
            assert!(
                matches!(error, FilterError::Expected { expected: Expectation::AttributeName, found: refused, position: at } if refused == found && at == position),
                "{text}: got {error:?}"
            );
        }
        let error = "a[b".parse::<ValuePath<Written>>().expect_err("the bracket is left open");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Punctuation(Punctuation::CloseBracket),
                    found: Found::EndOfInput,
                    position: 3
                }
            ),
            "got {error:?}"
        );
    }

    /// A `*` that touches a hyphen belongs to a language range; a lone one is the every-language marker, and a range separator ends the path.
    #[test]
    fn the_star_and_the_range_separator_are_read_by_what_touches_them() {
        assert_eq!(
            "a[ * ]".parse::<ValuePath<Written>>().expect("the marker").trailing,
            Some(TrailingGroup::AllLanguages)
        );
        assert!(matches!(
            "a[*-GB]".parse::<ValuePath<Written>>().expect("a range").trailing,
            Some(TrailingGroup::Language(_))
        ));
        let error = "a..b"
            .parse::<ValuePath<Written>>()
            .expect_err("a range separator is no sub-attribute separator");
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: '.', position: 1 }),
            "got {error:?}"
        );
        let error = "a[*x]".parse::<ValuePath<Written>>().expect_err("the marker stands alone");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Punctuation(Punctuation::CloseBracket),
                    found: Found::Character('x'),
                    position: 3
                }
            ),
            "got {error:?}"
        );
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

    /// An expanded path round-trips through the parser because its IRIs are
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
            assert!(malformed.parse::<ValuePath<Written>>().is_err(), "{malformed}");
        }
    }

    #[test]
    fn a_path_at_the_depth_limit_parses_and_one_beyond_it_does_not() {
        let at_limit = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH));
        assert!(at_limit.parse::<ValuePath<Written>>().is_ok(), "a path at the limit must parse");

        let too_deep = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH + 1));
        let error = too_deep.parse::<ValuePath<Written>>().expect_err("beyond the limit");
        assert!(matches!(error, FilterError::PathTooDeep { .. }), "got {error:?}");
    }

    /// The bracketed members count toward the same budget the dotted segments
    /// spend, because each one is another hop in the rendered jsonpath.
    #[test]
    fn the_depth_limit_counts_compound_members_too() {
        let members = (0..=MAX_SUB_ATTRIBUTE_DEPTH).map(|index| format!("m{index}")).collect::<Vec<_>>().join(".");
        let error = format!("a[{members}]").parse::<ValuePath<Written>>().expect_err("beyond the limit");
        assert!(matches!(error, FilterError::PathTooDeep { .. }), "got {error:?}");
    }

    /// Annex C.5.13.2: `marque[en-GB]` names a target language no `AttrName`
    /// spells.
    #[test]
    fn a_hyphenated_language_tag_is_a_target_language() {
        let path = "label[en-GB]".parse::<ValuePath<Written>>().expect("a target language");
        assert!(matches!(path.trailing, Some(TrailingGroup::Language(TargetLanguage::Tag(_)))), "{path:?}");
        pretty_assertions::assert_eq!(path.target_language().map(LanguageSelector::as_str), Some("en-GB"));
    }

    /// Clause 4.9 `color[en]`: a one-member path that is also a tag keeps its
    /// member reading and offers the language reading beside it.
    #[test]
    fn a_name_that_is_also_a_tag_stays_a_member_path_with_a_target_language() {
        let path = "label[en]".parse::<ValuePath<Written>>().expect("a member path");
        assert!(matches!(path.trailing, Some(TrailingGroup::Members(_))), "{path:?}");
        pretty_assertions::assert_eq!(path.target_language().map(LanguageSelector::as_str), Some("en"));
    }

    #[test]
    fn a_multi_member_or_non_tag_path_names_no_target_language() {
        for text in ["label[city_name]", "label[airquality.particulate]"] {
            let path = text.parse::<ValuePath<Written>>().expect("a member path");
            assert!(matches!(path.trailing, Some(TrailingGroup::Members(_))), "{text}");
            assert!(path.target_language().is_none(), "{text}");
        }
    }

    #[test]
    fn the_star_marker_selects_every_language() {
        let path = "label[*]".parse::<ValuePath<Written>>().expect("the marker");
        assert_eq!(path.trailing, Some(TrailingGroup::AllLanguages));
        assert!(path.target_language().is_none());
    }

    #[test]
    fn a_wildcard_range_is_a_target_language() {
        let path = "label[de-*-DE]".parse::<ValuePath<Written>>().expect("a range");
        assert!(matches!(path.trailing, Some(TrailingGroup::Language(TargetLanguage::Range(_)))), "{path:?}");
        assert!(path.target_language().is_some_and(LanguageSelector::is_range));
    }

    #[test]
    fn a_member_neither_name_nor_tag_is_refused() {
        let error = "label[en_GB-x]".parse::<ValuePath<Written>>().expect_err("neither");
        assert!(matches!(error, FilterError::InvalidTrailingMember { .. }), "got {error:?}");
        let error = "label[en-GB.x]".parse::<ValuePath<Written>>().expect_err("a tag starts no member path");
        assert!(matches!(error, FilterError::InvalidAttributeName(_)), "got {error:?}");
    }
}
