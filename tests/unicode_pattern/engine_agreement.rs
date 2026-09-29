//! The two engines, compared directly: the `regex` crate reading the client's
//! text, and PostgreSQL reading the restatement the store is handed.
//!
//! Every comparison runs against the test database the broker writes to, so
//! PostgreSQL is the one the broker runs on, at the version and with the
//! collations it has. The restatement is compared under `COLLATE "C"`, which
//! every stored value carries, under the database default, which is what a
//! registered pattern is evaluated under, and under `pg_unicode_fast`, a
//! collation that classifies and folds by Unicode, to pin that it depends on
//! none of them.
//!
//! The shorthand classes are compared over every code point rather than over a
//! corpus: each is a Unicode property of hundreds of ranges, and a restatement
//! that dropped or shifted one would show only on characters no corpus names.

use andromeda_ngsi_ld_query::pattern::Pattern;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;
use std::collections::BTreeSet;

/// Admitted patterns: the case-insensitive option set, scoped and cleared,
/// over letters whose fold sets hold two, three or no partners, the shorthand
/// classes bare, bracketed, negated and folded, beside the constructs the
/// restatement has to carry unchanged.
const CORPUS: &[&str] = &[
    "(?i)čer",
    "(?i)^čebela$",
    "(?i)[a-ž]",
    "(?i)[č-ž]",
    "(?i)k",
    "(?i)s",
    "(?i)σ",
    "(?i)ǆ",
    "(?i)[^č]",
    "(?i)^[^čš]+$",
    "(?i)ß",
    "(?i)i",
    "(?i)ő",
    "(?i)ł",
    "(?i)ĳ",
    "(?i)[[:alpha:]]",
    "(?i)[[:upper:]]+",
    "(?i)ö|ä|ü",
    "(?i)^ŽALEC(?:-[0-9]+)?$",
    "(?i)^straße$",
    "(?i)ΣΟΦΙΑ",
    "(?i:č)er",
    "^(?i:č)ebela$",
    "a(?i)b(?-i)c",
    "(?i)^če(?-i)bela$",
    "(?i)(ab|cd)(?-i)e",
    "(?im)^prvi.čebela$",
    "(?m)^b$",
    "(?m)a$",
    "(?x) č e r",
    "(?s)a.b",
    "[Č-č]",
    "[a-ž]",
    "[Z-a]",
    "[ÿ-ž]",
    "[А-я]",
    "[[:upper:]]+",
    "^urn:ngsi-ld:Vehicle:.*",
    "a.b",
    "[]a]",
    "[^]a]",
    "[a-]",
    "[.-/]",
    "a{2,3}",
    "(a*)*",
    r"Merc\.?edes",
    r"\w",
    r"^\w+$",
    r"\W",
    r"\d",
    r"^\d+$",
    r"\D",
    r"\s",
    r"^\S+\s\S+$",
    r"[^\s]",
    r"^[\w\-:]+$",
    r"^[\w.]+$",
    r"(?i)^\w+$",
    r"(?i)^č\w*$",
    r"^urn:ngsi-ld:\w+:\d+$",
    r"(?m)^\w+$",
];

/// The values every pattern of [`CORPUS`] is evaluated against.
const SUBJECTS: &[&str] = &[
    "č",
    "Č",
    "c",
    "C",
    "Čer",
    "ČER",
    "čEr",
    "cer",
    "čebela",
    "Čebela",
    "ČEBELA",
    "ČEbela",
    "cebela",
    "a",
    "A",
    "z",
    "Z",
    "ł",
    "Ł",
    "ž",
    "Ž",
    "ǆ",
    "ǅ",
    "Ǆ",
    "k",
    "K",
    "\u{212A}",
    "s",
    "S",
    "\u{17F}",
    "σ",
    "ς",
    "Σ",
    "ß",
    "ẞ",
    "SS",
    "i",
    "I",
    "İ",
    "ı",
    "ő",
    "Ő",
    "ĳ",
    "Ĳ",
    "é",
    "É",
    "ö",
    "Ö",
    "ä",
    "Ä",
    "ü",
    "Ü",
    "Žalec",
    "žalec-12",
    "ŽALEC-X",
    "Straße",
    "STRAẞE",
    "STRASSE",
    "σοφια",
    "ΣΟΦΙΑ",
    "σοφιά",
    "[",
    "\\",
    "]",
    "^",
    "_",
    "`",
    "-",
    ".",
    "/",
    "ÿ",
    "Ā",
    "ё",
    "Ё",
    "я",
    "Я",
    "\u{450}",
    "B",
    "1",
    " ",
    "ab",
    "abe",
    "cde",
    "Abe",
    "aBc",
    "abc",
    "aa",
    "aaa",
    "a\nb",
    "b\na",
    "prvi\nČebela",
    "prvi\nčebela",
    "Škofja Loka",
    "urn:ngsi-ld:Vehicle:A1",
    "Mercedes",
    "Merc.edes",
    "\u{663}",
    "\u{662}\u{660}\u{662}\u{666}",
    "\u{FF11}",
    "²",
    "½",
    "\u{A0}",
    "\u{3000}",
    "\u{200B}",
    "\t",
    "Škofja\u{A0}Loka",
    "Škofja\u{3000}Loka",
    "e\u{301}",
    "a_1",
    "urn:ngsi-ld:Čebela:\u{663}",
    "urn:ngsi-ld:čebela:12",
    "urn:ngsi-ld:čebela:x",
    "Ελλάδα",
    "łódź",
];

