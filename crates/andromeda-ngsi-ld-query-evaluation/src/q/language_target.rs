//! The target value of a `LanguageProperty` addressed with a target language.

use crate::{
    q::{
        family_target::terminal_value,
        target_value::{ResolvedTarget, TargetValue},
    },
    queried_entity::attribute_reading::QueriedInstance,
};
use andromeda_ngsi_ld_query::filter::{
    language_matching::LanguageMatching,
    reading::family_reading::{MultiValueReading, TargetOrigin, TerminalKeyword},
    target_language::LanguageSelector,
};

/// The target value clause 4.9 defines for a `LanguageProperty` with a target
/// language: "the Value associated to the matching key-value pair held within
/// the languageMap ... where the key matches the target language", every key
/// `matching` selects contributing its strings.
///
/// A language holding several strings is matched "as a single element from the
/// array of values of the key-value pair", which is the any-one reading the
/// language map has with no target language.
///
/// A language the map does not hold yields an empty array: the entity still
/// contains the target element, the `LanguageProperty`, and clause 4.9 makes
/// `!=` match it where "No matching value is found as the value of the
/// specified language key", and `!~=` where no value is in L(R). No positive
/// operator matches an empty array.
pub(crate) fn language_target<'a, I: QueriedInstance<'a>>(instance: I, language: LanguageSelector<'_>, matching: LanguageMatching) -> ResolvedTarget<'a> {
    let mut strings = instance
        .language_values(language, matching)
        .filter_map(|item| terminal_value(TerminalKeyword::Literal, item));
    let value = match (strings.next(), strings.next()) {
        (Some(only), None) => only,
        (None, _) => TargetValue::Array(Vec::new()),
        (Some(first), Some(second)) => TargetValue::Array([first, second].into_iter().chain(strings).collect()),
    };
    ResolvedTarget {
        value,
        origin: TargetOrigin::AttributeValue,
        reading: MultiValueReading::AnyOneValue,
    }
}
