//! The grammar shape type selection (clause 4.17) and `scopeQ` (clause 4.19) share.
//!
//! ```text
//! Form   = OrItem *(orOp OrItem)
//! OrItem = "(" Item *(";" Item) ")" / Item
//! ```
//!
//! A conjunction stands only inside one pair of parentheses around plain items. Unicode whitespace is skipped around every item, parenthesis and operator. Each language supplies its item parser and spells the refusals through [`DisjunctiveFormErrors`].

use crate::query_text::{
    grammar_fault::GrammarFault,
    or_operator::{OR_OPERATORS, or_operator},
    whitespace::Whitespace,
};
use winnow::{
    Parser,
    combinator::{opt, separated},
    stream::{LocatingSlice, Location, Stream},
    token::one_of,
};

/// The refusals of a language read as a disjunctive form. The form decides which fault the text has; the language only spells it.
pub(crate) trait DisjunctiveFormErrors: Sized {
    /// An item is required at `position`; `found` stands there, or the text ends (`None`).
    fn missing_item(found: Option<char>, position: usize) -> Self;

    /// A `(` stands at `position` where an item of a group is required.
    fn group_in_group(position: usize) -> Self;

    /// An `orOp` or a `(` stands at `position` after an item of a group.
    fn nested_group(position: usize) -> Self;

    /// After an item of a group, the text ends or a character other than `;`, `)`, an `orOp` or `(` stands at `position`.
    fn unclosed_group(position: usize) -> Self;

    /// A `;` stands at `position` after a complete alternative, outside any group.
    fn ungrouped_conjunction(position: usize) -> Self;

    /// The form is complete and `found`, a character other than `;`, stands at `position`.
    fn trailing_input(found: char, position: usize) -> Self;
}

/// Whether `character` ends an item: an `orOp`, `;`, `(` or `)`.
pub(crate) fn delimits_item(character: char) -> bool {
    OR_OPERATORS.contains(&character) || matches!(character, ';' | '(' | ')')
}

/// Reads all of `text` as a disjunctive form of `item`s: the alternatives in order, each the items of its conjunction.
///
/// Positions in the refusals are byte offsets into `text`.
pub(crate) fn disjunctive_form<'i, Item, E>(text: &'i str, mut item: impl Parser<LocatingSlice<&'i str>, Item, GrammarFault<E>>) -> Result<Vec<Vec<Item>>, E>
where
    E: DisjunctiveFormErrors,
{
    let mut input = LocatingSlice::new(text);
    form(&mut item, &mut input).map_err(|fault| {
        fault.finish(|position| {
            let found = text.get(position..).and_then(|rest| rest.chars().next());
            E::missing_item(found, position)
        })
    })
}

/// Reads the alternatives and then requires the end of the text.
fn form<'i, Item, E>(
    item: &mut impl Parser<LocatingSlice<&'i str>, Item, GrammarFault<E>>,
    input: &mut LocatingSlice<&'i str>,
) -> Result<Vec<Vec<Item>>, GrammarFault<E>>
where
    E: DisjunctiveFormErrors,
{
    let alternatives = separated(1.., |input: &mut LocatingSlice<&'i str>| alternative(&mut *item, input), or_operator::<E>).parse_next(input)?;
    let position = input.current_token_start();
    match input.peek_token() {
        None => Ok(alternatives),
        Some(';') => Err(GrammarFault::Refused(E::ungrouped_conjunction(position))),
        Some(found) => Err(GrammarFault::Refused(E::trailing_input(found, position))),
    }
}

/// Reads one alternative: a group or a single item, with the whitespace around it.
fn alternative<'i, Item, E>(
    item: &mut impl Parser<LocatingSlice<&'i str>, Item, GrammarFault<E>>,
    input: &mut LocatingSlice<&'i str>,
) -> Result<Vec<Item>, GrammarFault<E>>
where
    E: DisjunctiveFormErrors,
{
    Whitespace::Unicode.skip::<E>().parse_next(input)?;
    let opened = opt(one_of::<_, _, GrammarFault<E>>('(')).parse_next(input)?;
    let items = if opened.is_some() {
        group(item, input)?
    } else {
        vec![required_item(item, input)?]
    };
    Whitespace::Unicode.skip::<E>().parse_next(input)?;
    Ok(items)
}

/// Reads the items of a group after its `(`, through its `)`.
fn group<'i, Item, E>(
    item: &mut impl Parser<LocatingSlice<&'i str>, Item, GrammarFault<E>>,
    input: &mut LocatingSlice<&'i str>,
) -> Result<Vec<Item>, GrammarFault<E>>
where
    E: DisjunctiveFormErrors,
{
    let items = separated(
        1..,
        |input: &mut LocatingSlice<&'i str>| group_item(&mut *item, input),
        one_of::<_, _, GrammarFault<E>>(';'),
    )
    .parse_next(input)?;
    let position = input.current_token_start();
    match input.peek_token() {
        Some(')') => {
            input.next_token();
            Ok(items)
        }
        Some(character) if character == '(' || OR_OPERATORS.contains(&character) => Err(GrammarFault::Refused(E::nested_group(position))),
        Some(_) | None => Err(GrammarFault::Refused(E::unclosed_group(position))),
    }
}

