//! The clause 4.9 `LinkedEntityRelation` production: the axis that crosses a
//! Relationship into the entity it names.
//!
//! The production as clause 4.9 prints it asks for two types or more in a
//! hop's type list and puts the list only before a further hop, while its
//! EXAMPLE 14 writes one type before the compared attribute
//! (`sensor{Device:humidity}==40`). The parser reads it with both limits lifted:
//!
//! ```text
//! LinkedEntityRelation = AttrName %x7B LinkedEntityPath %x7D   ; AttrName{LinkedEntityPath}
//! LinkedEntityPath     = *1(EntityType *(%x2C EntityType) %x3A)
//!                        (AttrName %x7B LinkedEntityPath %x7D / ValuePath)
//! ```
//!
//! The braces enclose a path and nothing else: clause 4.9 makes the
//! `Attribute` of a Query Term this production, and the Operator and its value
//! follow it, so a comparison is written after the closing brace
//! (`sensor{humidity}==40`, EXAMPLE 13).

use crate::{
    filter::{
        attribute_path::AttributePath,
        expectation::{Expectation, Found, Punctuation},
        filter_error::FilterError,
        parser::{
            expected,
            name::{list_name, path_name},
            value_path::{attribute_name, check_depth, entity_type_name, value_path},
        },
    },
    name_stage::Written,
    query_text::{grammar_fault::GrammarFault, whitespace::Whitespace},
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use std::borrow::Cow;
use winnow::{
    Parser,
    stream::{LocatingSlice, Location, Stream},
};

/// Reads the attribute path of a Query Term whose leading name `leading` has already been read: a `ValuePath`, or a hop `leading{LinkedEntityPath}`.
///
/// `depth` counts the path segments consumed so far (sub-attributes and linked-entity hops together), so a chain of hops is bounded by the same budget a chain of sub-attributes is. The JSON whitespace between any two marks is skipped.
///
/// # Errors
///
/// Refuses with [`FilterError::Expected`] when a hop is empty, left unclosed or holds anything after its path (a comparison written inside the braces), and with the refusals of [`value_path`], of the hop's head and of [`check_depth`].
pub(super) fn attribute_path(input: &mut LocatingSlice<&str>, leading: &str, depth: usize) -> Result<AttributePath<Written>, GrammarFault<FilterError>> {
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    if input.peek_token() != Some('{') {
        return value_path(input, leading, depth).map(AttributePath::Local);
    }
    let relationship = attribute_name(leading).map_err(GrammarFault::Refused)?;
    let _opening = input.next_token();
    check_depth(depth + 1).map_err(GrammarFault::Refused)?;
    let (guard_types, inner_name) = hop_head(input)?;
    let inner = attribute_path(input, &inner_name, depth + 1)?;
    expected(Expectation::Punctuation(Punctuation::CloseBrace), '}').parse_next(input)?;
    Ok(AttributePath::Linked {
        relationship,
        guard_types,
        inner: Box::new(inner),
    })
}

/// Reads the head of a hop in one pass: its type list, empty when it holds none, and the leading name of its inner path.
///
/// The head's first name is read, then the JSON whitespace after it, and the next mark decides. A comma or a colon makes the name the first type of a list, which continues with names separated by commas and ends at the colon. Anything else makes it the leading name of the inner path. No unquoted name holds a colon, so an unquoted colon at the head of a hop always ends the list. Each type is validated before the next is read.
///
/// # Errors
///
/// Refuses with [`FilterError::Expected`] when the head holds no name, a comma has no type after it, the list's colon is absent or no name follows it, with [`FilterError::InvalidAttributeName`] when a type is outside the clause 4.6.2 grammar, and with [`FilterError::UnquotedColonName`] when the inner path's leading name is unquoted and directly followed by a colon.
fn hop_head<'i>(input: &mut LocatingSlice<&'i str>) -> Result<(Vec<EntityTypeName>, Cow<'i, str>), GrammarFault<FilterError>> {
    let first = expected(Expectation::AttributeName, list_name).parse_next(input)?;
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    if !matches!(input.peek_token(), Some(',' | ':')) {
        return Ok((Vec::new(), first));
    }
    let mut types = vec![entity_type_name(&first).map_err(GrammarFault::Refused)?];
    while input.peek_token() == Some(',') {
        let _separator = input.next_token();
        let next = expected(Expectation::EntityType, list_name).parse_next(input)?;
        types.push(entity_type_name(&next).map_err(GrammarFault::Refused)?);
        Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    }
    if input.peek_token() != Some(':') {
        return Err(GrammarFault::Refused(FilterError::Expected {
            expected: Expectation::GuardTerminator,
            found: Found::of(input.peek_token()),
            position: input.current_token_start(),
        }));
    }
    let _terminator = input.next_token();
    let inner_name = expected(Expectation::AttributeName, path_name).parse_next(input)?;
    Ok((types, inner_name))
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            attribute_path::AttributePath,
            complexity::MAX_SUB_ATTRIBUTE_DEPTH,
            expectation::{Expectation, Found, Punctuation},
            filter_error::FilterError,
            parser::attribute_path::attribute_path,
        },
        name_stage::Written,
        query_text::grammar_fault::GrammarFault,
    };
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use winnow::stream::{LocatingSlice, Stream};

    /// Reads the hop of `text` whose leading name is its first `leading_length` bytes, returning the result and what stays unread.
    fn hop(text: &str, leading_length: usize) -> (Result<AttributePath<Written>, FilterError>, &str) {
        let mut input = LocatingSlice::new(text);
        let leading = input.next_slice(leading_length);
        let result = attribute_path(&mut input, leading, 0).map_err(|fault| match fault {
            GrammarFault::Refused(error) => error,
            GrammarFault::Backtrack { position } => panic!("{text}: a backtrack at {position} reached the top"),
        });
        (result, input.peek_finish())
    }

    /// The relationship, guard types and inner path of a hop.
    fn linked(path: &AttributePath<Written>) -> (&str, Vec<&str>, &AttributePath<Written>) {
        match path {
            AttributePath::Linked {
                relationship,
                guard_types,
                inner,
            } => (relationship.as_str(), guard_types.iter().map(EntityTypeName::as_str).collect(), inner),
            AttributePath::Local(local) => panic!("expected a hop, got the local path {local:?}"),
        }
    }

    /// The leading name of a path that crosses no hop.
    fn local(path: &AttributePath<Written>) -> &str {
        match path {
            AttributePath::Local(local) => local.attribute.as_str(),
            AttributePath::Linked { relationship, .. } => panic!("expected a local path, got a hop through {relationship}"),
        }
    }

    #[test]
    fn a_hop_without_a_guard_reaches_its_inner_path() {
        let (path, rest) = hop("a{b}==1", 1);
        let path = path.expect("a hop");
        let (relationship, guard, inner) = linked(&path);
        pretty_assertions::assert_eq!((relationship, guard, local(inner)), ("a", Vec::<&str>::new(), "b"));
        pretty_assertions::assert_eq!(rest, "==1");
    }

    #[test]
    fn a_text_with_no_brace_after_the_leading_name_is_a_local_path() {
        let (path, rest) = hop("a.b>1", 1);
        let path = path.expect("a local path");
        pretty_assertions::assert_eq!(local(&path), "a");
        pretty_assertions::assert_eq!(rest, ">1");
    }

    #[test]
    fn a_name_before_a_colon_is_the_one_type_of_a_guard_bare_or_quoted() {
        for text in [
            "sensor{Device:humidity}",
            "sensor{Device: humidity}",
            "sensor{\"Device\":humidity}",
            "sensor{\"Device\" :humidity}",
            "sensor{\"Device\": humidity}",
            "sensor{Device :humidity}",
            "sensor{ Device : humidity }",
            "sensor{\"ex:Device\":humidity}",
        ] {
            let (path, rest) = hop(text, 6);
            let path = path.unwrap_or_else(|error| panic!("{text}: {error:?}"));
            let (relationship, guard, inner) = linked(&path);
            let expected = if text.contains("ex:") { "ex:Device" } else { "Device" };
            pretty_assertions::assert_eq!((relationship, guard, local(inner)), ("sensor", vec![expected], "humidity"), "{text}");
            pretty_assertions::assert_eq!(rest, "", "{text}");
        }
    }

    #[test]
    fn a_guard_of_several_types_ends_at_the_colon_of_its_last_one() {
        for (text, guard) in [
            ("isParkedIn{Building,Store:category}", vec!["Building", "Store"]),
            ("isParkedIn{Building , Store : category}", vec!["Building", "Store"]),
            ("isParkedIn{\"ex:A\",Store:category}", vec!["ex:A", "Store"]),
            ("isParkedIn{Building,\"ex:Store\":category}", vec!["Building", "ex:Store"]),
            ("isParkedIn{A,B,C:category}", vec!["A", "B", "C"]),
        ] {
            let (path, _) = hop(text, 10);
            let path = path.unwrap_or_else(|error| panic!("{text}: {error:?}"));
            let (_, read, inner) = linked(&path);
            pretty_assertions::assert_eq!((read, local(inner)), (guard, "category"), "{text}");
        }
    }

    /// No unquoted name holds a colon, so the colon after the head's first name ends the list, and a name after it directly followed by a colon is refused at its first character.
    #[test]
    fn an_unquoted_name_after_a_type_list_directly_followed_by_a_colon_is_refused_at_its_first_character() {
        for (text, position) in [("sensor{ex:T:humidity}==40", 10), ("sensor{T1,ex:T2:b}==1", 13)] {
            let (path, _) = hop(text, 6);
            assert!(
                matches!(path, Err(FilterError::UnquotedColonName { position: at }) if at == position),
                "{text}: got {path:?}"
            );
        }
    }

    /// A colon after an unquoted first name ends the list, so a comma after the name that follows it stands where the closing brace was required.
    #[test]
    fn a_comma_after_the_name_that_follows_a_type_list_is_refused_where_the_brace_was_required() {
        let (path, _) = hop("sensor{ex:A,Store:category}", 6);
        assert!(
            matches!(
                path,
                Err(FilterError::Expected {
                    expected: Expectation::Punctuation(Punctuation::CloseBrace),
                    found: Found::Character(','),
                    position: 11
                })
            ),
            "got {path:?}"
        );
    }

    /// A quoted first name with no comma or colon after it is the leading name of the inner path, the colons it holds belonging to it, while an unquoted first name before a colon is a type.
    #[test]
    fn a_quoted_first_name_with_no_list_mark_after_it_is_the_inner_path() {
        let (path, _) = hop("a{\"prefix:name\"}", 1);
        let path = path.expect("a quoted prefixed attribute");
        let (_, guard, inner) = linked(&path);
        pretty_assertions::assert_eq!((guard, local(inner)), (Vec::<&str>::new(), "prefix:name"));
        let (path, _) = hop("sensor{ex:humidity}==40", 6);
        let path = path.expect("an unquoted first name before a colon is a type");
        let (_, guard, inner) = linked(&path);
        pretty_assertions::assert_eq!((guard, local(inner)), (vec!["ex"], "humidity"));
    }

    #[test]
    fn a_guard_whose_last_type_has_no_colon_is_refused_where_the_colon_was_required() {
        for (text, found, position) in [
            ("a{A,B}", Found::Character('}'), 5),
            ("a{A,B", Found::EndOfInput, 5),
            ("a{A,B c}", Found::Character('c'), 6),
        ] {
            let (path, _) = hop(text, 1);
            let error = path.expect_err("the terminating colon is required");
            assert!(
                matches!(error, FilterError::Expected { expected: Expectation::GuardTerminator, found: refused, position: at } if refused == found && at == position),
                "{text}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_comma_with_no_type_after_it_is_refused_where_the_type_was_required() {
        let (path, _) = hop("a{A,}", 1);
        let error = path.expect_err("a type is required after the comma");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::EntityType,
                    found: Found::Character('}'),
                    position: 4
                }
            ),
            "got {error:?}"
        );
    }

    /// An unquoted name ends at the first character outside `AttrName`, so only a quoted type can be outside the clause 4.6.2 grammar.
    #[test]
    fn a_type_outside_the_name_grammar_is_refused_before_the_next_read() {
        let (path, _) = hop("a{\"x-\",B:c}", 1);
        assert!(matches!(path, Err(FilterError::InvalidAttributeName(_))), "got {path:?}");
        let (path, _) = hop("a{A,\"x-\":c}", 1);
        assert!(matches!(path, Err(FilterError::InvalidAttributeName(_))), "got {path:?}");
        let (path, _) = hop("a{A,x-:c}", 1);
        assert!(
            matches!(
                path,
                Err(FilterError::Expected {
                    expected: Expectation::GuardTerminator,
                    found: Found::Character('-'),
                    position: 5
                })
            ),
            "got {path:?}"
        );
    }

    #[test]
    fn a_hop_with_nothing_inside_or_left_open_is_refused_where_the_grammar_required_more() {
        for (text, expected, found, position) in [
            ("a{}", Expectation::AttributeName, Found::Character('}'), 2),
            ("a{", Expectation::AttributeName, Found::EndOfInput, 2),
            ("a{b", Expectation::Punctuation(Punctuation::CloseBrace), Found::EndOfInput, 3),
            ("a{b==1}", Expectation::Punctuation(Punctuation::CloseBrace), Found::Character('='), 3),
            ("a{b{c>5}}", Expectation::Punctuation(Punctuation::CloseBrace), Found::Character('>'), 5),
        ] {
            let (path, _) = hop(text, 1);
            let error = path.expect_err("the hop is malformed");
            assert!(
                matches!(error, FilterError::Expected { expected: refused, found: stood, position: at } if refused == expected && stood == found && at == position),
                "{text}: got {error:?}"
            );
        }
    }

    #[test]
    fn hops_nest_and_share_the_depth_budget_with_sub_attributes() {
        let (path, _) = hop("a{b{c}}", 1);
        let path = path.expect("nested hops");
        let (_, _, inner) = linked(&path);
        let (relationship, _, terminal) = linked(inner);
        pretty_assertions::assert_eq!((relationship, local(terminal)), ("b", "c"));

        let chain = |hops: usize| format!("a{}z{}", "{a".repeat(hops - 1) + "{", "}".repeat(hops));
        let (path, _) = hop(&chain(MAX_SUB_ATTRIBUTE_DEPTH), 1);
        assert!(path.is_ok(), "a chain at the limit parses: {path:?}");
        let (path, _) = hop(&chain(MAX_SUB_ATTRIBUTE_DEPTH + 1), 1);
        assert!(matches!(path, Err(FilterError::PathTooDeep { .. })), "got {path:?}");
    }

    #[test]
    fn json_whitespace_stands_between_the_marks_of_a_hop() {
        let (path, rest) = hop("rel { Building , Store : category } ;", 3);
        let path = path.expect("whitespace is skipped");
        let (relationship, guard, inner) = linked(&path);
        pretty_assertions::assert_eq!((relationship, guard, local(inner)), ("rel", vec!["Building", "Store"], "category"));
        pretty_assertions::assert_eq!(rest, " ;");
    }
}
