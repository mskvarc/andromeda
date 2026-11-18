//! Every string up to a few symbols long over an entry point's alphabet.
//!
//! Two parsers of one grammar first disagree on short inputs that combine its
//! punctuation in ways no hand-written case covers, so every entry point with
//! a grammar of its own is read over all of them.

use std::iter::successors;

/// The `q` alphabet: names, operands, every operator and every structural
/// character of clause 4.9, and a space.
pub(super) const FILTER_ALPHABET: [&str; 21] = [
    "a", "1", "\"x\"", "==", "!=", ">", "<=", "~=", ";", "|", "(", ")", "[", "]", "{", "}", ".", ",", "..", ":", " ",
];

/// The entity type selection alphabet of clause 4.17.
pub(super) const TYPE_SELECTION_ALPHABET: [&str; 8] = ["A", "*", "(", ")", ";", "|", ",", " "];

/// The `scopeQ` alphabet of clause 4.19.
pub(super) const SCOPE_QUERY_ALPHABET: [&str; 11] = ["/A", "/", "A", "+", "#", "(", ")", ";", "|", ",", " "];

/// The alphabet of one scope pattern.
pub(super) const SCOPE_PATTERN_ALPHABET: [&str; 6] = ["/", "A", "+", "#", " ", "é"];

/// The `pick` and `omit` alphabet of clause 4.21.
pub(super) const PROJECTION_ALPHABET: [&str; 8] = ["a", "r", "{", "}", ",", "|", " ", ";"];

/// The `orderBy` alphabet of clause 4.23.
pub(super) const ORDERING_ALPHABET: [&str; 14] = ["a", "b", ".", "[", "]", "*", ";", "asc", "desc", "{", "}", "\"", " ", "-"];

/// The `aggrGroupBy` alphabet.
pub(super) const GROUPING_ALPHABET: [&str; 10] = ["observedAt", "a", ".", "[b]", ",", ";", "month", "id", " ", "{"];

/// The alphabet of a standalone `ValuePath`.
pub(super) const VALUE_PATH_ALPHABET: [&str; 13] = ["a", ".", "[", "]", "*", "-", "{", "}", ":", "\"", " ", "1", "en"];

/// Every concatenation of up to `longest` symbols of `alphabet`, shortest
/// first, the empty string among them.
pub(super) fn short_strings(alphabet: &[&str], longest: usize) -> Vec<String> {
    successors(Some(vec![String::new()]), |shorter| {
        Some(
            shorter
                .iter()
                .flat_map(|prefix| alphabet.iter().map(move |symbol| format!("{prefix}{symbol}")))
                .collect(),
        )
    })
    .take(longest + 1)
    .flatten()
    .collect()
}
