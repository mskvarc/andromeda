//! The workspace-wide guard on client-facing error `detail` wording.
//!
//! Every RFC 7807 `detail` an NGSI-LD client sees is the `Display` of a typed
//! error, rendered by a thiserror `#[error("...")]` attribute, plus the handful
//! of literals still passed to [`ClassifiedReport::public`] at the HTTP
//! boundary. This guard reads both out of the syntax tree and holds each against
//! the two rules that cannot be expressed by the type system:
//!
//! - the prose begins lowercase, so casing does not drift between crates. A
//!   message that opens with an interpolation (`{name} ...`) or with one of the
//!   spec tokens whose casing ETSI GS CIM 009 fixes (`Property`, `observedAt`,
//!   ...) is exempt on that first token only;
//! - no value is interpolated through `Debug` (`{x:?}`), which leaks Rust type
//!   structure into text a client reads.
//!
//! Detection is deny-by-default over a shrinking baseline, like the raw-SQL and
//! type-erased-error gates: the messages present when the gate was introduced
//! are tolerated, a new violation anywhere fails immediately, and a baseline
//! entry that no longer violates fails too, so the list can only shrink.

use crate::source_scan::{item_is_test_only, workspace_sources};
use andromeda_ngsi_ld_vocabulary::COMPACT_TO_EXPANDED;
use std::{collections::BTreeSet, path::Path};
use syn::{
    Expr,
    ExprCall,
    ExprLit,
    ExprMacro,
    Item,
    Lit,
    Macro,
    Meta,
    visit::{Visit, visit_expr_call, visit_item},
};

/// Single-capital spec type names permitted to open a message, beyond those the
/// NGSI-LD vocabulary supplies and those recognised structurally.
///
/// Acronyms (`TLS`, `MQTT`, `NGSI-LD`) and mixed-case identifiers (`GeoJSON`,
/// `PostgreSQL`, `ContextSourceRegistration`) are recognised by their shape in
/// [`opener_is_proper_noun`]; the vocabulary supplies the spec's own terms. What
/// remains are the API resource types the spec writes as a single capitalised
/// word, which shape alone cannot tell apart from ordinary prose. HTTP header
/// field names are deliberately absent: HTTP/2 onward lower-cases them on the
/// wire, so a message names a header as `content-length`, not `Content-Length`.
const SUPPLEMENTARY_TOKENS: &[&str] = &["Query", "Subscription"];

/// Every token allowed to open a client-facing message uppercase.
///
/// The NGSI-LD compact term names whose fixed casing begins with a capital
/// (`Property`, `Relationship`, `GeoProperty`, `DateTime`, ...) come straight
/// from the vocabulary crate, so the allowlist tracks the spec's own casing and
/// cannot drift from it. Lower-case terms (`observedAt`, `datasetId`) already
/// satisfy the rule and need no exemption.
#[must_use]
pub fn spec_token_openers() -> Vec<&'static str> {
    COMPACT_TO_EXPANDED
        .iter()
        .map(|(compact, _)| *compact)
        .filter(|term| term.starts_with(|character: char| character.is_ascii_uppercase()))
        .chain(SUPPLEMENTARY_TOKENS.iter().copied())
        .collect()
}

/// A detail literal that breaks a wording rule, identified for the baseline.
///
/// The identity is the owning file and the verbatim message, so a fix that
/// changes the wording retires the baseline entry and the gate notices.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DetailViolation {
    /// Workspace-relative path of the file the message lives in.
    pub path: String,
    /// The verbatim message literal.
    pub message: String,
    /// Which rule the message breaks.
    pub rule: DetailRule,
}

/// The wording rule a detail literal broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DetailRule {
    /// The prose opens with an uppercase letter that is not a fixed spec token.
    UppercaseProse,
    /// A value is interpolated through `Debug` rather than `Display`.
    DebugInterpolation,
    /// The prose cites an ETSI clause, which belongs in a code comment.
    ClauseCitation,
}

impl DetailViolation {
    /// The single-line baseline key for this violation.
    #[must_use]
    pub fn key(&self) -> String {
        let rule = match self.rule {
            DetailRule::UppercaseProse => "uppercase-prose",
            DetailRule::DebugInterpolation => "debug-interpolation",
            DetailRule::ClauseCitation => "clause-citation",
        };
        format!("{}\t{rule}\t{}", self.path, self.message)
    }
}

