//! The parse entry points whose behaviour is recorded, how each reads a text,
//! and the inputs each is recorded over.

use crate::{
    dataset_selector::DatasetSelector,
    filter::{
        Filter,
        display::serialize_attr_path,
        generated_filters::{query, written_value_path},
        parser::value_path::parse_attribute_path,
        target_language::TargetLanguage,
    },
    ordering::{icu_collation::IcuCollation, written_ordering_term::WrittenOrderingTerm},
    parse_behaviour_tests::{
        dataset_selector_inputs::DATASET_SELECTOR_INPUTS,
        filter_inputs::{FILTER_INPUTS, PATTERN_INPUTS, TARGET_LANGUAGE_INPUTS, VALUE_PATH_INPUTS, deep_dotted_paths, deep_hops, nested_groups},
        generated_inputs::{aggregation_grouping, group_key, ordering_term, projection_list, scope_pattern, scope_query, type_selection},
        mutation::mutation,
        ordering_inputs::{COLLATION_INPUTS, ORDERING_INPUTS},
        outcome::{Outcome, read, read_and_write_back},
        projection_inputs::PROJECTION_INPUTS,
        scope_inputs::{SCOPE_LABEL_INPUTS, SCOPE_PATTERN_INPUTS, SCOPE_QUERY_INPUTS, long_levels},
        short_strings::{
            FILTER_ALPHABET,
            GROUPING_ALPHABET,
            ORDERING_ALPHABET,
            PROJECTION_ALPHABET,
            SCOPE_PATTERN_ALPHABET,
            SCOPE_QUERY_ALPHABET,
            TYPE_SELECTION_ALPHABET,
            VALUE_PATH_ALPHABET,
            short_strings,
        },
        temporal_inputs::{AGGREGATION_PERIOD_INPUTS, GROUP_KEY_INPUTS, GROUPING_INPUTS},
        type_selection_inputs::TYPE_SELECTION_INPUTS,
    },
    pattern::Pattern,
    projection::written_projection_list::WrittenProjectionList,
    scope::{
        scope_pattern::{ScopeLabel, ScopePattern},
        scope_query::ScopeQuery,
    },
    temporal::{
        aggregation_period::AggregationPeriod,
        grouping::{aggregation_grouping::AggregationGrouping, group_key::GroupKey},
    },
    type_selection::written_type_selection::WrittenTypeSelection,
};
use proptest::{
    prelude::BoxedStrategy,
    strategy::{Strategy, ValueTree},
    test_runner::TestRunner,
};
use std::{collections::HashSet, str::FromStr};
use strum::{Display, EnumIter};

/// How many mutations of each hand-written input are recorded.
const HAND_WRITTEN_MUTATIONS: usize = 4;

/// How many mutations of each generated input are recorded.
const GENERATED_MUTATIONS: usize = 2;

/// A parse entry point, named in messages by the request parameter or the
/// production it reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Display, EnumIter)]
pub(super) enum EntryPoint {
    /// `q` and `csf`, clause 4.9.
    #[strum(serialize = "q")]
    Filter,
    /// The entity type selection, clause 4.17.
    #[strum(serialize = "type")]
    TypeSelection,
    /// `scopeQ`, clause 4.19.
    #[strum(serialize = "scopeQ")]
    ScopeQuery,
    /// One scope pattern of a `scopeQ`, clause 4.19.
    #[strum(serialize = "scopePattern")]
    ScopePattern,
    /// One level of a scope pattern.
    #[strum(serialize = "scopeLabel")]
    ScopeLabel,
    /// `pick` and `omit`, clause 4.21.
    #[strum(serialize = "pick")]
    Projection,
    /// One `orderBy` term, clause 4.23.
    #[strum(serialize = "orderBy")]
    Ordering,
    /// `collation`, clause 4.23.
    #[strum(serialize = "collation")]
    Collation,
    /// `aggrGroupBy`.
    #[strum(serialize = "aggrGroupBy")]
    Grouping,
    /// One `aggrGroupBy` key.
    #[strum(serialize = "groupKey")]
    GroupKey,
    /// A standalone clause 4.9 `ValuePath`.
    #[strum(serialize = "valuePath")]
    ValuePath,
    /// The target language of a `q` bracket, clause 4.9.
    #[strum(serialize = "targetLanguage")]
    TargetLanguage,
    /// A regular expression of a `q` pattern operand or of `idPattern`.
    #[strum(serialize = "pattern")]
    Pattern,
    /// `datasetId`, clause 4.5.5.
    #[strum(serialize = "datasetId")]
    DatasetSelector,
    /// `aggrPeriodDuration`, clause 4.5.19.
    #[strum(serialize = "aggrPeriodDuration")]
    AggregationPeriod,
}

