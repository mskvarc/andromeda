//! Serialising a parsed `q` filter and parsing the text again gives back the
//! same filter, for filters generated over the whole clause 4.9 grammar.
//!
//! A filter is stored, forwarded and read back through its serialisation, so
//! a filter the round trip changes is answered differently once it has
//! travelled.

use crate::filter::{Filter, generated_filters::query};
use proptest::{
    prelude::{prop_assert_eq, proptest},
    test_runner::Config,
};

/// The filter a client wrote, parsed.
fn parsed(written: &str) -> Filter {
    written
        .parse::<Filter>()
        .unwrap_or_else(|error| panic!("the generator writes only grammatical filters, and {written} is refused: {error}"))
}

proptest! {
    #![proptest_config(Config::with_cases(2048))]

    #[test]
    fn serialising_a_parsed_filter_and_parsing_it_again_gives_the_same_filter(written in query()) {
        let expr = parsed(&written);
        let serialised = expr.to_string();
        prop_assert_eq!(serialised.parse::<Filter>().ok(), Some(expr), "{} serialised as {}", written, serialised);
    }

    #[test]
    fn serialising_is_stable_after_one_round_trip(written in query()) {
        let serialised = parsed(&written).to_string();
        prop_assert_eq!(parsed(&serialised).to_string(), serialised);
    }
}
