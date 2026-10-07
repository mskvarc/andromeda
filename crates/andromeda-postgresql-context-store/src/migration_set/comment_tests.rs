//! The comment rules for migration files, checked against every embedded
//! migration.
//!
//! A migration is frozen once released, so its comments must stay true for as
//! long as the file exists: they open with one header shape, cite nothing that
//! can move (code, documents, other migrations), and keep to the prose rules.
//! Only full-line comments are read; a `--` inside a string literal is SQL.

use crate::migration_set::declared_schema::every_migration;

/// The rule line that opens and closes every migration header.
const RULE_LINE: &str = "-- ============================================================================";

/// The start every `Spec:` header line has.
const SPEC_LINE_PREFIX: &str = "-- Spec: ETSI GS CIM 009 v1.9.1 clause";

/// The widest a comment line may be, in characters.
const MAXIMUM_COMMENT_WIDTH: usize = 80;

/// Text that cites code, a document or a working record.
const FORBIDDEN_FRAGMENTS: &[&str] = &["::", ".rs", "crates/", "andromeda-", "docs/", "Wave"];

/// Text the storage-bound tests parse out of the raw migration, comments
/// included.
const PARSED_FRAGMENTS: &[&str] = &["CONSTRAINT", "octet_length("];

/// A comment rule a migration breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommentRule {
    /// The header does not have the template's shape.
    HeaderShape,
    /// A comment cites code, a document, a working record or another migration.
    ForbiddenReference,
    /// A comment line is wider than the limit.
    LineTooWide,
    /// A comment uses a double hyphen as a dash.
    DoubleHyphenDash,
    /// A comment contains text the storage-bound tests would parse.
    ParsedFragment,
}

/// One broken comment rule.
#[derive(Debug, PartialEq, Eq)]
struct CommentViolation {
    /// The version of the migration concerned.
    version: i64,
    /// The one-based line number within the migration.
    line: usize,
    /// The line itself.
    text: String,
}

/// The prose of a full-line comment: the text after its `--`, or `None` when
/// the line is not a comment.
fn comment_prose(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("--").map(|prose| prose.strip_prefix(' ').unwrap_or(prose))
}

