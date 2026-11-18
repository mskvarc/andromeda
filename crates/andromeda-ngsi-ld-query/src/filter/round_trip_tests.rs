//! Serialising a parsed `q` filter and parsing the text again gives back the
//! same filter, for filters generated over the whole clause 4.9 grammar.
//!
//! A filter is stored, forwarded and read back through its serialisation, so
//! a filter the round trip changes is answered differently once it has
//! travelled.

use crate::filter::{expression::Expr, generated_filters::query, parser::parse, serialize::serialize};
use proptest::{
    prelude::{prop_assert_eq, proptest},
    test_runner::Config,
};

/// The filter a client wrote, parsed.
fn parsed(written: &str) -> Expr {
    parse(written).unwrap_or_else(|error| panic!("the generator writes only grammatical filters, and {written} is refused: {error}"))
}

proptest! {
    #![proptest_config(Config::with_cases(2048))]

    #[test]
    fn serialising_a_parsed_filter_and_parsing_it_again_gives_the_same_filter(written in query()) {
        let expr = parsed(&written);
        let serialised = serialize(&expr);
        prop_assert_eq!(parse(&serialised).ok(), Some(expr), "{} serialised as {}", written, serialised);
    }

    #[test]
    fn serialising_is_stable_after_one_round_trip(written in query()) {
        let serialised = serialize(&parsed(&written));
        prop_assert_eq!(serialize(&parsed(&serialised)), serialised);
    }
}
