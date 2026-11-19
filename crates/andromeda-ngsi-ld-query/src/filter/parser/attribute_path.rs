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

use crate::{
    filter::{
        attribute_path::AttributePath,
        expectation::{Expectation, Found, Punctuation},
        filter_error::FilterError,
        parser::{
            expected,
            value_path::{attribute_name, check_depth, entity_type_name, value_path},
            word::{Word, name_extent, word},
        },
        quoting::Quoting,
    },
    name_stage::Written,
    query_text::{grammar_fault::GrammarFault, whitespace::Whitespace},
};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use winnow::{
    Parser,
    stream::{LocatingSlice, Location, Stream},
};

/// What the head of a linked-entity hop contributed before its inner path.
#[derive(Debug)]
struct HopGuard<'a> {
    /// The entity types the hop is constrained to, empty when it carried none.
    types: Vec<EntityTypeName>,
    /// The inner path's leading name when the guard's terminating colon carried
    /// it (`Store:category`); `None` when the inner path starts at the next word.
    inner_leading: Option<&'a str>,
}

/// Reads the attribute path of a Query Term whose leading name `leading` has already been read: a `ValuePath`, or a hop `leading{LinkedEntityPath}`.
///
/// `depth` counts the path segments consumed so far (sub-attributes and linked-entity hops together), so a chain of hops is bounded by the same budget a chain of sub-attributes is. The JSON whitespace between any two marks is skipped.
///
/// # Errors
///
/// Refuses with [`FilterError::Expected`] when a hop is empty, left unclosed or holds anything after its path (a comparison written inside the braces), and with the refusals of [`value_path`], of the entity-type guard and of [`check_depth`].
pub(super) fn attribute_path(input: &mut LocatingSlice<&str>, leading: &str, depth: usize) -> Result<AttributePath<Written>, GrammarFault<FilterError>> {
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    if input.peek_token() != Some('{') {
        return value_path(input, leading, depth).map(AttributePath::Local);
    }
    let relationship = attribute_name(leading).map_err(GrammarFault::Refused)?;
    let _opening = input.next_token();
    check_depth(depth + 1).map_err(GrammarFault::Refused)?;
    let guard = hop_guard(input)?;
    let inner_leading = match guard.inner_leading {
        Some(text) => text,
        None => expected(Expectation::AttributeName, word).parse_next(input)?.text,
    };
    let inner = attribute_path(input, inner_leading, depth + 1)?;
    expected(Expectation::Punctuation(Punctuation::CloseBrace), '}').parse_next(input)?;
    Ok(AttributePath::Linked {
        relationship,
        guard_types: guard.types,
        inner: Box::new(inner),
    })
}

/// Reads the optional entity-type guard at the head of a linked-entity hop.
///
/// The guard is recognised from shape (see [`guard_opens`]). Each type is validated before the next is read.
///
/// # Errors
///
/// Refuses with [`FilterError::Expected`] when a type name is missing or the guard's terminating colon is absent, [`FilterError::AmbiguousTypeGuard`] when a bare last name carries two colons, and [`FilterError::InvalidAttributeName`] when a type name is outside the clause 4.6.2 grammar.
fn hop_guard<'i>(input: &mut LocatingSlice<&'i str>) -> Result<HopGuard<'i>, GrammarFault<FilterError>> {
    if !guard_opens(input)? {
        return Ok(HopGuard {
            types: Vec::new(),
            inner_leading: None,
        });
    }
    let mut types = Vec::new();
    loop {
        let name = expected(Expectation::EntityType, word).parse_next(input)?;
        Whitespace::Json.skip::<FilterError>().parse_next(input)?;
        if input.peek_token() == Some(',') {
            types.push(entity_type_name(name.text).map_err(GrammarFault::Refused)?);
            let _separator = input.next_token();
            continue;
        }
        let (last, inner_leading) = guard_end(input, name)?;
        types.push(entity_type_name(last).map_err(GrammarFault::Refused)?);
        return Ok(HopGuard { types, inner_leading });
    }
}

/// Whether the head of a hop opens a type guard: a word followed by a comma, a bare word holding a colon, or a quoted word followed by a colon.
///
/// Clause 4.9 spells the guard `*1(EntityType 1*("," EntityType) ":")` and writes it with one type in EXAMPLE 14 (`sensor{Device:humidity}`). `:` is a legal name character (a prefixed name carries one), so the colon arrives glued to whatever the client wrote next and the guard is recognised from shape, not from a colon of its own. A comma directly after the first word inside the braces opens a guard of several types, and nothing else in the grammar puts a comma there. A bare first word holding a colon opens a guard of one type (`Device:humidity`): no `AttrName` and no `EntityType` of the grammar holds a colon, so `X:y` inside the braces derives only as a type and an attribute. A quoted first word is an expanded IRI whose own colons belong to it, so only a colon after it opens the guard. A prefixed attribute name there is written quoted (`sensor{"ex:humidity"}`).
///
/// The input is left where it was, whatever the lookahead finds.
fn guard_opens(input: &mut LocatingSlice<&str>) -> Result<bool, GrammarFault<FilterError>> {
    let start = input.checkpoint();
    let opens = guard_head_opens(input);
    input.reset(&start);
    opens
}