/// Reads one item of a group with the whitespace around it, refusing a nested `(`.
fn group_item<'i, Item, E>(
    item: &mut impl Parser<LocatingSlice<&'i str>, Item, GrammarFault<E>>,
    input: &mut LocatingSlice<&'i str>,
) -> Result<Item, GrammarFault<E>>
where
    E: DisjunctiveFormErrors,
{
    Whitespace::Unicode.skip::<E>().parse_next(input)?;
    if input.peek_token() == Some('(') {
        return Err(GrammarFault::Refused(E::group_in_group(input.current_token_start())));
    }
    let read = required_item(item, input)?;
    Whitespace::Unicode.skip::<E>().parse_next(input)?;
    Ok(read)
}

/// Runs the item parser, turning its backtrack into the refusal of a missing item at the position the item started.
fn required_item<'i, Item, E>(
    item: &mut impl Parser<LocatingSlice<&'i str>, Item, GrammarFault<E>>,
    input: &mut LocatingSlice<&'i str>,
) -> Result<Item, GrammarFault<E>>
where
    E: DisjunctiveFormErrors,
{
    let start = input.checkpoint();
    match item.parse_next(input) {
        Ok(read) => Ok(read),
        Err(GrammarFault::Refused(error)) => Err(GrammarFault::Refused(error)),
        Err(GrammarFault::Backtrack { .. }) => {
            input.reset(&start);
            Err(GrammarFault::Refused(E::missing_item(input.peek_token(), input.current_token_start())))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::query_text::{
        disjunctive_form::{DisjunctiveFormErrors, disjunctive_form},
        grammar_fault::GrammarFault,
    };
    use winnow::{Parser, stream::LocatingSlice, token::one_of};

    #[derive(Debug, PartialEq, Eq)]
    enum CaseError {
        MissingItem { found: Option<char>, position: usize },
        GroupInGroup { position: usize },
        NestedGroup { position: usize },
        UnclosedGroup { position: usize },
        UngroupedConjunction { position: usize },
        TrailingInput { found: char, position: usize },
    }

    impl DisjunctiveFormErrors for CaseError {
        fn missing_item(found: Option<char>, position: usize) -> Self {
            CaseError::MissingItem { found, position }
        }

        fn group_in_group(position: usize) -> Self {
            CaseError::GroupInGroup { position }
        }

        fn nested_group(position: usize) -> Self {
            CaseError::NestedGroup { position }
        }

        fn unclosed_group(position: usize) -> Self {
            CaseError::UnclosedGroup { position }
        }

        fn ungrouped_conjunction(position: usize) -> Self {
            CaseError::UngroupedConjunction { position }
        }

        fn trailing_input(found: char, position: usize) -> Self {
            CaseError::TrailingInput { found, position }
        }
    }

    fn letter(input: &mut LocatingSlice<&str>) -> Result<char, GrammarFault<CaseError>> {
        one_of('a'..='z').parse_next(input)
    }

    fn read(text: &str) -> Result<Vec<Vec<char>>, CaseError> {
        disjunctive_form(text, letter)
    }

    #[test]
    fn one_item_is_one_alternative_of_one_item() {
        pretty_assertions::assert_eq!(read("a"), Ok(vec![vec!['a']]));
    }

    #[test]
    fn or_operators_separate_alternatives() {
        pretty_assertions::assert_eq!(read("a|b,c"), Ok(vec![vec!['a'], vec!['b'], vec!['c']]));
    }

    #[test]
    fn a_group_is_one_alternative_of_its_items() {
        pretty_assertions::assert_eq!(read("(a;b)|c"), Ok(vec![vec!['a', 'b'], vec!['c']]));
    }

    #[test]
    fn a_group_of_one_item_is_one_alternative_of_that_item() {
        pretty_assertions::assert_eq!(read("(a)"), Ok(vec![vec!['a']]));
    }

    #[test]
    fn whitespace_around_items_parentheses_and_operators_is_skipped() {
        pretty_assertions::assert_eq!(read(" a | ( b ; c ) "), Ok(vec![vec!['a'], vec!['b', 'c']]));
    }

    #[test]
    fn a_conjunction_outside_a_group_is_refused() {
        pretty_assertions::assert_eq!(read("a;b"), Err(CaseError::UngroupedConjunction { position: 1 }));
    }

    #[test]
    fn a_disjunction_or_a_group_after_an_item_of_a_group_is_refused() {
        pretty_assertions::assert_eq!(read("(a|b)"), Err(CaseError::NestedGroup { position: 2 }));
        pretty_assertions::assert_eq!(read("(a(b))"), Err(CaseError::NestedGroup { position: 2 }));
    }

    #[test]
    fn a_group_where_an_item_of_a_group_stands_is_refused() {
        pretty_assertions::assert_eq!(read("((a))"), Err(CaseError::GroupInGroup { position: 1 }));
    }

    #[test]
    fn a_group_left_open_is_refused() {
        pretty_assertions::assert_eq!(read("(a"), Err(CaseError::UnclosedGroup { position: 2 }));
        pretty_assertions::assert_eq!(read("(a b)"), Err(CaseError::UnclosedGroup { position: 3 }));
    }

    #[test]
    fn input_after_a_complete_form_is_refused() {
        pretty_assertions::assert_eq!(read("a)"), Err(CaseError::TrailingInput { found: ')', position: 1 }));
    }

    #[test]
    fn a_missing_item_is_refused_with_what_stands_there() {
        pretty_assertions::assert_eq!(read("a|"), Err(CaseError::MissingItem { found: None, position: 2 }));
        pretty_assertions::assert_eq!(read(""), Err(CaseError::MissingItem { found: None, position: 0 }));
        pretty_assertions::assert_eq!(read(" "), Err(CaseError::MissingItem { found: None, position: 1 }));
        pretty_assertions::assert_eq!(read("()"), Err(CaseError::MissingItem { found: Some(')'), position: 1 }));
    }
}
