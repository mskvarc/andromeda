//! Translation between NGSI-LD scope paths and PostgreSQL `ltree` / `lquery`.

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use andromeda_ngsi_ld_query::scope::pattern::{ScopePattern, ScopeSegment};

/// Convert an NGSI-LD scope path to PostgreSQL ltree format for storage.
///
/// The levels become dot-separated labels, so `/A/B/C` is `A.B.C` and
/// `/Madrid` is `Madrid`. Total by construction: a [`ScopePath`] is a
/// hierarchical clause 4.18 path whose levels satisfy the same label grammar
/// `ltree` accepts and stay within the label length it can hold, and the
/// NGSI-LD Null of a deleted scope is not one of its values.
#[must_use]
pub fn scope_to_ltree(scope: &ScopePath) -> String {
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
/// clause 4.19: a prefix `#` (something precedes it) is `*` — zero or more
/// further labels — so `/A/#` becomes `A.*` and matches the anchor `A` itself
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
        // They are the same clause, so they must give the same verdict; an
        // earlier in-memory reading had `#` consume exactly one further level
        // while `*{1,}` consumed one or more, and nothing held the two together.
        let paths = ["/A/B", "/A/B/C", "/A/B/C/D", "/A"];
        let cases = [
            // pattern, rendered lquery, verdict per path above
            ("/A/B", "A.B", [true, false, false, false]),
            ("/A/+/C", "A.*{1}.C", [false, true, false, false]),
            // Clause 4.19 Example 2: `/A/#` matches `/A` itself as well as below.
            ("/A/#", "A.*", [true, true, true, true]),
            ("/#", "*{1,}", [true, true, true, true]),
        ];

        for (raw, rendered, verdicts) in cases {
            let pattern = ScopePattern::parse(raw).expect("valid pattern");
            assert_eq!(pattern_to_lquery(&pattern), rendered, "{raw} renders the quantifier its verdicts assume");
            for (path, expected) in paths.iter().zip(verdicts) {
                assert_eq!(pattern.matches(path), expected, "{raw} against {path}");
            }
        }
    }

    #[test]
    fn unicode_level_names_survive_the_lquery_translation() {
        assert_eq!(lquery("/M\u{00fc}nchen/Stra\u{00df}e"), "M\u{00fc}nchen.Stra\u{00df}e");
    }

    fn ltree(raw: &str) -> String {
        scope_to_ltree(&raw.parse::<ScopePath>().expect("valid scope path"))
    }

    #[test]
    fn a_scope_path_becomes_a_dotted_label_path() {
        assert_eq!(ltree("/A/B/C"), "A.B.C");
        assert_eq!(ltree("/Madrid"), "Madrid");
    }

    #[test]
    fn a_scope_written_without_its_leading_slash_renders_identically() {
        // Clause 4.18: every scope is absolute, so the leading `/` carries no
        // information and both spellings are the same path.
        assert_eq!(ltree("Madrid"), ltree("/Madrid"));
    }
}