/// How the operand of `~` is collated in one comparison.
#[derive(Debug, Clone, Copy)]
enum OperandCollation {
    /// `COLLATE "C"`, which every stored value carries.
    Bytewise,
    /// The database default, which a registered pattern is evaluated under.
    DatabaseDefault,
    /// `pg_unicode_fast`, which classifies and folds by Unicode.
    UnicodeFast,
}

impl OperandCollation {
    /// The statement selecting the subjects the pattern bound as `$1` matches.
    const fn statement(self) -> &'static str {
        match self {
            OperandCollation::Bytewise => r#"SELECT subject FROM unnest($2::text[]) AS s(subject) WHERE subject COLLATE "C" ~ $1"#,
            OperandCollation::DatabaseDefault => "SELECT subject FROM unnest($2::text[]) AS s(subject) WHERE subject ~ $1",
            OperandCollation::UnicodeFast => "SELECT subject FROM unnest($2::text[]) AS s(subject) WHERE subject COLLATE pg_unicode_fast ~ $1",
        }
    }

    /// The statement counting the code points on which the pattern bound as
    /// `$1` disagrees with the set bound as `$2`, and the code points it
    /// matches.
    const fn code_point_statement(self) -> &'static str {
        match self {
            OperandCollation::Bytewise => {
                r#"WITH e AS MATERIALIZED (SELECT $2::text::int4multirange AS expected)
                   SELECT count(*) FILTER (WHERE (chr(cp) COLLATE "C" ~ $1) IS DISTINCT FROM (e.expected @> cp)),
                          count(*) FILTER (WHERE chr(cp) COLLATE "C" ~ $1)
                   FROM e, generate_series(1, 1114111) AS c(cp) WHERE cp NOT BETWEEN 55296 AND 57343"#
            }
            OperandCollation::DatabaseDefault => {
                "WITH e AS MATERIALIZED (SELECT $2::text::int4multirange AS expected)
                 SELECT count(*) FILTER (WHERE (chr(cp) ~ $1) IS DISTINCT FROM (e.expected @> cp)),
                        count(*) FILTER (WHERE chr(cp) ~ $1)
                 FROM e, generate_series(1, 1114111) AS c(cp) WHERE cp NOT BETWEEN 55296 AND 57343"
            }
            OperandCollation::UnicodeFast => {
                "WITH e AS MATERIALIZED (SELECT $2::text::int4multirange AS expected)
                 SELECT count(*) FILTER (WHERE (chr(cp) COLLATE pg_unicode_fast ~ $1) IS DISTINCT FROM (e.expected @> cp)),
                        count(*) FILTER (WHERE chr(cp) COLLATE pg_unicode_fast ~ $1)
                 FROM e, generate_series(1, 1114111) AS c(cp) WHERE cp NOT BETWEEN 55296 AND 57343"
            }
        }
    }
}

/// The subjects PostgreSQL finds `regex` matches, under `collation`.
async fn postgresql_selects(pool: &PgPool, regex: &str, collation: OperandCollation) -> BTreeSet<String> {
    sqlx::query_scalar::<_, String>(collation.statement())
        .bind(regex)
        .bind(SUBJECTS)
        .fetch_all(pool)
        .await
        .map_err(|error| format!("{regex:?}: {error}"))
        .expect("PostgreSQL compiles the pattern")
        .into_iter()
        .collect()
}