impl EntryPoint {
    /// What the entry point makes of `text`.
    pub(super) fn outcome(self, text: &str) -> Outcome {
        match self {
            EntryPoint::Filter => read_and_write_back(text, Filter::from_str, Filter::to_string),
            EntryPoint::TypeSelection => read_and_write_back(text, WrittenTypeSelection::from_str, WrittenTypeSelection::to_string),
            EntryPoint::ScopeQuery => read_and_write_back(text, ScopeQuery::from_str, ScopeQuery::to_string),
            EntryPoint::ScopePattern => read_and_write_back(text, ScopePattern::parse, ScopePattern::to_string),
            EntryPoint::ScopeLabel => read(text, ScopeLabel::parse),
            EntryPoint::Projection => read_and_write_back(text, WrittenProjectionList::from_str, WrittenProjectionList::to_string),
            EntryPoint::Ordering => read_and_write_back(text, WrittenOrderingTerm::from_str, WrittenOrderingTerm::to_string),
            EntryPoint::Collation => read_and_write_back(text, IcuCollation::parse, IcuCollation::to_string),
            EntryPoint::Grouping => read(text, AggregationGrouping::from_str),
            EntryPoint::GroupKey => read(text, GroupKey::parse),
            EntryPoint::ValuePath => read_and_write_back(text, parse_attribute_path, serialize_attr_path),
            EntryPoint::TargetLanguage => read(text, TargetLanguage::parse),
            EntryPoint::Pattern => read_and_write_back(text, Pattern::from_str, Pattern::to_string),
            EntryPoint::DatasetSelector => read_and_write_back(text, DatasetSelector::from_str, DatasetSelector::to_string),
            EntryPoint::AggregationPeriod => read(text, AggregationPeriod::parse),
        }
    }

    /// Every input recorded for the entry point, each once, in this order:
    /// each hand-written input followed by its mutations, each generated input
    /// followed by its mutations, and the short strings over its alphabet.
    pub(super) fn corpus(self) -> Vec<String> {
        let mut runner = TestRunner::deterministic();
        let mutations = mutation();
        let mut texts = Vec::new();
        for source in self.hand_written() {
            let mutated: Vec<String> = (0..HAND_WRITTEN_MUTATIONS)
                .map(|_| drawn(&mutations, &mut runner).applied_to(&source))
                .collect();
            texts.push(source);
            texts.extend(mutated);
        }
        if let Some((generator, count)) = self.generator() {
            for _ in 0..count {
                let source = drawn(&generator, &mut runner);
                let mutated: Vec<String> = (0..GENERATED_MUTATIONS).map(|_| drawn(&mutations, &mut runner).applied_to(&source)).collect();
                texts.push(source);
                texts.extend(mutated);
            }
        }
        texts.extend(self.short_strings());
        let mut seen = HashSet::new();
        texts.retain(|text| seen.insert(text.clone()));
        texts
    }