/// Whether `prose` contains a four-digit number with a leading zero, the shape
/// of a migration version.
fn cites_migration_number(prose: &str) -> bool {
    prose
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| word.len() == 4 && word.starts_with('0') && word.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Whether `prose` cites code, a document, a working record or another
/// migration.
fn cites_forbidden_reference(prose: &str) -> bool {
    FORBIDDEN_FRAGMENTS.iter().any(|fragment| prose.contains(fragment)) || cites_migration_number(prose) || prose.to_lowercase().contains("migration 0")
}

/// Whether the header of a migration at `version` with `lines` has the
/// template's shape: a rule line, `-- NNNN: Title`, a rule line, comment lines
/// only, a closing rule line, and a `Spec:` line of the fixed form if any.
fn header_is_well_formed(version: i64, lines: &[&str]) -> bool {
    let title_prefix = format!("-- {version:04}: ");
    let title_is_well_formed = lines
        .get(1)
        .and_then(|line| line.strip_prefix(title_prefix.as_str()))
        .is_some_and(|title| !title.is_empty() && !title.ends_with('.'));
    let body = lines.iter().skip(3).take_while(|line| **line != RULE_LINE);
    let body_is_comment_only = body.clone().all(|line| line.starts_with("--"));
    let spec_is_well_formed = body
        .clone()
        .filter(|line| line.starts_with("-- Spec:"))
        .all(|line| line.starts_with(SPEC_LINE_PREFIX));
    let header_is_closed = lines.iter().skip(3).any(|line| *line == RULE_LINE);
    lines.first() == Some(&RULE_LINE)
        && title_is_well_formed
        && lines.get(2) == Some(&RULE_LINE)
        && body_is_comment_only
        && spec_is_well_formed
        && header_is_closed
}

/// Whether `line`, found at the one-based `line_number`, breaks `rule`. The
/// header shape is judged per migration, not per line.
fn line_breaks(rule: CommentRule, line_number: usize, line: &str) -> bool {
    let Some(prose) = comment_prose(line) else {
        return false;
    };
    match rule {
        CommentRule::HeaderShape => false,
        // The title line carries the migration's own number.
        CommentRule::ForbiddenReference => line_number != 2 && cites_forbidden_reference(prose),
        CommentRule::LineTooWide => line.chars().count() > MAXIMUM_COMMENT_WIDTH,
        CommentRule::DoubleHyphenDash => prose.contains(" -- "),
        CommentRule::ParsedFragment => PARSED_FRAGMENTS.iter().any(|fragment| prose.contains(fragment)),
    }
}

/// Every violation of `rule` across the embedded migrations.
fn violations_of(rule: CommentRule) -> Vec<CommentViolation> {
    every_migration()
        .into_iter()
        .flat_map(|migration| {
            let lines: Vec<&str> = migration.sql.as_str().lines().collect();
            let header_violation = (rule == CommentRule::HeaderShape && !header_is_well_formed(migration.version, &lines)).then(|| CommentViolation {
                version: migration.version,
                line: 1,
                text: lines.first().map_or_else(String::new, |line| (*line).to_owned()),
            });
            let line_violations = lines
                .iter()
                .enumerate()
                .filter(|(index, line)| line_breaks(rule, index + 1, line))
                .map(|(index, line)| CommentViolation {
                    version: migration.version,
                    line: index + 1,
                    text: (*line).to_owned(),
                })
                .collect::<Vec<_>>();
            header_violation.into_iter().chain(line_violations)
        })
        .collect()
}

#[test]
fn every_migration_opens_with_the_header_template() {
    let violations = violations_of(CommentRule::HeaderShape);
    assert!(violations.is_empty(), "migrations whose header breaks the template: {violations:#?}");
}

#[test]
fn no_migration_comment_cites_code_documents_or_another_migration() {
    let violations = violations_of(CommentRule::ForbiddenReference);
    assert!(violations.is_empty(), "comments citing something that can move: {violations:#?}");
}

#[test]
fn no_migration_comment_line_is_wider_than_eighty_columns() {
    let violations = violations_of(CommentRule::LineTooWide);
    assert!(
        violations.is_empty(),
        "comment lines wider than {MAXIMUM_COMMENT_WIDTH} columns: {violations:#?}"
    );
}

#[test]
fn no_migration_comment_uses_a_double_hyphen_as_a_dash() {
    let violations = violations_of(CommentRule::DoubleHyphenDash);
    assert!(violations.is_empty(), "comments using ` -- ` as a dash: {violations:#?}");
}

#[test]
fn no_migration_comment_contains_text_the_storage_bound_tests_parse() {
    let violations = violations_of(CommentRule::ParsedFragment);
    assert!(violations.is_empty(), "comments containing `CONSTRAINT` or `octet_length(`: {violations:#?}");
}

#[test]
fn a_header_whose_title_names_another_version_is_refused() {
    let lines = [RULE_LINE, "-- 0007: Geometry-to-geography cast", RULE_LINE, "-- Purpose.", RULE_LINE];
    assert!(header_is_well_formed(7, &lines));
    assert!(!header_is_well_formed(8, &lines));
}

#[test]
fn a_header_with_a_malformed_spec_line_or_no_closing_rule_is_refused() {
    let unpinned_spec = [RULE_LINE, "-- 0001: Tenant registry", RULE_LINE, "-- Spec: clause 6.3.14", RULE_LINE];
    let unclosed = [RULE_LINE, "-- 0001: Tenant registry", RULE_LINE, "-- Purpose."];
    assert!(!header_is_well_formed(1, &unpinned_spec));
    assert!(!header_is_well_formed(1, &unclosed));
}

#[test]
fn citations_of_code_documents_records_and_migrations_are_detected() {
    assert!(cites_forbidden_reference("see `crates/andromeda-unit-catalog`"));
    assert!(cites_forbidden_reference("`queue::claimable_at`"));
    assert!(cites_forbidden_reference("the live-only 0029"));
    assert!(cites_forbidden_reference("added by migration 0008"));
    assert!(cites_forbidden_reference("measured in Wave 3"));
    assert!(cites_forbidden_reference("see `docs/unit-handling.md`"));
}

#[test]
fn specification_clauses_codes_and_sentinels_are_not_forbidden_references() {
    assert!(!cites_forbidden_reference("clause 5.2.12 and Table 5.2.12-1"));
    assert!(!cites_forbidden_reference("`{0}` on both attribute axes"));
    assert!(!cites_forbidden_reference("RFC 7946 section 3.1.1"));
    assert!(!cites_forbidden_reference("a comparison beyond 2^53"));
}

#[test]
fn a_double_hyphen_inside_a_string_literal_is_not_a_comment() {
    assert!(!line_breaks(
        CommentRule::DoubleHyphenDash,
        10,
        "        RAISE NOTICE 'requires preloading -- skipping';"
    ));
    assert!(line_breaks(CommentRule::DoubleHyphenDash, 10, "-- a dash -- inside prose"));
}
