//! Recursive-descent parser for the NGSI-LD Query Language (clause 4.9).
//!
//! The grammar this parser implements is reproduced verbatim as ABNF at the root of this crate. Nothing derives from it: the language needs operator precedence and value typing that a pure ABNF recogniser does not express, so it is the reference this parser is checked against, and the two are kept in step by hand.
//!
//! This module owns the grammar of a `q` filter: the boolean structure of a `Query`,
//! and the `QueryTerm` that combines an attribute path with an operator and an
//! operand. It reads the text directly: every
//! production skips the JSON whitespace before the marks it reads, and a
//! refusal is reported at the byte offset where the grammar required something.
//! The productions it composes each live with the concept they belong to,
//! because `q` is not the only parameter that derives from them:
//! [`value_path`] holds the `ValuePath` axis clause 4.23.3 shares,
//! [`attribute_path`] the `LinkedEntityRelation` hop, [`operand`] the operand
//! literals, [`word`] the name, entity type and pattern operand,
//! [`time_value_extent`] where a bare time value ends, and [`quoted_string`]
//! the quoted literal with its RFC 8259 escapes.

mod attribute_path;
pub(in crate::filter) mod operand;
mod quoted_string;
mod time_value_extent;
mod value_path;
mod word;

use crate::{
    filter::{
        Filter,
        complexity::WRITTEN_NESTING_CEILING,
        expectation::{Expectation, Found, Punctuation},
        filter_error::FilterError,
        operator_spelling::{AND, EQUAL, GREATER, GREATER_OR_EQUAL, LESS, LESS_OR_EQUAL, MATCH, NOT_MATCH, OR, UNEQUAL},
        parser::{
            attribute_path::attribute_path,
            operand::{BareWordExtent, comparable_value, equality_value, pattern_operand},
            word::word,
        },
        query_term::{OrderingOperator, QueryTerm, SetMembership},
    },
    name_stage::Written,
    query_text::{grammar_fault::GrammarFault, whitespace::Whitespace},
};
use std::str::FromStr;
use winnow::{
    Parser,
    combinator::separated,
    stream::{LocatingSlice, Location, Stream},
};

impl FromStr for Filter<Written> {
    type Err = FilterError;