/// Reads the word at the head of a hop and the character after it, and decides whether they open a guard.
fn guard_head_opens(input: &mut LocatingSlice<&str>) -> Result<bool, GrammarFault<FilterError>> {
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    let head = match word.parse_next(input) {
        Ok(head) => head,
        Err(GrammarFault::Backtrack { .. }) => return Ok(false),
        Err(refusal @ GrammarFault::Refused(_)) => return Err(refusal),
    };
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    let next = input.peek_token();
    Ok(next == Some(',')
        || match head.quoting {
            Quoting::Bare => head.text.contains(':'),
            Quoting::Quoted => next == Some(':'),
        })
}

/// Splits the last type of a guard from the inner path that follows its colon, for a last type `name` that no comma follows.
///
/// A bare last name holding a colon carries the terminator glued to it (`Store:` or `Store:category`). Otherwise a colon standing after the name is a word of its own that the name extent takes whole, which is how a quoted type, or a type the client followed by a space, meets its colon. An empty remainder means the inner path starts at the next word.
///
/// A bare last name holding *two* colons is ambiguous and is refused rather than resolved by guesswork: `ex:Store:category` reads equally well as the prefixed type `ex:Store` before the attribute `category` or as the type `ex` before the prefixed attribute `Store:category`, and clause 4.9 decides neither: its `EntityType` production admits no colon at all, while real filters do carry prefixed names. Quoting the type says which was meant.
///
/// Returns the text of the last type and the leading name of the inner path when the colon carried one.
fn guard_end<'i>(input: &mut LocatingSlice<&'i str>, name: Word<'i>) -> Result<(&'i str, Option<&'i str>), GrammarFault<FilterError>> {
    if name.quoting == Quoting::Bare
        && let Some((last, rest)) = name.text.split_once(':')
    {
        if rest.contains(':') {
            return Err(GrammarFault::Refused(FilterError::AmbiguousTypeGuard));
        }
        return Ok((last, (!rest.is_empty()).then_some(rest)));
    }
    if input.peek_token() == Some(':') {
        let extent = name_extent(input.peek_finish());
        let colon_word = input.next_slice(extent);
        return Ok((name.text, colon_word.strip_prefix(':').filter(|rest| !rest.is_empty())));
    }
    Err(GrammarFault::Refused(FilterError::Expected {
        expected: Expectation::GuardTerminator,
        found: Found::of(input.peek_token()),
        position: input.current_token_start(),
    }))
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
        ] {
            let (path, rest) = hop(text, 6);
            let path = path.unwrap_or_else(|error| panic!("{text}: {error:?}"));
            let (relationship, guard, inner) = linked(&path);
            pretty_assertions::assert_eq!((relationship, guard, local(inner)), ("sensor", vec!["Device"], "humidity"), "{text}");
            pretty_assertions::assert_eq!(rest, "", "{text}");
        }
    }

    #[test]
    fn a_guard_of_several_types_ends_at_the_colon_of_its_last_one() {
        for (text, guard) in [
            ("isParkedIn{Building,Store:category}", vec!["Building", "Store"]),
            ("isParkedIn{Building , Store : category}", vec!["Building", "Store"]),
            ("isParkedIn{ex:A,Store:category}", vec!["ex:A", "Store"]),
            ("isParkedIn{Building,\"ex:Store\":category}", vec!["Building", "ex:Store"]),
            ("isParkedIn{A,B,C:category}", vec!["A", "B", "C"]),
        ] {
            let (path, _) = hop(text, 10);
            let path = path.unwrap_or_else(|error| panic!("{text}: {error:?}"));
            let (_, read, inner) = linked(&path);
            pretty_assertions::assert_eq!((read, local(inner)), (guard, "category"), "{text}");
        }
    }

    #[test]
    fn a_bare_last_type_with_two_colons_is_ambiguous() {
        for text in ["rel{ex:Store:category}", "rel{Building,ex:Store:category}"] {
            let (path, _) = hop(text, 3);
            assert!(matches!(path, Err(FilterError::AmbiguousTypeGuard)), "{text}: got {path:?}");
        }
    }

    #[test]
    fn a_quoted_name_or_a_name_with_a_space_before_its_colon_is_no_guard_unless_the_colon_follows_it() {
        let (path, _) = hop("a{\"prefix:name\"}", 1);
        let path = path.expect("a quoted prefixed attribute");
        let (_, guard, inner) = linked(&path);
        pretty_assertions::assert_eq!((guard, local(inner)), (Vec::<&str>::new(), "prefix:name"));

        let (path, _) = hop("sensor{Device :humidity}==40", 6);
        let error = path.expect_err("a bare name with a space before its colon is no guard");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Punctuation(Punctuation::CloseBrace),
                    found: Found::Character(':'),
                    position: 14
                }
            ),
            "got {error:?}"
        );
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

    #[test]
    fn a_type_outside_the_name_grammar_is_refused_before_the_next_read() {
        let (path, _) = hop("a{x-,B:c}", 1);
        assert!(matches!(path, Err(FilterError::InvalidAttributeName(_))), "got {path:?}");
        let (path, _) = hop("a{A,x-:c}", 1);
        assert!(matches!(path, Err(FilterError::InvalidAttributeName(_))), "got {path:?}");
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
