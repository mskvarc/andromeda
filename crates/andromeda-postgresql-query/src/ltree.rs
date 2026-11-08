//! Translation between NGSI-LD scope paths and PostgreSQL `ltree` / `lquery`.
//!
//! The one place a Scope meets PostgreSQL. A stored Scope is a
//! [`StoredScopePath`]: the `ltree` label path a `scopeQ` is matched against,
//! beside the text it was written in, which is the only form read back. A
//! label path forgets whether the Scope was written with its optional leading
//! `/`, so nothing rebuilds a Scope from one.

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use andromeda_ngsi_ld_query::scope::pattern::{ScopePattern, ScopeSegment};

/// The two columns one stored clause 4.18 Scope occupies.
///
/// A store keeps both, side by side: the label path in an `ltree` column that
/// `scopeQ` predicates and their index read, and the written text in a text
/// column that the read path parses back with `ScopePath::from_str`. Clause
/// 4.6.4 has the broker "return the original content", and only the text
/// holds it, since `Madrid` and `/Madrid` share the label path `Madrid`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredScopePath<'a> {
    /// The `ltree` label path: the levels joined by `.`, so `/A/B/C` is
    /// `A.B.C` and `Madrid` is `Madrid`.
    pub label_path: String,
    /// The Scope as it was written.
    pub written: &'a str,
}

impl<'a> From<&'a ScopePath> for StoredScopePath<'a> {
    fn from(scope: &'a ScopePath) -> StoredScopePath<'a> {
        StoredScopePath {
            label_path: label_path(scope),
            written: scope.as_str(),
        }
    }
}

/// The `ltree` label path of a scope path.
///
/// Total by construction: a [`ScopePath`] is a hierarchical clause 4.18 path
/// whose levels satisfy the same label grammar `ltree` accepts and stay within
/// the label length it can hold, and the NGSI-LD Null of a deleted scope is not
/// one of its values.
fn label_path(scope: &ScopePath) -> String {
    let mut ltree = String::with_capacity(scope.as_str().len());
    for (index, level) in scope.levels().enumerate() {
        if index > 0 {
            ltree.push('.');
        }
        ltree.push_str(level);
    }
    ltree
}

/// The PostgreSQL `lquery` a clause 4.18 scope pattern matches with.
///
/// Levels are dot-separated labels; `+` becomes `*{1}`. A `#` renders per
/// clause 4.19: a prefix `#` (something precedes it) is `*`, meaning zero or more
/// further labels, so `/A/#` becomes `A.*` and matches the anchor `A` itself
/// as well as its descendants; a bare `/#` is `*{1,}`, any non-empty scope.
/// Total by construction, because [`ScopePattern`] validated every label
/// against the same grammar `ltree` itself accepts.
#[must_use]
pub fn pattern_to_lquery(pattern: &ScopePattern) -> String {
    let mut lquery = String::new();
    for (index, segment) in pattern.segments().iter().enumerate() {
        if index > 0 {
            lquery.push('.');
        }
        match segment {
            ScopeSegment::Label(label) => lquery.push_str(label.as_str()),
            ScopeSegment::SingleLevel => lquery.push_str("*{1}"),
            // A prefix `#` matches the anchor or below (`*`, zero-or-more); a
            // bare `#` matches any non-empty scope (`*{1,}`, one-or-more).
            ScopeSegment::MultiLevel if index == 0 => lquery.push_str("*{1,}"),
            ScopeSegment::MultiLevel => lquery.push('*'),
        }
    }
    lquery
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn lquery(raw: &str) -> String {
        pattern_to_lquery(&ScopePattern::parse(raw).expect("valid pattern"))
    }

    #[test]
    fn scope_wildcards_become_lquery_quantifiers() {
        assert_eq!(lquery("/A/B/C"), "A.B.C");
        // A prefix `#` is `*` so `A.*` matches the anchor `A` and its descendants.
        assert_eq!(lquery("/A/#"), "A.*");
        assert_eq!(lquery("/A/+/C"), "A.*{1}.C");
        assert_eq!(lquery("/#"), "*{1,}");
    }

    #[test]
    fn a_pattern_written_without_its_leading_slash_renders_identically() {
        assert_eq!(lquery("Madrid"), lquery("/Madrid"));
    }

    #[test]
    fn the_in_memory_matcher_and_the_lquery_rendering_agree_on_every_wildcard() {
        // Clause 4.18 has two readings in this workspace: `ScopePattern::matches`
        // answers it in memory for subscription matching and for a residual
        // filter, and this module answers it by handing PostgreSQL an `lquery`.
        // They are the same clause, so they must give the same verdict.
        let paths = ["/A/B", "/A/B/C", "/A/B/C/D", "/A"];
        let cases = [
            ("/A/B", "A.B", [true, false, false, false]),
            ("/A/+/C", "A.*{1}.C", [false, true, false, false]),
            // Clause 4.19 Example 2: `/A/#` matches `/A` itself as well as below.
            ("/A/#", "A.*", [true, true, true, true]),
            ("/#", "*{1,}", [true, true, true, true]),
        ];

        for (raw, rendered, verdicts) in cases {
            let pattern = ScopePattern::parse(raw).expect("valid pattern");
            assert_eq!(pattern_to_lquery(&pattern), rendered, "{raw} renders the quantifier its verdicts assume");
            for (written, expected) in paths.iter().zip(verdicts) {
                assert_eq!(pattern.matches(&path(written)), expected, "{raw} against {written}");
            }
        }
    }

    #[test]
    fn unicode_level_names_survive_the_lquery_translation() {
        assert_eq!(lquery("/M\u{00fc}nchen/Stra\u{00df}e"), "M\u{00fc}nchen.Stra\u{00df}e");
    }

    fn path(raw: &str) -> ScopePath {
        raw.parse::<ScopePath>().expect("valid scope path")
    }

    /// A stored Scope keeps the text it was written in beside the label path
    /// it is matched by.
    #[test]
    fn a_stored_scope_keeps_its_written_text_beside_its_label_path() {
        let bare = path("Madrid");
        let nested = path("/A/B/C");
        assert_eq!(
            StoredScopePath::from(&bare),
            StoredScopePath {
                label_path: "Madrid".to_owned(),
                written: "Madrid"
            }
        );
        assert_eq!(
            StoredScopePath::from(&nested),
            StoredScopePath {
                label_path: "A.B.C".to_owned(),
                written: "/A/B/C"
            }
        );
    }

    /// Both spellings name one place, so a `scopeQ` matches them alike.
    #[test]
    fn both_spellings_share_one_label_path() {
        let bare = path("Madrid");
        let rooted = path("/Madrid");
        assert_eq!(StoredScopePath::from(&bare).label_path, StoredScopePath::from(&rooted).label_path);
    }

    #[test]
    fn unicode_levels_survive_the_label_path() {
        let unicode = path("/M\u{00fc}nchen/Stra\u{00df}e");
        assert_eq!(StoredScopePath::from(&unicode).label_path, "M\u{00fc}nchen.Stra\u{00df}e");
    }
}