/// The subjects the `regex` crate finds `pattern` matches.
fn memory_selects(pattern: &Pattern) -> BTreeSet<String> {
    SUBJECTS
        .iter()
        .filter(|subject| pattern.is_match(subject))
        .map(|subject| (*subject).to_owned())
        .collect()
}

/// Every admitted pattern selects, from the same values, the same subjects in
/// PostgreSQL as in memory, whatever collation the operand carries.
#[tokio::test]
async fn unicode_pattern_engine_agreement_every_admitted_pattern_selects_the_same_subjects_in_both_engines() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let pool = scope.database_pool();

    for raw in CORPUS {
        let pattern = Pattern::parse(raw)
            .map_err(|error| format!("{raw}: {error:?}"))
            .expect("the pattern is admitted");
        let expected = memory_selects(&pattern);
        for collation in [OperandCollation::Bytewise, OperandCollation::DatabaseDefault, OperandCollation::UnicodeFast] {
            let selected = postgresql_selects(pool, pattern.portable().as_str(), collation).await;
            assert_eq!(selected, expected, "{raw} restated as {:?} under {collation:?}", pattern.portable().as_str());
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// The letters of the Latin, Greek and Cyrillic blocks, and the three symbols
/// that fold into them: every European alphabet NGSI-LD values are written in.
fn european_letters() -> Vec<char> {
    let blocks = [0x41..=0x24F, 0x370..=0x52F, 0x1E00..=0x1EFF, 0x2126..=0x2126, 0x212A..=0x212B];
    blocks
        .into_iter()
        .flatten()
        .filter_map(char::from_u32)
        .filter(|letter| !letter.is_control())
        .collect()
}

/// For every letter of those blocks, `(?i)^<letter>$` selects the same letters
/// in PostgreSQL as in memory: the fold set of each one, written out, and
/// nothing else.
#[tokio::test]
async fn unicode_pattern_engine_agreement_every_european_letter_folds_alike_in_both_engines() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let letters: Vec<String> = european_letters().into_iter().map(String::from).collect();
    let patterns: Vec<Pattern> = letters
        .iter()
        .map(|letter| {
            Pattern::parse(&format!("(?i)^{}$", regex::escape(letter)))
                .map_err(|error| format!("{letter}: {error:?}"))
                .expect("the pattern is admitted")
        })
        .collect();

    let expected: BTreeSet<(i64, String)> = patterns
        .iter()
        .zip(1_i64..)
        .flat_map(|(pattern, ordinal)| {
            letters
                .iter()
                .filter(|letter| pattern.is_match(letter))
                .map(move |letter| (ordinal, letter.clone()))
        }) // clone: each matched pair owns its letter.
        .collect();
    let portable: Vec<&str> = patterns.iter().map(|pattern| pattern.portable().as_str()).collect();
    let selected: BTreeSet<(i64, String)> = sqlx::query_as::<_, (i64, String)>(
        r#"SELECT p.ordinal, s.letter
           FROM unnest($1::text[]) WITH ORDINALITY AS p(pattern, ordinal)
           CROSS JOIN unnest($2::text[]) AS s(letter)
           WHERE s.letter COLLATE "C" ~ p.pattern"#,
    )
    .bind(&portable)
    .bind(&letters)
    .fetch_all(scope.database_pool())
    .await
    .expect("PostgreSQL compiles every restatement")
    .into_iter()
    .collect();

    assert_eq!(selected.len(), expected.len(), "one pair per letter and fold partner");
    assert_eq!(selected, expected);

    scope.finish().await.expect("cleanup failed");
}

/// The shorthand classes, bare, negated, bracketed with a member of their own
/// and folded, whose restatements are compared over every code point.
const SHORTHAND_CLASSES: &[&str] = &[r"\w", r"\W", r"\d", r"\D", r"\s", r"\S", r"[\w\-]", r"[^\d\s]", r"(?i)\w"];

/// Every scalar value PostgreSQL text can hold, U+0000 and the surrogates
/// aside, in code point order.
fn every_code_point() -> String {
    ('\u{1}'..=char::MAX).collect()
}

/// The code points the `regex` crate matches `class` against in `haystack`, as
/// an `int4multirange` literal, and how many there are.
///
/// One pass over every code point rather than one match per code point: a
/// single-character class matches each character on its own, so the matches
/// are exactly its members.
fn code_points_matched(class: &str, haystack: &str) -> (String, i64) {
    let regex = regex::Regex::new(class)
        .map_err(|error| format!("{class}: {error}"))
        .expect("the regex crate compiles the class");
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for found in regex.find_iter(haystack) {
        let Some(character) = found.as_str().chars().next() else { continue };
        let code_point = u32::from(character);
        match ranges.last_mut() {
            Some((_, end)) if *end + 1 == code_point => *end = code_point,
            Some(_) | None => ranges.push((code_point, code_point)),
        }
    }
    let count = ranges.iter().map(|(start, end)| i64::from(end - start + 1)).sum();
    let members: Vec<String> = ranges.iter().map(|(start, end)| format!("[{start},{end}]")).collect();
    (format!("{{{}}}", members.join(",")), count)
}

/// Every code point PostgreSQL classifies differently from the `regex` crate,
/// and how many it matches, under `collation`.
async fn code_point_disagreement(pool: &PgPool, regex: &str, matched: &str, collation: OperandCollation) -> (i64, i64) {
    sqlx::query_as::<_, (i64, i64)>(collation.code_point_statement())
        .bind(regex)
        .bind(matched)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("{regex:?}: {error}"))
        .expect("PostgreSQL compiles the restatement")
}

