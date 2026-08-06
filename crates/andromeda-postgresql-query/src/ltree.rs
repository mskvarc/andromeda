//! Translation between NGSI-LD scope paths and PostgreSQL `ltree` / `lquery`.

use andromeda_ngsi_ld_lexicon::scope::grammar::Scope;
use andromeda_ngsi_ld_query::scope::pattern::{ScopePattern, ScopeSegment};
use sea_query::{Expr, SimpleExpr};

/// Convert an NGSI-LD scope path to PostgreSQL ltree format for storage.
///
/// Strips the optional leading `/` and replaces the remaining `/` separators
/// with `.` to form a valid ltree label path. Total by construction: every
/// level of a [`Scope`] already satisfies the clause 4.18 label grammar, which
/// is the same grammar `ltree` accepts.
///
/// # Examples
/// - `/A/B/C` -> `A.B.C`
/// - `/Madrid` -> `Madrid`
pub fn scope_to_ltree(scope: &Scope) -> String {
    // Exactly one leading slash, because clause 4.18 admits no empty level.
    let path = scope.as_str();
    path.strip_prefix('/').unwrap_or(path).replace('/', ".")
}

/// The PostgreSQL `lquery` a clause 4.18 scope pattern matches with.
///
/// Levels are dot-separated labels; `+` becomes `*{1}` and `#` becomes
/// `*{1,}`. Total by construction, because [`ScopePattern`] validated every
/// label against the same grammar `ltree` itself accepts.
pub fn pattern_to_lquery(pattern: &ScopePattern) -> String {
    let mut lquery = String::new();
    for (index, segment) in pattern.segments().iter().enumerate() {
        if index > 0 {
            lquery.push('.');
        }
        match segment {
            ScopeSegment::Label(label) => lquery.push_str(label.as_str()),
            ScopeSegment::SingleLevel => lquery.push_str("*{1}"),
            ScopeSegment::MultiLevel => lquery.push_str("*{1,}"),
        }
    }
    lquery
}

/// ltree: `col ~ lquery` - tests if ltree matches lquery pattern.
pub fn ltree_matches(col: SimpleExpr, lquery: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 ~ $2::lquery", [col, lquery])
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    fn lquery(raw: &str) -> String {
        pattern_to_lquery(&ScopePattern::parse(raw).expect("valid pattern"))
    }

    #[test]
    fn scope_wildcards_become_lquery_quantifiers() {
        assert_eq!(lquery("/A/B/C"), "A.B.C");
        assert_eq!(lquery("/A/#"), "A.*{1,}");
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
            ("/A/#", "A.*{1,}", [true, true, true, false]),
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
        scope_to_ltree(Scope::new(raw).expect("valid scope"))
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

    #[test]
    fn ltree_matches_uses_lquery_cast() {
        let expr = ltree_matches(Expr::col("scope_paths").into(), Expr::val("A.B.*").into());
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("::lquery"), "{sql}");
    }
}
