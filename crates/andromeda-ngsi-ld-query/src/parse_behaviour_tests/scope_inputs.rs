//! Hand-written inputs of the clause 4.19 entry points: `scopeQ`, one scope
//! pattern and one scope level, and the levels built at the length a level
//! may have.

use andromeda_ngsi_ld_lexicon::scope::level::LEVEL_CHARACTER_LIMIT;

/// `scopeQ` expressions of clause 4.19.
pub(super) const SCOPE_QUERY_INPUTS: &[&str] = &[
    "/A",
    "/A/+/C",
    "/A/#",
    "/#",
    "/A,/B",
    "/A|/B",
    "(/A;/B),/C",
    "/A;/B",
    "(/A|/B)",
    "((/A))",
    "(/A",
    "/A)",
    "/A,",
    "",
    "/A/#/B",
    "CompanyB",
    " /A ",
    "/A;",
    "/A/B/C",
    "/A/+",
    "/+/#",
    "/A ; /B",
    "(/A ; /B)",
    "(/A;/B)|(/C;/D)",
    "/A,(/B;/C)",
    "()",
    "(/A;)",
    "/A,,/B",
    "/A||/B",
    "/A/",
    "//A",
    "/A//B",
    "/é",
    "/A B",
    "#",
    "+",
    "/A/#/#",
    "(/A)",
    "/A\u{a0}",
];

/// Single scope patterns of clause 4.19.
pub(super) const SCOPE_PATTERN_INPUTS: &[&str] = &[
    "/A", "/A/B", "/A/+/C", "/A/#", "/#", "/+", "A", "CompanyB", "", " ", " /A ", "/A/", "//A", "/A//B", "/A/#/B", "/#/A", "/A/+", "/é", "/A B", "/A-b_1", "+",
    "#", "/A/++", "/A/#x", "/", "/A;B", "/A,B",
];

/// Single scope levels.
pub(super) const SCOPE_LABEL_INPUTS: &[&str] = &["A", "Madrid", "a_b-1", "", "A/B", "+", "#", " A", "A ", "é", "A B", "/", "a.b", "1"];

/// A scope level as long as a level may be, and one character longer.
pub(super) fn long_levels() -> [String; 2] {
    [LEVEL_CHARACTER_LIMIT, LEVEL_CHARACTER_LIMIT + 1].map(|characters| "A".repeat(characters))
}