/// Assert that no unsanctioned, non-baseline detail literal breaks a wording rule.
///
/// # Panics
///
/// Panics on a new violation, on a baseline entry that no longer violates, or
/// when the workspace source tree cannot be read or parsed.
pub fn assert_detail_messages_are_well_formed(workspace_root: &str, baseline: &[&str]) {
    let keys: BTreeSet<String> = detail_violations(workspace_root).iter().map(DetailViolation::key).collect();
    let baseline: BTreeSet<&str> = baseline.iter().copied().collect();

    let unexpected: Vec<&str> = keys.iter().map(String::as_str).filter(|key| !baseline.contains(key)).collect();
    let stale: Vec<&str> = baseline.iter().copied().filter(|key| !keys.contains(*key)).collect();

    assert!(
        unexpected.is_empty(),
        "client-facing detail wording outside the sanctioned baseline; make the prose lowercase and interpolate with Display, not Debug:\n{}",
        unexpected.join("\n")
    );
    assert!(
        stale.is_empty(),
        "these baseline detail messages no longer violate a wording rule; delete them from the baseline:\n{}",
        stale.join("\n")
    );
}

/// Every production detail literal that breaks a wording rule, sorted.
#[must_use]
pub fn detail_violations(workspace_root: &str) -> Vec<DetailViolation> {
    let mut violations = BTreeSet::new();
    for source in workspace_sources(Path::new(workspace_root)) {
        if source.is_black_box_suite() || source.workspace_path.contains("/tests/") || source.workspace_path.ends_with("build.rs") {
            continue;
        }
        let mut visitor = DetailVisitor {
            path: &source.workspace_path,
            violations: &mut violations,
        };
        visitor.visit_file(&source.syntax);
    }
    violations.into_iter().collect()
}

/// Records the wording violations found in one parsed source.
struct DetailVisitor<'a> {
    /// Workspace-relative path of the source under inspection.
    path: &'a str,
    /// Accumulated violations across the file.
    violations: &'a mut BTreeSet<DetailViolation>,
}

impl DetailVisitor<'_> {
    /// Record every rule one collected message breaks.
    fn inspect(&mut self, message: &str) {
        for rule in message_violations(message) {
            self.violations.insert(DetailViolation {
                path: self.path.to_string(),
                message: message.to_string(),
                rule,
            });
        }
    }
}

impl<'ast> Visit<'ast> for DetailVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if let Some(message) = thiserror_message(attribute) {
            self.inspect(&message);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Some(message) = classified_public_detail(call) {
            self.inspect(&message);
        }
        visit_expr_call(self, call);
    }
}

/// The leading format string of a `#[error("...")]` attribute, if any.
///
/// `#[error(transparent)]` and `#[error(code = ...)]` carry no literal message;
/// `#[error("text", ..args)]` carries the message as its first token.
fn thiserror_message(attribute: &syn::Attribute) -> Option<String> {
    if !attribute.path().is_ident("error") {
        return None;
    }
    let Meta::List(list) = &attribute.meta else {
        return None;
    };
    let mut tokens = list.tokens.clone().into_iter();
    // `TokenTree` is `#[non_exhaustive]`, so the one shape that carries a message
    // is asked for directly rather than matched against a catch-all arm.
    if let proc_macro2::TokenTree::Literal(literal) = tokens.next()? {
        return literal_string(&literal.to_string());
    }
    None
}

/// The detail argument of a call that publishes a client-facing detail from a
/// literal: `ClassifiedReport::public(category, detail)`, `public_binding`, or
/// the route-level `public_error(category, detail, uri)`. All three take the
/// detail second, as a string literal or a `format!` with a literal template.
fn classified_public_detail(call: &ExprCall) -> Option<String> {
    let Expr::Path(path) = call.func.as_ref() else {
        return None;
    };
    let last = path.path.segments.last()?;
    if !matches!(last.ident.to_string().as_str(), "public" | "public_binding" | "public_error") {
        return None;
    }
    let detail = call.args.iter().nth(1)?;
    literal_or_format_template(detail)
}

/// The template text behind a string literal or a `format!(...)` expression.
///
/// `Expr` is `#[non_exhaustive]`, so the two shapes a detail literal takes are
/// asked for directly rather than matched against a catch-all arm.
fn literal_or_format_template(expr: &Expr) -> Option<String> {
    if let Expr::Lit(ExprLit { lit: Lit::Str(value), .. }) = expr {
        return Some(value.value());
    }
    if let Expr::Macro(ExprMacro { mac, .. }) = expr {
        return format_macro_template(mac);
    }
    None
}