/// `\d`, `\w` and `\s`, their negations and their bracketed forms classify
/// every one of the 1,112,063 code points PostgreSQL text can hold alike in
/// both engines, whatever collation the operand carries.
#[tokio::test]
async fn unicode_pattern_engine_agreement_every_code_point_classifies_alike_under_every_shorthand_class() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");

    let haystack = every_code_point();
    for raw in SHORTHAND_CLASSES {
        let pattern = Pattern::parse(raw)
            .map_err(|error| format!("{raw}: {error:?}"))
            .expect("the pattern is admitted");
        let (matched, count) = code_points_matched(raw, &haystack);
        assert!(count > 0, "{raw} matches some code point");
        for collation in [OperandCollation::Bytewise, OperandCollation::DatabaseDefault, OperandCollation::UnicodeFast] {
            let disagreement = code_point_disagreement(scope.database_pool(), pattern.portable().as_str(), &matched, collation).await;
            assert_eq!(
                disagreement,
                (0, count),
                "{raw} under {collation:?}: (code points classified differently, matched)"
            );
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// The control for the shorthand classes: handed the client's text, PostgreSQL
/// classifies by the operand's collation, which under `COLLATE "C"` knows only
/// ASCII, so `\w` misses `č`, `\d` misses `٣` and `\s` misses U+00A0.
#[tokio::test]
async fn unicode_pattern_engine_agreement_postgresql_reading_the_clients_shorthand_classifies_only_ascii() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");

    for (raw, subject) in [(r"^\w+$", "čebela"), (r"^\d$", "\u{663}"), (r"^\s$", "\u{A0}")] {
        let pattern = Pattern::parse(raw).expect("admitted");
        let text = postgresql_selects(scope.database_pool(), pattern.as_str(), OperandCollation::Bytewise).await;
        let restated = postgresql_selects(scope.database_pool(), pattern.portable().as_str(), OperandCollation::Bytewise).await;
        assert!(!text.contains(subject), "PostgreSQL's own {raw} misses {subject:?}: {text:?}");
        assert!(restated.contains(subject), "the restatement of {raw} holds {subject:?}: {restated:?}");
        assert_eq!(restated, memory_selects(&pattern), "{raw}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// The control: handed the client's text, PostgreSQL applies its own
/// case-insensitive option, which folds only ASCII under `COLLATE "C"`. That is
/// the reading the restatement exists to replace.
#[tokio::test]
async fn unicode_pattern_engine_agreement_postgresql_reading_the_clients_text_folds_only_ascii() {
    let scope = TestEnvironment::from_suite("unicode_pattern").await.expect("isolated test environment");
    let pattern = Pattern::parse("(?i)čer").expect("admitted");

    let raw = postgresql_selects(scope.database_pool(), pattern.as_str(), OperandCollation::Bytewise).await;
    let restated = postgresql_selects(scope.database_pool(), pattern.portable().as_str(), OperandCollation::Bytewise).await;

    assert!(!raw.contains("Čer"), "PostgreSQL's own (?i) does not fold Č: {raw:?}");
    assert!(raw.contains("čEr"), "it folds the ASCII letters: {raw:?}");
    assert!(
        restated.contains("Čer") && restated.contains("ČER"),
        "the restatement folds Č as the regex crate does: {restated:?}"
    );
    assert_eq!(restated, memory_selects(&pattern));

    scope.finish().await.expect("cleanup failed");
}