    /// The hand-written inputs, those built at a ceiling among them.
    fn hand_written(self) -> Vec<String> {
        let (written, built): (&[&str], Vec<String>) = match self {
            EntryPoint::Filter => (
                FILTER_INPUTS,
                nested_groups().into_iter().chain(deep_dotted_paths()).chain(deep_hops()).collect(),
            ),
            EntryPoint::TypeSelection => (TYPE_SELECTION_INPUTS, Vec::new()),
            EntryPoint::ScopeQuery => (SCOPE_QUERY_INPUTS, Vec::new()),
            EntryPoint::ScopePattern => (SCOPE_PATTERN_INPUTS, long_levels().map(|level| format!("/{level}")).into()),
            EntryPoint::ScopeLabel => (SCOPE_LABEL_INPUTS, long_levels().into()),
            EntryPoint::Projection => (PROJECTION_INPUTS, Vec::new()),
            EntryPoint::Ordering => (ORDERING_INPUTS, Vec::new()),
            EntryPoint::Collation => (COLLATION_INPUTS, Vec::new()),
            EntryPoint::Grouping => (GROUPING_INPUTS, Vec::new()),
            EntryPoint::GroupKey => (GROUP_KEY_INPUTS, Vec::new()),
            EntryPoint::ValuePath => (VALUE_PATH_INPUTS, deep_dotted_paths().into()),
            EntryPoint::TargetLanguage => (TARGET_LANGUAGE_INPUTS, Vec::new()),
            EntryPoint::Pattern => (PATTERN_INPUTS, Vec::new()),
            EntryPoint::DatasetSelector => (DATASET_SELECTOR_INPUTS, Vec::new()),
            EntryPoint::AggregationPeriod => (AGGREGATION_PERIOD_INPUTS, Vec::new()),
        };
        written.iter().map(|text| (*text).to_owned()).chain(built).collect()
    }

    /// The generator of the entry point's own grammar and how many values are
    /// drawn from it, for an entry point that reads a grammar of its own
    /// rather than a value another type owns.
    fn generator(self) -> Option<(BoxedStrategy<String>, usize)> {
        match self {
            EntryPoint::Filter => Some((query(), 1_000)),
            EntryPoint::TypeSelection => Some((type_selection(), 300)),
            EntryPoint::ScopeQuery => Some((scope_query(), 300)),
            EntryPoint::ScopePattern => Some((scope_pattern(), 200)),
            EntryPoint::Projection => Some((projection_list(), 300)),
            EntryPoint::Ordering => Some((ordering_term(), 300)),
            EntryPoint::Grouping => Some((aggregation_grouping(), 300)),
            EntryPoint::GroupKey => Some((group_key(), 200)),
            EntryPoint::ValuePath => Some((written_value_path(), 200)),
            EntryPoint::ScopeLabel
            | EntryPoint::Collation
            | EntryPoint::TargetLanguage
            | EntryPoint::Pattern
            | EntryPoint::DatasetSelector
            | EntryPoint::AggregationPeriod => None,
        }
    }

    /// Every short string over the entry point's alphabet, for an entry point
    /// whose grammar is its own.
    fn short_strings(self) -> Vec<String> {
        match self {
            EntryPoint::Filter => short_strings(&FILTER_ALPHABET, 3),
            EntryPoint::TypeSelection => short_strings(&TYPE_SELECTION_ALPHABET, 4),
            EntryPoint::ScopeQuery => short_strings(&SCOPE_QUERY_ALPHABET, 3),
            EntryPoint::ScopePattern => short_strings(&SCOPE_PATTERN_ALPHABET, 4),
            EntryPoint::Projection => short_strings(&PROJECTION_ALPHABET, 4),
            EntryPoint::Ordering => short_strings(&ORDERING_ALPHABET, 3),
            EntryPoint::Grouping => short_strings(&GROUPING_ALPHABET, 3),
            EntryPoint::ValuePath => short_strings(&VALUE_PATH_ALPHABET, 3),
            EntryPoint::ScopeLabel
            | EntryPoint::Collation
            | EntryPoint::GroupKey
            | EntryPoint::TargetLanguage
            | EntryPoint::Pattern
            | EntryPoint::DatasetSelector
            | EntryPoint::AggregationPeriod => Vec::new(),
        }
    }
}

/// One value drawn from `strategy` with `runner`'s random source.
fn drawn<Drawn: Strategy>(strategy: &Drawn, runner: &mut TestRunner) -> Drawn::Value {
    strategy.new_tree(runner).expect("the strategy rejects no draw").current()
}