/// The leading template literal of a `format!`/`write!`-shaped macro.
fn format_macro_template(mac: &Macro) -> Option<String> {
    if !mac.path.is_ident("format") {
        return None;
    }
    // `TokenTree` is `#[non_exhaustive]`, so the leading literal is asked for
    // directly rather than matched against a catch-all arm.
    if let proc_macro2::TokenTree::Literal(literal) = mac.tokens.clone().into_iter().next()? {
        return literal_string(&literal.to_string());
    }
    None
}

/// The decoded value of a Rust string-literal token, or `None` when the token is
/// not a plain string literal.
fn literal_string(token: &str) -> Option<String> {
    syn::parse_str::<syn::LitStr>(token).ok().map(|literal| literal.value())
}

/// Every wording rule a message breaks, in a stable order.
#[must_use]
pub fn message_violations(message: &str) -> Vec<DetailRule> {
    let mut rules = Vec::new();
    if !prose_starts_lowercase(message, &spec_token_openers()) {
        rules.push(DetailRule::UppercaseProse);
    }
    if has_debug_interpolation(message) {
        rules.push(DetailRule::DebugInterpolation);
    }
    if has_clause_citation(message) {
        rules.push(DetailRule::ClauseCitation);
    }
    rules
}

/// Whether a message cites an ETSI clause or the ETSI standard by name.
///
/// Clause numbers and the standard's designation are internal provenance a
/// client cannot act on and cannot look up; they belong in a code comment. RFC
/// references (`RFC 7946`, `RFC 3339`) are deliberately not matched: those name
/// a public format a client can consult, and the best messages cite them.
#[must_use]
pub fn has_clause_citation(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("clause") || lower.contains("etsi")
}

/// Whether a message's prose opens acceptably: with an interpolation, a fixed
/// spec token, a proper noun, punctuation, or a lowercase letter.
#[must_use]
pub fn prose_starts_lowercase(message: &str, spec_tokens: &[&str]) -> bool {
    let trimmed = message.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('{') {
        return true;
    }
    if trimmed.chars().next().is_none_or(|first| !first.is_ascii_uppercase()) {
        return true;
    }
    if spec_tokens.iter().any(|token| starts_with_token(trimmed, token)) {
        return true;
    }
    let opener = trimmed.split_whitespace().next().unwrap_or_default();
    opener_is_proper_noun(opener)
}

/// Whether `text` opens with `token` as a whole word rather than a longer
/// identifier that merely shares its prefix.
fn starts_with_token(text: &str, token: &str) -> bool {
    text.strip_prefix(token)
        .is_some_and(|rest| rest.chars().next().is_none_or(|next| !next.is_alphanumeric() && next != '_'))
}

/// Whether a capitalised opener is a proper noun rather than ordinary prose.
///
/// Two shapes are proper nouns no message should lowercase: an acronym, whose
/// letters are all uppercase with at least two of them (`TLS`, `MQTT`,
/// `NGSI-LD`, `EWKB`), and a single-word mixed-case identifier, which carries an
/// uppercase letter somewhere after the first alongside a lowercase one
/// (`GeoJSON`, `PostgreSQL`, `DateTime`, `RegistrationInfo`). A hyphenated
/// Title-Case token (`Content-Range`) is an HTTP header field name, which
/// HTTP/2 lower-cases, so it is not exempt. A single capitalised word with an
/// all-lowercase tail (`Invalid`, `Failed`, `Entity`) is ordinary prose, and a
/// lone capital (`Q`) is neither acronym nor mixed-case, so `Q expression:` is
/// correctly rejected in favour of `q`.
fn opener_is_proper_noun(opener: &str) -> bool {
    let trimmed = opener.trim_end_matches(|character: char| !character.is_alphanumeric());
    let letters: Vec<char> = trimmed.chars().filter(|character| character.is_alphabetic()).collect();
    let all_uppercase = letters.len() >= 2 && letters.iter().all(char::is_ascii_uppercase);
    let has_lowercase = letters.iter().any(char::is_ascii_lowercase);
    let has_trailing_uppercase = trimmed.chars().skip(1).any(|character| character.is_ascii_uppercase());
    let hyphenated = trimmed.contains('-');
    all_uppercase || (has_lowercase && has_trailing_uppercase && !hyphenated)
}