    /// Parses the text of an NGSI-LD `q` filter into a [`Filter`] (clause 4.9).
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::Empty`] when the input is blank, [`FilterError::Expected`],
    /// [`FilterError::UnexpectedCharacter`], [`FilterError::UnterminatedString`],
    /// [`FilterError::PathTooDeep`] or one of the literal-typing variants when it is
    /// outside the clause 4.9 grammar, and [`FilterError::NestedTooDeep`] when its
    /// parentheses nest past [`WRITTEN_NESTING_CEILING`].
    fn from_str(input: &str) -> Result<Filter<Written>, FilterError> {
        if input.trim().is_empty() {
            return Err(FilterError::Empty);
        }
        let mut stream = LocatingSlice::new(input);
        whole_filter(&mut stream).map_err(|fault| entry_refusal(input, fault))
    }
}

/// Reads a filter and then requires the end of the text.
fn whole_filter(input: &mut LocatingSlice<&str>) -> Result<Filter<Written>, GrammarFault<FilterError>> {
    let filter = disjunction(input, 0)?;
    complete(input)?;
    Ok(filter)
}

/// Reads a disjunction: `conjunction ('|' conjunction)*`, inside `depth` parentheses.
///
/// One alternative is itself, several are an [`Filter::Or`].
fn disjunction(input: &mut LocatingSlice<&str>, depth: usize) -> Result<Filter<Written>, GrammarFault<FilterError>> {
    let alternatives: Vec<Filter<Written>> = separated(
        1..,
        |input: &mut LocatingSlice<&str>| conjunction(input, depth),
        (Whitespace::Json.skip::<FilterError>(), OR),
    )
    .parse_next(input)?;
    Ok(match <[Filter<Written>; 1]>::try_from(alternatives) {
        Ok([only]) => only,
        Err(several) => Filter::Or(several),
    })
}

/// Reads a conjunction: `primary (';' primary)*`, inside `depth` parentheses.
///
/// One primary is itself, several are a [`Filter::And`].
fn conjunction(input: &mut LocatingSlice<&str>, depth: usize) -> Result<Filter<Written>, GrammarFault<FilterError>> {
    let primaries: Vec<Filter<Written>> = separated(
        1..,
        |input: &mut LocatingSlice<&str>| primary(input, depth),
        (Whitespace::Json.skip::<FilterError>(), AND),
    )
    .parse_next(input)?;
    Ok(match <[Filter<Written>; 1]>::try_from(primaries) {
        Ok([only]) => only,
        Err(several) => Filter::And(several),
    })
}

/// Reads a primary: either a parenthesised group or a single query term, inside `depth` parentheses.
///
/// Each group recurses once, so the group that would nest past [`WRITTEN_NESTING_CEILING`] is refused before its parenthesis is taken. The refusal is made when the `(` is reached, before anything after it is read, so a lexical fault later in the text never replaces it.
fn primary(input: &mut LocatingSlice<&str>, depth: usize) -> Result<Filter<Written>, GrammarFault<FilterError>> {
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    if input.peek_token() != Some('(') {
        return query_term(input).map(Filter::Term);
    }
    let inner = depth + 1;
    if inner > WRITTEN_NESTING_CEILING.get() {
        return Err(GrammarFault::Refused(FilterError::NestedTooDeep {
            depth: inner,
            limit: WRITTEN_NESTING_CEILING,
        }));
    }
    let _opening = input.next_token();
    let filter = disjunction(input, inner)?;
    expected(Expectation::Punctuation(Punctuation::CloseParen), ')').parse_next(input)?;
    Ok(filter)
}

/// Reads `parser` after the JSON whitespace before it, turning its backtrack into the refusal that `expectation` was required where it started.
///
/// A refusal of `parser` passes through unchanged.
fn expected<'i, O>(
    expectation: Expectation,
    mut parser: impl Parser<LocatingSlice<&'i str>, O, GrammarFault<FilterError>>,
) -> impl Parser<LocatingSlice<&'i str>, O, GrammarFault<FilterError>> {
    move |input: &mut LocatingSlice<&'i str>| -> Result<O, GrammarFault<FilterError>> {
        Whitespace::Json.skip::<FilterError>().parse_next(input)?;
        let start = input.checkpoint();
        match parser.parse_next(input) {
            Err(GrammarFault::Backtrack { .. }) => {
                input.reset(&start);
                Err(GrammarFault::Refused(FilterError::Expected {
                    expected: expectation,
                    found: Found::of(input.peek_token()),
                    position: input.current_token_start(),
                }))
            }
            Ok(value) => Ok(value),
            Err(refusal @ GrammarFault::Refused(_)) => Err(refusal),
        }
    }
}

/// What a Query Term compares its attribute path with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Comparison {
    /// `==` or `!=`, reading an equality value.
    Equality(SetMembership),
    /// `>`, `>=`, `<` or `<=`, reading a comparable value.
    Ordering(OrderingOperator),
    /// `~=`, reading a pattern.
    MatchPattern,
    /// `!~=`, reading a pattern.
    NotMatchPattern,
}

/// The comparison operators in the order they are tried: an operator is tried before every operator it is a prefix of, so `!~=` is no `!=` and `>=` is no `>`.
const COMPARISONS: [(&str, Comparison); 8] = [
    (EQUAL, Comparison::Equality(SetMembership::Within)),
    (NOT_MATCH, Comparison::NotMatchPattern),
    (UNEQUAL, Comparison::Equality(SetMembership::Outside)),
    (MATCH, Comparison::MatchPattern),
    (GREATER_OR_EQUAL, Comparison::Ordering(OrderingOperator::GreaterOrEqual)),
    (GREATER, Comparison::Ordering(OrderingOperator::Greater)),
    (LESS_OR_EQUAL, Comparison::Ordering(OrderingOperator::LessOrEqual)),
    (LESS, Comparison::Ordering(OrderingOperator::Less)),
];

/// Reads one Query Term: its attribute path (clause 4.9 `Attribute`, a hop's braces included), optionally followed by an operator and its operand.
///
/// A path with no operator after it is a bare existence term, and what follows it belongs to the enclosing filter.
fn query_term(input: &mut LocatingSlice<&str>) -> Result<QueryTerm<Written>, GrammarFault<FilterError>> {
    let leading = expected(Expectation::AttributeName, word).parse_next(input)?;
    let path = attribute_path(input, leading.text, 0)?;
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    let stands = COMPARISONS.iter().find(|(spelling, _)| input.peek_finish().starts_with(spelling));
    let Some(&(spelling, comparison)) = stands else {
        return Ok(QueryTerm::Exists(path));
    };
    let _operator = input.next_slice(spelling.len());
    match comparison {
        Comparison::Equality(membership) => {
            let value = equality_value(input)?;
            Ok(QueryTerm::Equality { path, membership, value })
        }
        Comparison::Ordering(operator) => {
            let value = comparable_value(BareWordExtent::Name).parse_next(input)?;
            Ok(QueryTerm::Ordering { path, operator, value })
        }
        Comparison::MatchPattern => {
            let pattern = pattern_operand(input)?;
            Ok(QueryTerm::MatchPattern { path, pattern })
        }
        Comparison::NotMatchPattern => {
            let pattern = pattern_operand(input)?;
            Ok(QueryTerm::NotMatchPattern { path, pattern })
        }
    }
}

/// The refusal an entry point reports for the fault its parse ended in: a refusal as decided, a backtrack as an attribute name required where it stood.
fn entry_refusal(text: &str, fault: GrammarFault<FilterError>) -> FilterError {
    fault.finish(|position| FilterError::Expected {
        expected: Expectation::AttributeName,
        found: Found::of(text.get(position..).and_then(|rest| rest.chars().next())),
        position,
    })
}

/// Requires that nothing but JSON whitespace remains, refusing the first character that does as input continuing where the text is complete.
fn complete(input: &mut LocatingSlice<&str>) -> Result<(), GrammarFault<FilterError>> {
    Whitespace::Json.skip::<FilterError>().parse_next(input)?;
    let position = input.current_token_start();
    match input.peek_token() {
        Some(character) => Err(GrammarFault::Refused(FilterError::UnexpectedCharacter { character, position })),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            Filter,
            attribute_path::AttributePath,
            complexity::{MAX_SUB_ATTRIBUTE_DEPTH, WRITTEN_NESTING_CEILING},
            equality_value::EqualityValue,
            expectation::{Expectation, Found, Punctuation},
            filter_error::FilterError,
            language_range::LanguageRangeError,
            parser::{expected, query_term},
            query_term::{OrderingOperator, QueryTerm, SetMembership},
            quoting::Quoting,
            scalar::Scalar,
            target_language::TargetLanguage,
            trailing_group::TrailingGroup,
            value_path::ValuePath,
        },
        name_stage::Written,
        pattern::{Pattern, pattern_error::PatternError},
        query_text::grammar_fault::GrammarFault,
    };
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        entity_type_name::EntityTypeName,
        reference::NgsiLdReferenceError,
        time::{date_time::NgsiLdDateTimeError, lexical::LexicalError, rendering::instant_text, time_of_day::NgsiLdTimeError},
    };
    use andromeda_ngsi_ld_number::JsonNumber;
    use winnow::{
        Parser,
        combinator::fail,
        stream::{LocatingSlice, Stream},
    };

    /// Reads a Query Term from `text`, returning the result and what stays unread.
    fn read_term(text: &str) -> (Result<QueryTerm<Written>, FilterError>, &str) {
        let mut input = LocatingSlice::new(text);
        let result = query_term(&mut input).map_err(|fault| match fault {
            GrammarFault::Refused(error) => error,
            GrammarFault::Backtrack { position } => panic!("{text}: a backtrack at {position} reached the top"),
        });
        (result, input.peek_finish())
    }

    fn number(value: i32) -> Scalar {
        Scalar::Number(JsonNumber::from(value))
    }

    #[test]
    fn the_pattern_operators_are_read_before_the_operators_they_begin_with() {
        let (term, _) = read_term("a!~=\"x\"");
        assert!(matches!(term, Ok(QueryTerm::NotMatchPattern { .. })), "got {term:?}");
        let (term, _) = read_term("a~=\"x\"");
        assert!(matches!(term, Ok(QueryTerm::MatchPattern { .. })), "got {term:?}");
        let (term, rest) = read_term("a!=~");
        assert!(term.is_err(), "`~` is no value: {term:?}");
        pretty_assertions::assert_eq!(rest, "~");
    }

    #[test]
    fn a_path_with_no_operator_is_an_existence_term_and_leaves_the_rest_unread() {
        for (text, rest) in [("a", ""), ("a ;b", ";b"), ("a|b", "|b"), ("a)", ")"), ("a=1", "=1"), ("a.b ", ""), ("a[b]", "")] {
            let (term, unread) = read_term(text);
            assert!(matches!(term, Ok(QueryTerm::Exists(_))), "{text}: got {term:?}");
            pretty_assertions::assert_eq!(unread, rest, "{text}");
        }
    }

    #[test]
    fn json_whitespace_stands_between_the_path_the_operator_and_the_operand() {
        let (term, rest) = read_term(" speed \t>=\n 50 ;x");
        let term = term.expect("whitespace is skipped");
        assert!(
            matches!(&term, QueryTerm::Ordering { operator: OrderingOperator::GreaterOrEqual, value, .. } if *value == number(50)),
            "got {term:?}"
        );
        pretty_assertions::assert_eq!(rest, " ;x");
    }

    #[test]
    fn a_hop_reaches_its_operator_after_the_closing_brace() {
        let (term, rest) = read_term("isParkedIn{Building:category}==\"x\";y");
        let term = term.expect("a hop term");
        assert!(
            matches!(
                term,
                QueryTerm::Equality {
                    path: AttributePath::Linked { .. },
                    ..
                }
            ),
            "got {term:?}"
        );
        pretty_assertions::assert_eq!(rest, ";y");
    }

    #[test]
    fn a_missing_operand_is_refused_where_the_operator_required_one() {
        for (text, expected, found, position) in [
            ("a==", Expectation::Value, Found::EndOfInput, 3),
            ("a>", Expectation::Value, Found::EndOfInput, 2),
            ("a>true", Expectation::ComparableValue, Found::Character('t'), 2),
            ("a>= urn:x:y", Expectation::ComparableValue, Found::Character('u'), 4),
            ("a==urn:a.b..urn:c.d", Expectation::ComparableValue, Found::Character('u'), 3),
            ("a~=1", Expectation::PatternString, Found::Character('1'), 3),
            ("a!~=", Expectation::PatternString, Found::EndOfInput, 4),
            ("==1", Expectation::AttributeName, Found::Character('='), 0),
        ] {
            let (term, _) = read_term(text);
            let error = term.expect_err("an operand is required");
            assert!(
                matches!(error, FilterError::Expected { expected: required, found: stood, position: at } if required == expected && stood == found && at == position),
                "{text}: got {error:?}"
            );
        }
    }

    /// The refusal of `expected(Value, fail)` over `text` once the first `consumed` bytes are read.
    fn value_required_after(text: &str, consumed: usize) -> FilterError {
        let mut input = LocatingSlice::new(text);
        let _read = input.next_slice(consumed);
        let result: Result<(), GrammarFault<FilterError>> = expected(Expectation::Value, fail).parse_next(&mut input);
        match result {
            Err(GrammarFault::Refused(error)) => error,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_q_expectation_reads_what_stood_at_the_backtrack() {
        assert!(
            matches!(
                value_required_after("a==", 3),
                FilterError::Expected {
                    expected: Expectation::Value,
                    found: Found::EndOfInput,
                    position: 3
                }
            ),
            "the end of the text is named"
        );
        assert!(
            matches!(
                value_required_after("a==@", 3),
                FilterError::Expected {
                    expected: Expectation::Value,
                    found: Found::Character('@'),
                    position: 3
                }
            ),
            "the character that stood there is named"
        );
    }

    #[test]
    fn the_q_expectation_skips_json_whitespace_before_it_reads() {
        assert!(
            matches!(
                value_required_after("a== \t\n\r@", 3),
                FilterError::Expected {
                    expected: Expectation::Value,
                    found: Found::Character('@'),
                    position: 7
                }
            ),
            "the position follows the whitespace"
        );
    }

    #[test]
    fn parse_existence() {
        let filter = "temperature".parse::<Filter<Written>>().unwrap();
        pretty_assertions::assert_eq!(
            filter,
            Filter::Term(QueryTerm::Exists(AttributePath::Local(ValuePath::simple(
                "temperature".parse().expect("valid attribute name")
            ))))
        );
    }

    #[test]
    fn parse_equality_number() {
        let filter = "temperature==20".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Equality { path, membership, value }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "temperature");
                pretty_assertions::assert_eq!(membership, SetMembership::Within);
                pretty_assertions::assert_eq!(value, EqualityValue::Scalar(Scalar::Number(20.into())));
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn an_attribute_path_within_the_depth_limit_parses() {
        let at_limit = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH));
        assert!(at_limit.parse::<Filter<Written>>().is_ok(), "a path at the limit must parse");
    }

    #[test]
    fn an_attribute_path_deeper_than_the_limit_is_rejected() {
        // The limit is the depth a write is admitted to, so a deeper path
        // addresses nothing storable while its backend rendering grows with
        // its depth.
        let too_deep = format!("a{}", ".s".repeat(MAX_SUB_ATTRIBUTE_DEPTH + 1));
        assert!(
            matches!(too_deep.parse::<Filter<Written>>(), Err(FilterError::PathTooDeep { .. })),
            "an over-deep path must be rejected"
        );
    }

    /// Clause 5.5.6: parentheses past the ceiling are `TooComplexQuery`, and
    /// the refusal comes before the parser recurses into them.
    #[test]
    fn parentheses_at_the_written_ceiling_parse_and_one_level_deeper_are_too_complex() {
        let grouped = |levels: usize| format!("{}a{}", "(".repeat(levels), ")".repeat(levels));
        assert!(
            grouped(WRITTEN_NESTING_CEILING.get()).parse::<Filter<Written>>().is_ok(),
            "parentheses at the ceiling must parse"
        );
        let error = grouped(WRITTEN_NESTING_CEILING.get() + 1).parse::<Filter<Written>>().expect_err("must reject");
        assert!(
            matches!(error, FilterError::NestedTooDeep { depth: 65, limit } if limit == WRITTEN_NESTING_CEILING),
            "got {error:?}"
        );
        let unbounded = format!("{}a", "(".repeat(1_000_000));
        assert!(matches!(
            unbounded.parse::<Filter<Written>>(),
            Err(FilterError::NestedTooDeep { depth: 65, .. })
        ));
    }

    #[test]
    fn parse_inequality_string() {
        let filter = "brandName!=\"Mercedes\"".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Equality { path, membership, value }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "brandName");
                pretty_assertions::assert_eq!(membership, SetMembership::Outside);
                pretty_assertions::assert_eq!(value, EqualityValue::Scalar(Scalar::Str("Mercedes".to_string())));
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_greater_than() {
        let filter = "speed>50".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Ordering { path, operator, value }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "speed");
                pretty_assertions::assert_eq!(operator, OrderingOperator::Greater);
                pretty_assertions::assert_eq!(value, Scalar::Number(50.into()));
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_filter() {
        let filter = "speed>50;brandName!=\"Mercedes\"".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::And(filters) => pretty_assertions::assert_eq!(filters.len(), 2),
            other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parse_or_filter() {
        let filter = "speed>50|temperature<10".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Or(filters) => pretty_assertions::assert_eq!(filters.len(), 2),
            other @ (Filter::And(_) | Filter::Term(_)) => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_and_takes_precedence_over_or() {
        let filter = "a==1|b==2;c==3".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Or(filters) => {
                pretty_assertions::assert_eq!(filters.len(), 2);
                match &filters[1] {
                    Filter::And(and_filters) => pretty_assertions::assert_eq!(and_filters.len(), 2),
                    other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected And as second OR term, got {other:?}"),
                }
            }
            other @ (Filter::And(_) | Filter::Term(_)) => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_parenthesized_group() {
        let filter = "(speed>50|rpm>3000);brandName==\"Mercedes\"".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::And(filters) => {
                pretty_assertions::assert_eq!(filters.len(), 2);
                match &filters[0] {
                    Filter::Or(_) => {}
                    other @ (Filter::And(_) | Filter::Term(_)) => panic!("expected Or inside parens, got {other:?}"),
                }
            }
            other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn parse_range() {
        let filter = "temperature==10..20".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Equality { value, .. }) => match value {
                EqualityValue::Range { min, max } => {
                    pretty_assertions::assert_eq!(min, Scalar::Number(10.into()));
                    pretty_assertions::assert_eq!(max, Scalar::Number(20.into()));
                }
                other @ (EqualityValue::Scalar(_) | EqualityValue::ValueList(_)) => panic!("expected Range, got {other:?}"),
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    /// Clause 4.9 admits a range only after `==` and `!=`; an ordering
    /// operator takes a single value.
    #[test]
    fn a_range_after_an_ordering_operator_does_not_parse() {
        assert!("temperature>10..20".parse::<Filter<Written>>().is_err());
    }

    #[test]
    fn parse_value_list() {
        let filter = "color==\"red\",\"blue\",\"green\"".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Equality { value, .. }) => match value {
                EqualityValue::ValueList(values) => {
                    pretty_assertions::assert_eq!(values.len(), 3);
                }
                other @ (EqualityValue::Scalar(_) | EqualityValue::Range { .. }) => panic!("expected ValueList, got {other:?}"),
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_match_pattern() {
        let filter = "brandName~=\"Merc.*\"".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::MatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "brandName");
                pretty_assertions::assert_eq!(pattern.as_str(), "Merc.*");
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected MatchPattern, got {other:?}"),
        }
    }

    #[test]
    fn parse_not_match_pattern() {
        let filter = "brandName!~=\"Merc.*\"".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::NotMatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "brandName");
                pretty_assertions::assert_eq!(pattern.as_str(), "Merc.*");
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected NotMatchPattern, got {other:?}"),
        }
    }

    #[test]
    fn a_date_time_scalar_is_parsed_once_at_filter_parse_time() {
        // Clause 4.6.3: a temporal operand is an RFC 3339 value, so a bare
        // literal that has the shape of one but is not a real instant is
        // derivable from no production of the clause 4.9 grammar (`quotedStr`
        // carries the quotes it lacks), and the request is malformed.
        match "observed>=2017-12-24T12:00:00Z".parse::<Filter<Written>>().expect("parses") {
            Filter::Term(QueryTerm::Ordering {
                value: Scalar::DateTime(_), ..
            }) => {}
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a date-time operand, got {other:?}"),
        }

        let error = "observed>=2017-13-45".parse::<Filter<Written>>().expect_err("must reject");
        assert!(matches!(error, FilterError::InvalidDate { .. }), "got {error:?}");

        let error = "observed>=2017-12-24T99:00:00Z".parse::<Filter<Written>>().expect_err("must reject");
        assert!(matches!(error, FilterError::InvalidDateTime { .. }), "got {error:?}");
    }

    /// Clause 4.6.3 lets a request write "a comma instead of a decimal point"
    /// before the fraction of the seconds, and clause 4.9 takes its `dateTime`
    /// and `time` operands "as mandated by clause 4.6.3": after an ordering
    /// operator, in a range and in a value list alike, the comma reads as the
    /// point and is written back as one.
    #[test]
    fn a_comma_before_the_fraction_reads_as_the_decimal_point() {
        for (comma, point) in [
            ("tm>2024-01-01T00:00:00,5Z", "tm>2024-01-01T00:00:00.5Z"),
            ("t<=12:00:00,25Z", "t<=12:00:00.25Z"),
            ("tm==2024-01-01T00:00:00,5Z", "tm==2024-01-01T00:00:00.5Z"),
            (
                "tm==2024-01-01T00:00:00,5Z,2024-01-02T00:00:00,123456Z",
                "tm==2024-01-01T00:00:00.5Z,2024-01-02T00:00:00.123456Z",
            ),
            ("t!=11:00:00,5Z..13:00:00,5Z", "t!=11:00:00.5Z..13:00:00.5Z"),
        ] {
            let read = comma.parse::<Filter<Written>>().unwrap_or_else(|error| panic!("{comma}: {error}"));
            let written_with_a_point = point.parse::<Filter<Written>>().expect("the decimal point form parses");
            pretty_assertions::assert_eq!(read.to_string(), written_with_a_point.to_string(), "{comma}");
            pretty_assertions::assert_eq!(read, written_with_a_point, "{comma}");
        }
    }

    /// Clause 4.9's `quotedStr` is the IETF RFC 8259 `String` production
    /// *including its quotes*, so the quoted spelling of the same literal is a
    /// well-formed Query Term whatever it contains, and the value compares as
    /// the text the write path stored for it.
    #[test]
    fn a_quoted_operand_that_satisfies_no_temporal_form_compares_as_text() {
        match "observed>=\"2017-12-24T99:00:00Z\""
            .parse::<Filter<Written>>()
            .expect("a quotedStr is always derivable")
        {
            Filter::Term(QueryTerm::Ordering { value: Scalar::Str(text), .. }) => {
                pretty_assertions::assert_eq!(text, "2017-12-24T99:00:00Z");
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a string operand, got {other:?}"),
        }
    }

    /// Clause 4.9: `quotedStr` is a String whatever it contains, so a quoted
    /// literal in a `DateTime` form is a String, and only the bare one is a
    /// `dateTime`.
    #[test]
    fn a_quoted_operand_in_a_temporal_form_is_a_string_and_a_bare_one_that_value() {
        match "observed>=\"2017-12-24T12:00:00Z\"".parse::<Filter<Written>>().expect("parses") {
            Filter::Term(QueryTerm::Ordering { value: Scalar::Str(_), .. }) => {}
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a string operand, got {other:?}"),
        }
        match "observed>=2017-12-24T12:00:00Z".parse::<Filter<Written>>().expect("parses") {
            Filter::Term(QueryTerm::Ordering {
                value: Scalar::DateTime(_), ..
            }) => {}
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected a date-time operand, got {other:?}"),
        }
    }

    #[test]
    fn an_invalid_regex_pattern_is_rejected_when_the_filter_is_parsed() {
        // Clause 4.9: the operand of `~=` is a regular expression. An
        // unterminated character class is not one, and the request is rejected
        // rather than turned into a filter that matches nothing.
        let error = "name~=\"[\"".parse::<Filter<Written>>().expect_err("must reject");
        assert!(matches!(error, FilterError::Pattern(PatternError::Syntax(_))), "got {error:?}");

        let error = "name!~=\"[\"".parse::<Filter<Written>>().expect_err("must reject");
        assert!(matches!(error, FilterError::Pattern(PatternError::Syntax(_))), "got {error:?}");
    }

    #[test]
    fn parse_dotted_value_path() {
        let filter = "temperature.observedAt>=2017-12-24T12:00:00Z".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Ordering { path, operator, value }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "temperature");
                pretty_assertions::assert_eq!(
                    path.terminal().sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
                    vec!["observedAt"]
                );
                pretty_assertions::assert_eq!(operator, OrderingOperator::GreaterOrEqual);
                match value {
                    Scalar::DateTime(dt) => {
                        pretty_assertions::assert_eq!(instant_text(dt), "2017-12-24T12:00:00Z");
                    }
                    other @ (Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::Date(_) | Scalar::Time(_)) => {
                        panic!("expected DateTime scalar, got {other:?}")
                    }
                }
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_compound_value_path() {
        let filter = "address[city]==\"Berlin\"".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Equality { path, .. }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "address");
                pretty_assertions::assert_eq!(
                    path.terminal()
                        .members()
                        .expect("a member path")
                        .addressed
                        .iter()
                        .map(AttributeName::as_str)
                        .collect::<Vec<_>>(),
                    vec!["city"]
                );
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_nested_compound_value_path() {
        let filter = "sensor.rawdata[airquality.particulate]==40".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Equality { path, .. }) => {
                pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "sensor");
                pretty_assertions::assert_eq!(
                    path.terminal().sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
                    vec!["rawdata"]
                );
                pretty_assertions::assert_eq!(
                    path.terminal()
                        .members()
                        .expect("a member path")
                        .addressed
                        .iter()
                        .map(AttributeName::as_str)
                        .collect::<Vec<_>>(),
                    vec!["airquality", "particulate"]
                );
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_less_equal() {
        let filter = "capacity<=10".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Ordering { operator, .. }) => {
                pretty_assertions::assert_eq!(operator, OrderingOperator::LessOrEqual);
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_complex_mixed() {
        let filter = "(temperature>=20;temperature<=25)|capacity<=10".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Or(filters) => {
                pretty_assertions::assert_eq!(filters.len(), 2);
                match &filters[0] {
                    Filter::And(and_filters) => pretty_assertions::assert_eq!(and_filters.len(), 2),
                    other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected And, got {other:?}"),
                }
            }
            other @ (Filter::And(_) | Filter::Term(_)) => panic!("expected Or, got {other:?}"),
        }
    }

    #[test]
    fn parse_uri_equality() {
        let filter = "isParked==\"urn:ngsi-ld:OffStreetParking:Downtown1\"".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Equality { value, .. }) => match value {
                EqualityValue::Scalar(Scalar::Str(s)) => {
                    pretty_assertions::assert_eq!(s, "urn:ngsi-ld:OffStreetParking:Downtown1");
                }
                other @ (EqualityValue::Scalar(_) | EqualityValue::Range { .. } | EqualityValue::ValueList(_)) => panic!("expected Str, got {other:?}"),
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn parse_negative_number() {
        let filter = "temperature>-5".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Ordering { value, .. }) => match value {
                Scalar::Number(n) => {
                    pretty_assertions::assert_eq!(n, JsonNumber::from(-5));
                }
                other @ (Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_)) => {
                    panic!("expected Number, got {other:?}")
                }
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Ordering, got {other:?}"),
        }
    }

    #[test]
    fn parse_float_number() {
        let filter = "temperature==23.5".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Equality { value, .. }) => match value {
                EqualityValue::Scalar(Scalar::Number(n)) => {
                    pretty_assertions::assert_eq!(n, JsonNumber::try_from(23.5).expect("a finite number"));
                }
                other @ (EqualityValue::Scalar(_) | EqualityValue::Range { .. } | EqualityValue::ValueList(_)) => panic!("expected Number, got {other:?}"),
            },
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn exact_fraction_and_range_failure_are_typed() {
        let filter = "temperature==0.100000000000000005".parse::<Filter<Written>>().expect("supported exact number");
        let Filter::Term(QueryTerm::Equality {
            value: EqualityValue::Scalar(Scalar::Number(number)),
            ..
        }) = filter
        else {
            panic!("expected numeric equality");
        };
        pretty_assertions::assert_eq!(number.to_string(), "0.100000000000000005");
        assert!(matches!(
            "temperature==1e-16384".parse::<Filter<Written>>(),
            Err(FilterError::InvalidNumber { .. })
        ));
    }

    #[test]
    fn parse_boolean_value() {
        let filter = "active==true".parse::<Filter<Written>>().unwrap();
        match filter {
            Filter::Term(QueryTerm::Equality { value, .. }) => {
                pretty_assertions::assert_eq!(value, EqualityValue::Scalar(Scalar::Bool(true)));
            }
            other @ (Filter::And(_) | Filter::Or(_) | Filter::Term(_)) => panic!("expected Equality, got {other:?}"),
        }
    }

    #[test]
    fn error_on_unterminated_string() {
        let result = "name==\"hello".parse::<Filter<Written>>();
        assert!(result.is_err());
    }

    #[test]
    fn an_operator_where_an_attribute_name_is_required_is_refused_as_a_missing_name() {
        let error = "==".parse::<Filter<Written>>().expect_err("no attribute name opens the filter");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::AttributeName,
                    found: Found::Character('='),
                    position: 0
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn input_after_a_complete_filter_is_refused_at_its_first_character() {
        let error = "a==1 b".parse::<Filter<Written>>().expect_err("input continues after the filter");
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: 'b', position: 5 }),
            "got {error:?}"
        );
    }

    /// The filter `text` parses to.
    fn parsed(text: &str) -> Filter<Written> {
        text.parse::<Filter<Written>>().unwrap_or_else(|error| panic!("{text}: {error:?}"))
    }

    /// The text `text` reads back as.
    fn read_back(text: &str) -> String {
        parsed(text).to_string()
    }

    /// The filter of one existence term over a plain attribute name.
    fn exists(name: &str) -> Filter<Written> {
        Filter::Term(QueryTerm::Exists(AttributePath::Local(ValuePath::simple(
            name.parse().expect("valid attribute name"),
        ))))
    }

    /// The equality value of a filter holding one equality term.
    fn equality_of(text: &str) -> EqualityValue {
        let Filter::Term(QueryTerm::Equality { value, .. }) = parsed(text) else {
            panic!("{text}: expected one equality term");
        };
        value
    }

    /// A number operand written as `text`.
    fn decimal(text: &str) -> Scalar {
        Scalar::Number(text.parse::<JsonNumber>().expect("a number"))
    }

    /// A bare time value operand written as `text`.
    fn time_value(text: &str) -> Scalar {
        Scalar::from_literal(text, Quoting::Bare).expect("a clause 4.6.3 time value")
    }

    /// Every spelling reads back as itself.
    fn assert_reads_back_as_written(texts: &[&str]) {
        for text in texts {
            pretty_assertions::assert_eq!(read_back(text), *text, "{text}");
        }
    }

    #[test]
    fn every_operator_of_clause_four_nine_parses_to_its_query_term() {
        let path = || AttributePath::Local(ValuePath::simple("a".parse().expect("valid attribute name")));
        let equality = |membership| QueryTerm::Equality {
            path: path(),
            membership,
            value: EqualityValue::Scalar(number(1)),
        };
        let ordering = |operator| QueryTerm::Ordering {
            path: path(),
            operator,
            value: number(1),
        };
        let pattern = || "x".parse::<Pattern>().expect("a pattern");
        for (text, expected) in [
            ("a==1", equality(SetMembership::Within)),
            ("a!=1", equality(SetMembership::Outside)),
            ("a>1", ordering(OrderingOperator::Greater)),
            ("a>=1", ordering(OrderingOperator::GreaterOrEqual)),
            ("a<1", ordering(OrderingOperator::Less)),
            ("a<=1", ordering(OrderingOperator::LessOrEqual)),
            (
                r#"a~="x""#,
                QueryTerm::MatchPattern {
                    path: path(),
                    pattern: pattern(),
                },
            ),
            (
                r#"a!~="x""#,
                QueryTerm::NotMatchPattern {
                    path: path(),
                    pattern: pattern(),
                },
            ),
        ] {
            pretty_assertions::assert_eq!(parsed(text), Filter::Term(expected), "{text}");
            pretty_assertions::assert_eq!(read_back(text), text, "{text}");
        }
    }

    #[test]
    fn every_structural_character_has_its_production() {
        pretty_assertions::assert_eq!(parsed("(a;b)|c"), Filter::Or(vec![Filter::And(vec![exists("a"), exists("b")]), exists("c")]));
        pretty_assertions::assert_eq!(read_back("(a;b)|c"), "a;b|c");

        let filter = parsed("a[b]");
        let members = local_of(term_of(&filter).path()).members().expect("a trailing member path");
        pretty_assertions::assert_eq!(members.addressed.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["b"]);

        let filter = parsed("r{a}");
        let (relationship, guard, inner) = hop_of(term_of(&filter).path());
        pretty_assertions::assert_eq!((relationship, guard, local_of(inner).attribute.as_str()), ("r", Vec::<&str>::new(), "a"));

        pretty_assertions::assert_eq!(equality_of("a==1,2"), EqualityValue::ValueList(vec![number(1), number(2)]));
    }

    #[test]
    fn a_linked_entity_traversal_reads_its_guard() {
        let text = r#"isParkedIn{Building,Store:category}=="x""#;
        let filter = parsed(text);
        let (relationship, guard, inner) = hop_of(term_of(&filter).path());
        pretty_assertions::assert_eq!(
            (relationship, guard, local_of(inner).attribute.as_str()),
            ("isParkedIn", vec!["Building", "Store"], "category")
        );
        pretty_assertions::assert_eq!(filter.to_string(), text);
    }

    #[test]
    fn whitespace_separates_tokens_without_meaning() {
        pretty_assertions::assert_eq!(parsed(" \t\n\r speed \t "), exists("speed"));
        assert!(matches!("".parse::<Filter<Written>>(), Err(FilterError::Empty)));
    }

    #[test]
    fn the_range_separator_wins_over_the_sub_attribute_separator() {
        pretty_assertions::assert_eq!(
            equality_of("a==1..5"),
            EqualityValue::Range {
                min: number(1),
                max: number(5)
            }
        );
        let filter = parsed("a.b");
        let term = term_of(&filter);
        assert!(matches!(term, QueryTerm::Exists(_)), "got {term:?}");
        pretty_assertions::assert_eq!(
            term.path().terminal().sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["b"]
        );
    }

    #[test]
    fn a_fractional_number_does_not_swallow_the_range_separator() {
        pretty_assertions::assert_eq!(
            equality_of("a==1.5..2.5"),
            EqualityValue::Range {
                min: decimal("1.5"),
                max: decimal("2.5")
            }
        );
    }

    #[test]
    fn numbers_carry_their_sign_fraction_and_exponent() {
        assert_reads_back_as_written(&["a==-3", "a==-3.25"]);
        pretty_assertions::assert_eq!(read_back("a==1e3"), "a==1000");
        pretty_assertions::assert_eq!(read_back("a==1.5E-3"), "a==0.0015");
        pretty_assertions::assert_eq!(read_back("a==-1.5e+3"), "a==-1500");
    }

    #[test]
    fn true_and_false_are_keywords() {
        pretty_assertions::assert_eq!(equality_of("a==true"), EqualityValue::Scalar(Scalar::Bool(true)));
        pretty_assertions::assert_eq!(equality_of("a==false"), EqualityValue::Scalar(Scalar::Bool(false)));
        assert!(matches!("a==truthy".parse::<Filter<Written>>(), Err(FilterError::BareWord)));
        let error = refusal_of("true==1");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::AttributeName,
                    found: Found::Character('t'),
                    position: 0
                }
            ),
            "got {error:?}"
        );
        pretty_assertions::assert_eq!(read_back(r#""true"==1"#), "true==1");
    }

    #[test]
    fn a_date_like_literal_is_one_operand() {
        assert!(matches!(equality_of("a==2017-12-24T12:00:00Z"), EqualityValue::Scalar(Scalar::DateTime(_))));
        assert!(matches!(equality_of("a==2017-12-24"), EqualityValue::Scalar(Scalar::Date(_))));
        assert!(matches!(
            "a==2017-12-24T12:00:00+01:00".parse::<Filter<Written>>(),
            Err(FilterError::InvalidDateTime(_))
        ));
    }

    #[test]
    fn a_date_range_stops_at_the_range_separator() {
        let EqualityValue::Range { min, max } = equality_of("a==2017-12-24..2017-12-25") else {
            panic!("a range");
        };
        assert!(matches!((min, max), (Scalar::Date(_), Scalar::Date(_))));
    }

    #[test]
    fn a_bare_time_of_day_is_one_operand() {
        assert!(matches!(equality_of("a==12:00:00Z"), EqualityValue::Scalar(Scalar::Time(_))));
        pretty_assertions::assert_eq!(read_back("a==12:00:00.500000Z"), "a==12:00:00.500Z");
        assert!(matches!(equality_of(r#"a=="12:00:00Z""#), EqualityValue::Scalar(Scalar::Str(_))));
    }

    #[test]
    fn a_comma_before_the_fraction_of_a_time_value_stays_in_the_operand() {
        for (written, read) in [
            ("a==2024-01-01T00:00:00,5Z", "a==2024-01-01T00:00:00.500Z"),
            ("a==12:00:00,123456Z", "a==12:00:00.123456Z"),
            ("tm>2024-01-01T00:00:00,5Z", "tm>2024-01-01T00:00:00.500Z"),
            ("t==11:00:00,5Z..13:00:00,25Z", "t==11:00:00.500Z..13:00:00.250Z"),
        ] {
            pretty_assertions::assert_eq!(read_back(written), read, "{written}");
        }
    }

    #[test]
    fn a_comma_after_a_time_value_still_separates_a_value_list() {
        for (written, read) in [
            (
                "t==2024-01-01T00:00:00,5Z,2024-01-02T00:00:00Z",
                "t==2024-01-01T00:00:00.500Z,2024-01-02T00:00:00Z",
            ),
            ("d==2024-01-15,2024-01-16", "d==2024-01-15,2024-01-16"),
            ("t==12:00:00Z,5e3", "t==12:00:00Z,5000"),
            ("t==12:00:00Z,5", "t==12:00:00Z,5"),
        ] {
            pretty_assertions::assert_eq!(read_back(written), read, "{written}");
            let EqualityValue::ValueList(values) = equality_of(written) else {
                panic!("{written}: a value list");
            };
            pretty_assertions::assert_eq!(values.len(), 2, "{written}");
        }
    }

    #[test]
    fn a_digit_run_that_no_separator_continues_is_a_number() {
        assert_reads_back_as_written(&["a==12", "a==12.5", "a==12..20"]);
        pretty_assertions::assert_eq!(equality_of("a==12"), EqualityValue::Scalar(number(12)));
        pretty_assertions::assert_eq!(equality_of("a==12.5"), EqualityValue::Scalar(decimal("12.5")));
        pretty_assertions::assert_eq!(
            equality_of("a==12..20"),
            EqualityValue::Range {
                min: number(12),
                max: number(20)
            }
        );
    }

    #[test]
    fn a_time_of_day_range_stops_at_the_range_separator() {
        pretty_assertions::assert_eq!(
            equality_of("a==09:00:00Z..17:00:00Z"),
            EqualityValue::Range {
                min: time_value("09:00:00Z"),
                max: time_value("17:00:00Z")
            }
        );
        assert!(matches!(time_value("09:00:00Z"), Scalar::Time(_)));
    }

    #[test]
    fn a_name_carries_the_punctuation_a_compact_name_needs() {
        pretty_assertions::assert_eq!(parsed("ngsi-ld:speed_1"), exists("ngsi-ld:speed_1"));
        pretty_assertions::assert_eq!(
            equality_of("a==urn:ngsi-ld:Vehicle:A4567"),
            EqualityValue::Scalar(Scalar::Uri("urn:ngsi-ld:Vehicle:A4567".to_owned()))
        );
    }

    #[test]
    fn a_dotted_iri_splits_where_a_name_stands() {
        assert!(matches!(
            "https://example.com/Vehicle#speed".parse::<Filter<Written>>(),
            Err(FilterError::InvalidAttributeName(_))
        ));
    }

    #[test]
    fn a_dotted_uri_after_an_equality_operator_is_one_operand() {
        pretty_assertions::assert_eq!(
            equality_of("p==http://example.org/a"),
            EqualityValue::Scalar(Scalar::Uri("http://example.org/a".to_owned()))
        );

        let outside = "p!=https://a.example/x?y=1&z=%20~@!$'*+";
        let Filter::Term(QueryTerm::Equality { membership, value, .. }) = parsed(outside) else {
            panic!("one equality term");
        };
        pretty_assertions::assert_eq!(membership, SetMembership::Outside);
        pretty_assertions::assert_eq!(value, EqualityValue::Scalar(Scalar::Uri(outside[3..].to_owned())));

        let Filter::And(parts) = parsed("p==http://a.org/x,http://b.org/y;q.r==1") else {
            panic!("a conjunction");
        };
        pretty_assertions::assert_eq!(parts.len(), 2);
        let Filter::Term(QueryTerm::Equality { value, .. }) = &parts[0] else {
            panic!("the first term is an equality");
        };
        pretty_assertions::assert_eq!(
            *value,
            EqualityValue::ValueList(vec![Scalar::Uri("http://a.org/x".to_owned()), Scalar::Uri("http://b.org/y".to_owned())])
        );
        pretty_assertions::assert_eq!(parts[1].to_string(), "q.r==1");

        pretty_assertions::assert_eq!(read_back("(p==urn:a.b)|q==urn:c"), "p==urn:a.b|q==urn:c");

        let error = refusal_of("p==urn:a.b..urn:c.d");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::ComparableValue,
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn a_dotted_word_after_an_ordering_or_pattern_operator_still_splits() {
        assert!(matches!("p>a.b".parse::<Filter<Written>>(), Err(FilterError::BareWord)));
        let error = refusal_of("p~=a.b");
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: '.', position: 4 }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_quoted_operand_is_taken_without_a_second_percent_decode() {
        for (text, body) in [(r#"a=="a%20b""#, "a%20b"), (r#"a=="a%3Bb""#, "a%3Bb"), (r#"a=="urn:x:a%2Fb""#, "urn:x:a%2Fb")] {
            pretty_assertions::assert_eq!(equality_of(text), EqualityValue::Scalar(Scalar::Str(body.to_owned())), "{text}");
        }
    }

    #[test]
    fn a_backslash_resolves_by_the_production_it_stands_in() {
        pretty_assertions::assert_eq!(equality_of(r#"a=="say \"hi\"""#), EqualityValue::Scalar(Scalar::Str("say \"hi\"".to_owned())));
        pretty_assertions::assert_eq!(equality_of(r#"a=="back\\slash""#), EqualityValue::Scalar(Scalar::Str(r"back\slash".to_owned())));
        let Filter::Term(QueryTerm::MatchPattern { pattern, .. }) = parsed(r#"a~="^example\.com$""#) else {
            panic!("a pattern term");
        };
        pretty_assertions::assert_eq!(pattern.as_str(), r"^example\.com$");
    }

    #[test]
    fn an_empty_quoted_operand_is_the_empty_string() {
        pretty_assertions::assert_eq!(equality_of("a==\"\""), EqualityValue::Scalar(Scalar::Str(String::new())));
    }

    #[test]
    fn a_quoted_operand_left_open_is_refused() {
        assert!(matches!(
            "name==\"unterminated".parse::<Filter<Written>>(),
            Err(FilterError::UnterminatedString)
        ));
    }

    #[test]
    fn a_character_no_operand_starts_with_is_refused_with_its_position() {
        for (text, character, position) in [("speed==@", '@', 7), ("a==-x", '-', 3)] {
            let error = refusal_of(text);
            assert!(
                matches!(error, FilterError::Expected { expected: Expectation::Value, found: Found::Character(found), position: at } if found == character && at == position),
                "{text}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_whole_filter_parses_into_its_terms() {
        let Filter::And(parts) = parsed(r#"temperature.observedAt>=2017-12-24T12:00:00Z;name~="^A.*""#) else {
            panic!("a conjunction");
        };
        pretty_assertions::assert_eq!(parts.len(), 2);
        let QueryTerm::Ordering { path, operator, value } = term_of(&parts[0]) else {
            panic!("an ordering term");
        };
        pretty_assertions::assert_eq!(path.terminal().attribute.as_str(), "temperature");
        pretty_assertions::assert_eq!(
            path.terminal().sub_attributes.iter().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["observedAt"]
        );
        pretty_assertions::assert_eq!(*operator, OrderingOperator::GreaterOrEqual);
        assert!(matches!(value, Scalar::DateTime(_)), "got {value:?}");
        let QueryTerm::MatchPattern { path, pattern } = term_of(&parts[1]) else {
            panic!("a pattern term");
        };
        pretty_assertions::assert_eq!((path.terminal().attribute.as_str(), pattern.as_str()), ("name", "^A.*"));
    }

    #[test]
    fn a_compound_value_path_reads_its_member() {
        let text = r#"address[city]=="Berlin""#;
        let filter = parsed(text);
        let QueryTerm::Equality { path, value, .. } = term_of(&filter) else {
            panic!("an equality term");
        };
        let members = path.terminal().members().expect("a trailing member path");
        pretty_assertions::assert_eq!(members.addressed.iter().map(AttributeName::as_str).collect::<Vec<_>>(), vec!["city"]);
        pretty_assertions::assert_eq!(*value, EqualityValue::Scalar(Scalar::Str("Berlin".to_owned())));
    }

    #[test]
    fn a_wildcard_subtag_joins_its_language_range() {
        for text in ["name[de-*-DE]", "name[*-GB]", "name[en-*]"] {
            let filter = parsed(text);
            assert!(
                matches!(
                    &term_of(&filter).path().terminal().trailing,
                    Some(TrailingGroup::Language(TargetLanguage::Range(_)))
                ),
                "{text}"
            );
        }
        let filter = parsed("name[en-GB]");
        assert!(matches!(
            &term_of(&filter).path().terminal().trailing,
            Some(TrailingGroup::Language(TargetLanguage::Tag(_)))
        ));
        let filter = parsed("name[*]");
        pretty_assertions::assert_eq!(term_of(&filter).path().terminal().trailing, Some(TrailingGroup::AllLanguages));
        let filter = parsed("name[en]");
        assert!(matches!(&term_of(&filter).path().terminal().trailing, Some(TrailingGroup::Members(_))));
        assert_reads_back_as_written(&["name[de-*-DE]", "name[*-GB]", "name[en-*]", "name[en-GB]", "name[*]", "name[en]"]);
    }

    /// The refusal of a text that is no `q` filter.
    fn refusal_of(text: &str) -> FilterError {
        text.parse::<Filter<Written>>().expect_err("outside the clause 4.9 grammar")
    }

    /// A character that no production admits is expected as what the grammar required where it stands.
    #[test]
    fn a_character_no_production_admits_is_expected_as_what_the_grammar_required() {
        let error = refusal_of(r"a{=1");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::AttributeName,
                    found: Found::Character('='),
                    position: 2
                }
            ),
            "got {error:?}"
        );
    }

    /// A guard whose last bare type holds two colons is refused where it stands, before a later character that opens nothing.
    #[test]
    fn an_ambiguous_guard_is_refused_before_a_later_stray_character() {
        let error = refusal_of(
            r#"((isParked{r{Vehicle:ex:q"}}~="[0-9]+";"ex:q".brandName.ex:name<=9007199254740993)|"ex:q".speed[en.en]|r{link{r{brandName[*]}}}~="\\d")"#,
        );
        assert!(matches!(error, FilterError::AmbiguousTypeGuard), "got {error:?}");
    }

    /// A bare operand in no clause 4.6.3 form and no URI is refused as a bare word, before a later stray character.
    #[test]
    fn a_bare_word_operand_is_refused_before_a_later_stray_character() {
        let error = refusal_of(r"a==2017-1%2-24T12:00:00Z");
        assert!(matches!(error, FilterError::BareWord), "got {error:?}");
    }

    /// A character that opens no operand is expected as a value where it stands, before a later stray character.
    #[test]
    fn a_missing_operand_is_refused_before_a_later_stray_character() {
        let error = refusal_of(r#"a==[a%20b""#);
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Value,
                    found: Found::Character('['),
                    position: 3
                }
            ),
            "got {error:?}"
        );
    }

    /// A name outside the clause 4.6.2 grammar is refused as that name, before a later stray character.
    #[test]
    fn an_invalid_name_is_refused_before_a_later_stray_character() {
        let error = refusal_of(r"ngsi-ld=:speed_1");
        assert!(
            matches!(error, FilterError::InvalidAttributeName(NgsiLdReferenceError::Invalid)),
            "got {error:?}"
        );
    }

    /// A bare date-time operand that stops at a stray character is refused as the malformed date-time it is.
    #[test]
    fn a_malformed_date_time_is_refused_before_a_later_stray_character() {
        let error = refusal_of(r"a==2017-12-24T12:00:00%");
        assert!(
            matches!(error, FilterError::InvalidDateTime(NgsiLdDateTimeError::Lexical(LexicalError::Malformed))),
            "got {error:?}"
        );
    }

    /// A bare time operand that stops at a stray character is refused as the malformed time it is.
    #[test]
    fn a_malformed_time_is_refused_before_a_later_stray_character() {
        let error = refusal_of(r"t==11:00:00,5Z.=13:00:00,25Z");
        assert!(
            matches!(error, FilterError::InvalidTime(NgsiLdTimeError::Lexical(LexicalError::Malformed))),
            "got {error:?}"
        );
    }

    /// A one-member bracket that is neither a name nor a language tag or range is refused as that member, before a later stray character.
    #[test]
    fn a_trailing_member_that_is_no_name_or_language_is_refused_before_a_later_stray_character() {
        let error = refusal_of(
            r#"(r{Building,Vehicle,Building:r{Building,Vehicle,T1:brandName.a[*]}}!=http://example.com/a;(isParked{"ex:q.brandName.observedAt[zip]}~="[0-9]+";r{Building:observedAt.x_1};"ex:q".observedAt~="a.*"))"#,
        );
        assert!(
            matches!(&error, FilterError::InvalidTrailingMember { tag, range: LanguageRangeError::FirstSubtag } if tag.0 == "0-9"),
            "got {error:?}"
        );
    }

    /// A pattern operand the regular-expression syntax refuses is refused as that pattern, before a later stray character.
    #[test]
    fn a_pattern_that_is_no_regular_expression_is_refused_before_a_later_stray_character() {
        let error = refusal_of(
            r#"((speed.ex:name==false,false,0|"https://example.com/q".x_1<=""|r{"ex:q"."ex:q"[*]}==2024-01-01..2024-12-31)|link{isParked{x_1."https://example.com/q"}}!=http://example.com/a|("https://example.com/q"~="a.*/|link{x_1.observedAt[*]}~="\\d"|a.speed[*]))"#,
        );
        assert!(matches!(error, FilterError::Pattern(PatternError::Syntax(_))), "got {error:?}");
    }

    /// Input continuing after a complete filter is refused at its first character, before a later stray character.
    #[test]
    fn a_character_after_a_complete_filter_is_reported_before_a_later_stray_one() {
        let error = refusal_of(r"a)=1");
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: ')', position: 1 }),
            "got {error:?}"
        );
    }

    /// A bare operand in no clause 4.6.3 form and no URI is refused as a bare word, before a later quotation mark that is never closed.
    #[test]
    fn a_bare_word_operand_is_refused_before_a_later_unterminated_quotation_mark() {
        let error = refusal_of(r#"a==x"y!"#);
        assert!(matches!(error, FilterError::BareWord), "got {error:?}");
    }

    /// A quotation mark inside a trailing group is refused where the group required its closing bracket.
    #[test]
    fn a_closing_bracket_is_expected_before_a_later_unterminated_quotation_mark() {
        let error = refusal_of(r#"address[c"ity]=="Berlin""#);
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Punctuation(Punctuation::CloseBracket),
                    found: Found::Character('"'),
                    position: 9
                }
            ),
            "got {error:?}"
        );
    }

    /// An empty quoted name is refused as an empty name, before a later quotation mark that is never closed.
    #[test]
    fn an_empty_quoted_name_is_refused_before_a_later_unterminated_quotation_mark() {
        let error = refusal_of(r#"""a b"==1"#);
        assert!(matches!(error, FilterError::InvalidAttributeName(NgsiLdReferenceError::Empty)), "got {error:?}");
    }

    /// A name outside the clause 4.6.2 grammar is refused as that name, before a later quotation mark that is never closed.
    #[test]
    fn an_invalid_name_is_refused_before_a_later_unterminated_quotation_mark() {
        let error = refusal_of(r#"a[en-*-*"]"#);
        assert!(
            matches!(error, FilterError::InvalidAttributeName(NgsiLdReferenceError::Invalid)),
            "got {error:?}"
        );
    }

    /// A pattern operand the regular-expression syntax refuses is refused as that pattern, before a later quotation mark that is never closed.
    #[test]
    fn a_pattern_that_is_no_regular_expression_is_refused_before_a_later_unterminated_quotation_mark() {
        let error = refusal_of(r#"(link{isParked{Building,Building:link{brandName.x_1.ex:name}}}~="\\d-|r{isParked{Building,T1,T1:speed[*]}}!~="a.*")"#);
        assert!(matches!(error, FilterError::Pattern(PatternError::Syntax(_))), "got {error:?}");
    }

    /// Input continuing after a complete filter is refused at its first character, before a quotation mark that is never closed.
    #[test]
    fn a_character_after_a_complete_filter_is_reported_before_a_later_unterminated_quotation_mark() {
        let error = refusal_of(r#"a=="x"y""#);
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: 'y', position: 6 }),
            "got {error:?}"
        );
    }

    /// The text a filter is written back as, read again: a name holding a character no bare word carries stops the path where the closing brace was required.
    #[test]
    fn a_stored_filter_with_a_stray_character_inside_a_hop_is_refused_where_the_brace_was_required() {
        let error = refusal_of(
            r#"r{link{Building,Building,Building:r{Building,T1,T1:"https://example.com/q".observedAt.ex:%}}}<=10|r{T1,T1,Building:speed."https://example.com/q"[en.b]}!=-1,true,true;link{T1:r{isParked{Vehicle:observedAt."https://example.com/q"[*]}}}<9007199254740993;r{Vehicle,T1:isParked{Vehicle:"https://example.com/q"."https://example.com/q"[*]}}"#,
        );
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Punctuation(Punctuation::CloseBrace),
                    found: Found::Character('%'),
                    position: 89
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn a_group_past_the_ceiling_is_refused_before_a_later_lexical_fault() {
        for text in [
            format!("{}a@", "(".repeat(WRITTEN_NESTING_CEILING.get() + 1)),
            format!("{}a\"", "(".repeat(WRITTEN_NESTING_CEILING.get() + 1)),
        ] {
            let error = refusal_of(&text);
            assert!(
                matches!(error, FilterError::NestedTooDeep { depth: 65, limit } if limit == WRITTEN_NESTING_CEILING),
                "{text}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_position_counts_bytes_of_the_text() {
        let error = "ä==@".parse::<Filter<Written>>().expect_err("`@` opens no value");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Value,
                    found: Found::Character('@'),
                    position: 4
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn a_lexical_fault_after_a_syntax_fault_is_not_reported_first() {
        let error = "a==1 b \"x".parse::<Filter<Written>>().expect_err("`b` continues a complete filter");
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: 'b', position: 5 }),
            "got {error:?}"
        );
    }

    #[test]
    fn the_guard_colon_keeps_its_whitespace_behaviour() {
        for written in ["sensor{Device: humidity}==40", "sensor{\"Device\" :humidity}==40"] {
            let filter = written.parse::<Filter<Written>>().unwrap_or_else(|error| panic!("{written}: {error:?}"));
            pretty_assertions::assert_eq!(filter.to_string(), "sensor{Device:humidity}==40", "{written}");
        }
        let error = "sensor{Device :humidity}==40"
            .parse::<Filter<Written>>()
            .expect_err("a space before a bare type's colon ends the guard's reading");
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
    fn an_escape_position_counts_in_the_whole_filter() {
        let error = "a==\"\\q\"".parse::<Filter<Written>>().expect_err("`\\q` is not a JSON escape");
        assert!(matches!(error, FilterError::InvalidStringEscape { escape: 'q', position: 4 }), "got {error:?}");
    }

    /// The hop this attribute path descends: its Relationship, its type guard, and
    /// the inner attribute path, so a test states the shape it expects without
    /// matching a nested enum by hand.
    fn hop_of(path: &AttributePath<Written>) -> (&str, Vec<&str>, &AttributePath<Written>) {
        match path {
            AttributePath::Linked {
                relationship,
                guard_types,
                inner,
            } => (
                relationship.as_str(),
                guard_types.iter().map(EntityTypeName::as_str).collect::<Vec<&str>>(),
                inner,
            ),
            AttributePath::Local(local) => panic!("expected a linked-entity hop, got the local path {local:?}"),
        }
    }

    fn local_of(path: &AttributePath<Written>) -> &ValuePath<Written> {
        match path {
            AttributePath::Local(local) => local,
            AttributePath::Linked { relationship, .. } => panic!("expected a local path, got a hop through {relationship}"),
        }
    }

    fn term_of(filter: &Filter<Written>) -> &QueryTerm<Written> {
        match filter {
            Filter::Term(term) => term,
            other @ (Filter::And(_) | Filter::Or(_)) => panic!("expected a single query term, got {other:?}"),
        }
    }

    #[test]
    fn a_comparison_after_a_hop_applies_to_the_linked_entity() {
        // Clause 4.9 `QueryTerm =/ Attribute equal CompEqualityValue` with
        // `Attribute = LinkedEntityRelation`: the braces close over the path and
        // the comparison follows them (EXAMPLE 13, `sensor{humidity}==40`).
        let filter = "isParkedIn{category}==\"commercial\"".parse::<Filter<Written>>().expect("parses");
        match term_of(&filter) {
            QueryTerm::Equality { path, membership, value } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
                assert!(guard.is_empty(), "no type guard was written: {guard:?}");
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "category");
                pretty_assertions::assert_eq!(*membership, SetMembership::Within);
                pretty_assertions::assert_eq!(*value, EqualityValue::Scalar(Scalar::Str("commercial".to_string())));
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality query term, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_carrying_two_comma_separated_types_parses_them_as_its_guard() {
        // The clause 4.9 guard production is `*1(EntityType 1*("," EntityType) ":")`,
        // so two or more types followed by a colon constrain the linked entity.
        let filter = "isParkedIn{Building,Store:category}==\"x\"".parse::<Filter<Written>>().expect("parses");
        match term_of(&filter) {
            QueryTerm::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
                pretty_assertions::assert_eq!(guard, vec!["Building", "Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "category");
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality query term, got {other:?}")
            }
        }
    }

    #[test]
    fn hops_nest_and_the_innermost_one_carries_the_comparison() {
        let filter = "a{b{c}}==5".parse::<Filter<Written>>().expect("parses");
        match term_of(&filter) {
            QueryTerm::Equality { path, value, .. } => {
                let (outer, outer_guard, inner_path) = hop_of(path);
                pretty_assertions::assert_eq!(outer, "a");
                assert!(outer_guard.is_empty(), "{outer_guard:?}");
                let (inner, inner_guard, terminal) = hop_of(inner_path);
                pretty_assertions::assert_eq!(inner, "b");
                assert!(inner_guard.is_empty(), "{inner_guard:?}");
                pretty_assertions::assert_eq!(local_of(terminal).attribute.as_str(), "c");
                pretty_assertions::assert_eq!(*value, EqualityValue::Scalar(Scalar::Number(5.into())));
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality query term, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_with_no_operator_inside_is_an_existence_check_on_the_linked_entity() {
        let filter = "a{b}".parse::<Filter<Written>>().expect("parses");
        match term_of(&filter) {
            QueryTerm::Exists(path) => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "a");
                assert!(guard.is_empty(), "{guard:?}");
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "b");
            }
            other @ (QueryTerm::Equality { .. } | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an existence query term, got {other:?}")
            }
        }
    }

    /// Clause 4.9 EXAMPLE 14 writes a guard of one type,
    /// `sensor{Device:humidity}`, and no `AttrName` or `EntityType` of its
    /// grammar holds a colon, so a name before a colon at the head of a hop is
    /// the one type of its guard, written bare or quoted.
    #[test]
    fn a_single_name_before_a_colon_inside_a_hop_is_a_type_guard() {
        for (q, relationship, guard, attribute) in [
            ("sensor{Device:humidity}==40", "sensor", "Device", "humidity"),
            ("sensor{Device: humidity}==40", "sensor", "Device", "humidity"),
            ("a{prefix:name}==\"v\"", "a", "prefix", "name"),
            (
                "\"https://example.com/sensor\"{\"https://example.com/Device\":\"https://example.com/humidity\"}==40",
                "https://example.com/sensor",
                "https://example.com/Device",
                "https://example.com/humidity",
            ),
            ("sensor{\"ex:Device\":humidity}==40", "sensor", "ex:Device", "humidity"),
        ] {
            let filter = q.parse::<Filter<Written>>().unwrap_or_else(|error| panic!("{q}: {error}"));
            let (read_relationship, read_guard, inner) = hop_of(term_of(&filter).path());
            pretty_assertions::assert_eq!(
                (read_relationship, read_guard, local_of(inner).attribute.as_str()),
                (relationship, vec![guard], attribute),
                "{q}"
            );
        }
    }

    /// The guard of one type opens each hop of a chain, outermost first.
    #[test]
    fn each_hop_of_a_chain_takes_a_guard_of_one_type() {
        let filter = "r{T1:s{T2:a}}==1".parse::<Filter<Written>>().expect("parses");
        let (relationship, guard, inner) = hop_of(term_of(&filter).path());
        pretty_assertions::assert_eq!((relationship, guard), ("r", vec!["T1"]));
        let (relationship, guard, inner) = hop_of(inner);
        pretty_assertions::assert_eq!((relationship, guard), ("s", vec!["T2"]));
        pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "a");
    }

    /// A prefixed attribute name at the head of a hop is written quoted, since
    /// its colon would otherwise end a type guard.
    #[test]
    fn a_quoted_prefixed_name_inside_a_hop_is_an_attribute_name() {
        let filter = "a{\"prefix:name\"}==\"v\"".parse::<Filter<Written>>().expect("parses");
        let (relationship, guard, inner) = hop_of(term_of(&filter).path());
        pretty_assertions::assert_eq!(relationship, "a");
        assert!(guard.is_empty(), "a quoted name is no guard: {guard:?}");
        pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "prefix:name");
    }

    /// A bare name at the head of a hop holding two colons reads equally well
    /// as a prefixed type before an attribute or a type before a prefixed
    /// attribute, so it is refused and the client quotes what it meant.
    #[test]
    fn a_bare_name_with_two_colons_at_the_head_of_a_hop_is_ambiguous() {
        let error = "rel{ex:Store:category}==\"x\"".parse::<Filter<Written>>().expect_err("must reject");
        assert!(matches!(error, FilterError::AmbiguousTypeGuard), "got {error:?}");
    }

    #[test]
    fn a_traversal_term_combines_with_a_local_one_through_the_conjunction() {
        let filter = "speed>50;isParkedIn{category}==\"x\"".parse::<Filter<Written>>().expect("parses");
        match filter {
            Filter::And(filters) => {
                pretty_assertions::assert_eq!(filters.len(), 2);
                match term_of(&filters[0]) {
                    QueryTerm::Ordering { path, .. } => pretty_assertions::assert_eq!(local_of(path).attribute.as_str(), "speed"),
                    other @ (QueryTerm::Exists(_) | QueryTerm::Equality { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                        panic!("expected an ordering query term, got {other:?}")
                    }
                }
                let (relationship, _, _) = hop_of(term_of(&filters[1]).path());
                pretty_assertions::assert_eq!(relationship, "isParkedIn");
            }
            other @ (Filter::Or(_) | Filter::Term(_)) => panic!("expected a conjunction, got {other:?}"),
        }
    }

    #[test]
    fn a_guard_type_that_is_not_the_last_one_keeps_the_colons_it_carries() {
        // Only the final type can carry the guard's terminating colon, because a
        // comma says another type follows. A prefixed type written before the
        // last one is therefore unambiguous and must not be split.
        let filter = "rel{ex:A,Store:category}==\"x\"".parse::<Filter<Written>>().expect("parses");
        match term_of(&filter) {
            QueryTerm::Equality { path, .. } => {
                let (relationship, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(relationship, "rel");
                pretty_assertions::assert_eq!(guard, vec!["ex:A", "Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "category");
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality query term, got {other:?}")
            }
        }
    }

    #[test]
    fn a_final_guard_type_carrying_two_colons_is_rejected_rather_than_split_by_guesswork() {
        // `ex:Store:category` could be the prefixed type `ex:Store` followed by
        // `category`, or the type `ex` followed by the prefixed attribute
        // `Store:category`. Nothing in the clause 4.9 grammar decides between
        // them, so the request is rejected and the client quotes what it meant.
        let error = "rel{Building,ex:Store:category}==\"x\"".parse::<Filter<Written>>().expect_err("must reject");
        assert!(matches!(error, FilterError::AmbiguousTypeGuard), "got {error:?}");
    }

    #[test]
    fn quoting_a_guard_type_resolves_the_ambiguity_its_colons_would_create() {
        let filter = "rel{Building,\"ex:Store\":category}==\"x\"".parse::<Filter<Written>>().expect("parses");
        match term_of(&filter) {
            QueryTerm::Equality { path, .. } => {
                let (_, guard, inner) = hop_of(path);
                pretty_assertions::assert_eq!(guard, vec!["Building", "ex:Store"]);
                pretty_assertions::assert_eq!(local_of(inner).attribute.as_str(), "category");
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Ordering { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an equality query term, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_left_unclosed_is_rejected() {
        let error = "a{b".parse::<Filter<Written>>().expect_err("must reject");
        assert!(
            matches!(
                error,
                FilterError::Expected {
                    expected: Expectation::Punctuation(Punctuation::CloseBrace),
                    found: Found::EndOfInput,
                    position: 3
                }
            ),
            "got {error:?}"
        );
    }

    /// Clause 4.9 puts no Operator inside a hop's braces: `LinkedEntityPath`
    /// ends in a `ValuePath`, so the closing brace is expected where the
    /// comparison starts.
    #[test]
    fn a_comparison_written_inside_the_braces_is_rejected() {
        for (written, character, position) in [
            ("isParkedIn{category==\"commercial\"}", '=', 19),
            ("a{b{c>5}}", '>', 5),
            ("a{b~=\"x\"}", '~', 3),
        ] {
            let error = written.parse::<Filter<Written>>().expect_err("must reject");
            assert!(
                matches!(
                    error,
                    FilterError::Expected {
                        expected: Expectation::Punctuation(Punctuation::CloseBrace),
                        found: Found::Character(found),
                        position: at
                    } if found == character && at == position
                ),
                "{written}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_hop_ending_in_a_compound_member_compares_that_member() {
        let filter = "a{b[c.d]}>=1".parse::<Filter<Written>>().expect("parses");
        match term_of(&filter) {
            QueryTerm::Ordering { path, operator, .. } => {
                let (_, _, inner) = hop_of(path);
                pretty_assertions::assert_eq!(
                    local_of(inner)
                        .members()
                        .expect("a member path")
                        .addressed
                        .iter()
                        .map(AttributeName::as_str)
                        .collect::<Vec<_>>(),
                    vec!["c", "d"]
                );
                pretty_assertions::assert_eq!(*operator, OrderingOperator::GreaterOrEqual);
            }
            other @ (QueryTerm::Exists(_) | QueryTerm::Equality { .. } | QueryTerm::MatchPattern { .. } | QueryTerm::NotMatchPattern { .. }) => {
                panic!("expected an ordering query term, got {other:?}")
            }
        }
    }

    #[test]
    fn a_hop_with_nothing_inside_it_is_rejected() {
        // Clause 4.9 requires a `LinkedEntityPath` between the braces.
        let error = "a{}".parse::<Filter<Written>>().expect_err("must reject");
        assert!(matches!(error, FilterError::Expected { .. }), "got {error:?}");
    }

    #[test]
    fn hop_nesting_is_bounded_by_the_same_budget_as_sub_attribute_nesting() {
        // A hop compiles to a correlated subquery per level, so an unbounded
        // chain is a denial-of-service vector rather than a legitimate query.
        let chain = |hops: usize| format!("{}z{}", "a{".repeat(hops), "}".repeat(hops));
        assert!(
            chain(MAX_SUB_ATTRIBUTE_DEPTH).parse::<Filter<Written>>().is_ok(),
            "a chain at the limit must parse"
        );
        assert!(
            matches!(
                chain(MAX_SUB_ATTRIBUTE_DEPTH + 1).parse::<Filter<Written>>(),
                Err(FilterError::PathTooDeep { .. })
            ),
            "an over-deep hop chain must be rejected"
        );
    }
}