/// Whether a format template interpolates any value through `Debug`.
///
/// A `Debug` interpolation carries a `?` inside its format specification, after
/// the `:` separator: `{x:?}`, `{:?}`, `{x:#?}`. `{{` and `}}` are escaped
/// braces and open no interpolation.
#[must_use]
pub fn has_debug_interpolation(message: &str) -> bool {
    let mut chars = message.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
            }
            '{' => {
                let mut spec = String::new();
                let mut saw_colon = false;
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                    if inner == ':' {
                        saw_colon = true;
                        continue;
                    }
                    if saw_colon {
                        spec.push(inner);
                    }
                }
                if spec.contains('?') {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use crate::detail_message::{DetailRule, has_debug_interpolation, message_violations, prose_starts_lowercase, spec_token_openers};

    #[test]
    fn the_vocabulary_supplies_uppercase_openers() {
        let openers = spec_token_openers();
        assert!(openers.contains(&"Property"));
        assert!(openers.contains(&"Relationship"));
        assert!(openers.contains(&"GeoProperty"));
    }

    #[test]
    fn lowercase_prose_is_accepted() {
        assert!(prose_starts_lowercase("invalid attribute name", &spec_token_openers()));
        assert!(prose_starts_lowercase("geoJSON position has too few ordinates", &spec_token_openers()));
    }

    #[test]
    fn an_interpolation_opener_is_accepted() {
        assert!(prose_starts_lowercase("{name} is not a known attribute", &spec_token_openers()));
    }

    #[test]
    fn a_fixed_spec_token_may_open_a_message() {
        assert!(prose_starts_lowercase("Property expected a value member", &spec_token_openers()));
        assert!(prose_starts_lowercase("observedAt must be a DateTime", &spec_token_openers()));
        assert!(prose_starts_lowercase(
            "ContextSourceRegistration is missing an endpoint",
            &spec_token_openers()
        ));
    }

    #[test]
    fn a_prefix_of_a_spec_token_is_not_the_token() {
        assert!(!prose_starts_lowercase("Propertyish thing is wrong", &spec_token_openers()));
    }

    #[test]
    fn capitalized_prose_is_rejected() {
        assert!(!prose_starts_lowercase("Invalid attribute name", &spec_token_openers()));
        assert!(!prose_starts_lowercase("Failed to expand document", &spec_token_openers()));
        assert!(!prose_starts_lowercase("Q expression: empty input", &spec_token_openers()));
    }

    #[test]
    fn acronyms_and_mixed_case_proper_nouns_may_open_a_message() {
        let openers = spec_token_openers();
        assert!(prose_starts_lowercase("TLS handshake failed", &openers));
        assert!(prose_starts_lowercase("MQTT URI missing host", &openers));
        assert!(prose_starts_lowercase("NGSI-LD reference must not be empty", &openers));
        assert!(prose_starts_lowercase("EWKB buffer is empty", &openers));
        assert!(prose_starts_lowercase("GeoJSON position has too few ordinates", &openers));
        assert!(prose_starts_lowercase("PostgreSQL maintenance failed", &openers));
    }

    #[test]
    fn capitalized_http_header_names_are_rejected() {
        let openers = spec_token_openers();
        assert!(!prose_starts_lowercase("Content-Range must separate range and size", &openers));
        assert!(!prose_starts_lowercase("Host header does not identify a valid origin", &openers));
        assert!(!prose_starts_lowercase("Via element is empty", &openers));
    }

    #[test]
    fn debug_interpolation_is_detected() {
        assert!(has_debug_interpolation("types {types:?} do not match"));
        assert!(has_debug_interpolation("value {0:?}"));
        assert!(has_debug_interpolation("value {:#?}"));
    }

    #[test]
    fn display_interpolation_is_not_flagged() {
        assert!(!has_debug_interpolation("value {value} is wrong"));
        assert!(!has_debug_interpolation("count {n:>3} exceeds limit"));
        assert!(!has_debug_interpolation("escaped braces {{ and }} are literal"));
    }

    #[test]
    fn clause_citations_are_detected_but_rfc_references_are_not() {
        use crate::detail_message::has_clause_citation;
        assert!(has_clause_citation("a type is required (clause 4.5.1)"));
        assert!(has_clause_citation("clause 4.18 uses the null only for a deleted scope"));
        assert!(has_clause_citation("ETSI GS CIM 009 excludes this geometry"));
        assert!(!has_clause_citation("a position needs at least two ordinates (RFC 7946 section 3.1.1)"));
        assert!(!has_clause_citation("the value is not an RFC 3339 timestamp"));
    }

    #[test]
    fn message_violations_reports_each_broken_rule() {
        assert_eq!(message_violations("invalid attribute {name}"), Vec::new());
        assert_eq!(
            message_violations("Invalid attribute {name:?}"),
            vec![DetailRule::UppercaseProse, DetailRule::DebugInterpolation]
        );
        assert_eq!(message_violations("a type is required (clause 4.5.1)"), vec![DetailRule::ClauseCitation]);
    }
}
