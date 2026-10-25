//! Reading one Rust source for the ways it converts between text and time values.
//!
//! The reading is over the token stream of the whole file, so a conversion
//! written inside a macro call (`assert_eq!(at.to_rfc3339(), ...)`) is seen as
//! readily as one in plain code; a binding annotated with a `chrono` type and
//! initialised by a bare `parse` is read from the syntax tree, where the
//! annotation and the initialiser are related.

use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use syn::{
    Expr,
    File,
    Local,
    Pat,
    Type,
    spanned::Spanned,
    visit::{Visit, visit_local},
};

/// `chrono` names that convert between text and an instant, or round one below
/// the second, which only the lexicon's `time` module may use.
const FORBIDDEN_NAMES: [&str; 18] = [
    "parse_from_rfc3339",
    "parse_from_rfc2822",
    "parse_from_str",
    "parse_and_remainder",
    "datetime_from_str",
    "to_rfc3339",
    "to_rfc3339_opts",
    "to_rfc2822",
    "format_with_items",
    "trunc_subsecs",
    "round_subsecs",
    "duration_trunc",
    "duration_round",
    "with_nanosecond",
    "SecondsFormat",
    "FixedOffset",
    "SubsecRound",
    "StrftimeItems",
];

/// The `chrono` types a text conversion produces.
const CHRONO_TIME_TYPES: [&str; 4] = ["DateTime", "NaiveDate", "NaiveTime", "NaiveDateTime"];

/// Fragments of a string literal that spell a strftime directive or a regular
/// expression of a time value form.
const TEMPLATE_MARKERS: [&str; 8] = ["%Y", "%H", "%M", "%S", "%.f", "[0-9]{2}:", "[0-5][0-9]", "HH24"];

/// The one marker the PostgreSQL lift of stored time text may keep.
const SQL_TEMPLATE_MARKER: &str = "HH24";

/// Every finding in `file`, as `(line, subject)`.
///
/// `keeps_sql_template` exempts the `to_char` template of the PostgreSQL lift.
pub(crate) fn time_handling_findings(file: &File, keeps_sql_template: bool) -> Vec<(usize, String)> {
    let mut findings = Vec::new();
    read_tokens(file.to_token_stream(), keeps_sql_template, &mut findings);
    let mut bindings = AnnotatedParses::default();
    bindings.visit_file(file);
    findings.extend(bindings.findings);
    findings.sort();
    findings.dedup();
    findings
}

/// Read one token stream, descending into every group.
fn read_tokens(stream: TokenStream, keeps_sql_template: bool, findings: &mut Vec<(usize, String)>) {
    let tokens: Vec<TokenTree> = stream.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => read_tokens(group.stream(), keeps_sql_template, findings),
            TokenTree::Ident(ident) => {
                let name = ident.to_string();
                let line = ident.span().start().line;
                if FORBIDDEN_NAMES.contains(&name.as_str()) {
                    findings.push((line, format!("`{name}` converts or rounds a time value outside its owner")));
                } else if name == "parse" && turbofish_names_a_chrono_type(tokens.get(index + 1..).unwrap_or_default()) {
                    findings.push((line, "`parse::<chrono type>` reads a time value outside its owner".to_owned()));
                } else if name == "from_str" && preceded_by_chrono_type(tokens.get(..index).unwrap_or_default()) {
                    findings.push((line, "`<chrono type>::from_str` reads a time value outside its owner".to_owned()));
                }
            }
            TokenTree::Literal(literal) => {
                let text = literal.to_string();
                let spelled = TEMPLATE_MARKERS
                    .iter()
                    .filter(|marker| !(keeps_sql_template && **marker == SQL_TEMPLATE_MARKER))
                    .find(|marker| text.contains(**marker));
                if let Some(marker) = spelled {
                    findings.push((
                        literal.span().start().line,
                        format!("a literal spells a time value template (`{marker}`) outside its owner"),
                    ));
                }
            }
            TokenTree::Punct(_) => {}
        }
    }
}

/// Whether the tokens after `parse` are a turbofish naming a `chrono` time type:
/// `::<DateTime<Utc>>`, `::<chrono::NaiveDate>`.
fn turbofish_names_a_chrono_type(following: &[TokenTree]) -> bool {
    let opens = matches!(following, [TokenTree::Punct(first), TokenTree::Punct(second), TokenTree::Punct(angle), ..]
        if first.as_char() == ':' && second.as_char() == ':' && angle.as_char() == '<');
    opens
        && following
            .iter()
            .skip(3)
            .take_while(|token| !matches!(token, TokenTree::Punct(punct) if punct.as_char() == '>' || punct.as_char() == '('))
            .any(|token| matches!(token, TokenTree::Ident(ident) if CHRONO_TIME_TYPES.contains(&ident.to_string().as_str())))
}

/// Whether the path before `from_str` names a `chrono` time type, however it
/// is qualified: `DateTime::`, `DateTime::<Utc>::`, `<DateTime<Utc>>::`,
/// `chrono::NaiveDate::`.
///
/// The path is read backwards over the tokens a type path is made of (`::`,
/// names, angle brackets and the commas between generic arguments) and ends at
/// the first token that cannot belong to it.
fn preceded_by_chrono_type(preceding: &[TokenTree]) -> bool {
    preceding
        .iter()
        .rev()
        .take_while(|token| match token {
            TokenTree::Punct(punct) => matches!(punct.as_char(), ':' | '<' | '>' | ','),
            TokenTree::Ident(_) => true,
            TokenTree::Group(_) | TokenTree::Literal(_) => false,
        })
        .any(|token| matches!(token, TokenTree::Ident(ident) if CHRONO_TIME_TYPES.contains(&ident.to_string().as_str())))
}

/// Collects bindings annotated with a `chrono` time type and initialised by `parse`.
#[derive(Default)]
struct AnnotatedParses {
    /// The findings, as `(line, subject)`.
    findings: Vec<(usize, String)>,
}

impl<'ast> Visit<'ast> for AnnotatedParses {
    fn visit_local(&mut self, local: &'ast Local) {
        if let (Pat::Type(annotated), Some(init)) = (&local.pat, &local.init)
            && names_a_chrono_type(&annotated.ty)
            && ends_in_parse(&init.expr)
        {
            self.findings.push((
                local.span().start().line,
                "a binding typed as a chrono time type is read by `parse` outside its owner".to_owned(),
            ));
        }
        visit_local(self, local);
    }
}

/// Whether a type is, or wraps, a `chrono` time type.
fn names_a_chrono_type(ty: &Type) -> bool {
    ty.to_token_stream()
        .into_iter()
        .any(|token| matches!(token, TokenTree::Ident(ident) if CHRONO_TIME_TYPES.contains(&ident.to_string().as_str())))
}

/// The calls an initialiser may wrap a `parse` in.
const PARSE_WRAPPERS: [&str; 4] = ["unwrap", "expect", "map_err", "ok"];

/// Whether an initialiser is a `parse` call, possibly behind `?`, `unwrap`,
/// `expect` or an error mapping.
fn ends_in_parse(expr: &Expr) -> bool {
    if let Expr::MethodCall(call) = expr {
        return call.method == "parse" || (PARSE_WRAPPERS.iter().any(|name| call.method == *name) && ends_in_parse(&call.receiver));
    }
    if let Expr::Try(attempt) = expr {
        return ends_in_parse(&attempt.expr);
    }
    if let Expr::Paren(inner) = expr {
        return ends_in_parse(&inner.expr);
    }
    false
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::time_handling::source_reading::time_handling_findings;

    fn findings(source: &str) -> Vec<String> {
        let file = syn::parse_file(source).unwrap_or_else(|error| panic!("the fixture parses: {error}"));
        time_handling_findings(&file, false).into_iter().map(|(_, subject)| subject).collect()
    }

    #[test]
    fn chronos_own_rfc_3339_reader_is_a_finding_in_code_and_in_macros() {
        assert_eq!(findings("fn f() { let _ = DateTime::parse_from_rfc3339(\"x\"); }").len(), 1);
        assert_eq!(findings("fn f() { assert_eq!(at.to_rfc3339(), \"x\"); }").len(), 1);
    }

    #[test]
    fn an_import_of_a_rounding_trait_is_a_finding() {
        assert_eq!(findings("use chrono::{SubsecRound, Utc};").len(), 1);
    }

    #[test]
    fn a_turbofish_parse_into_a_chrono_type_is_a_finding() {
        assert_eq!(findings("fn f() { let _ = \"x\".parse::<DateTime<Utc>>(); }").len(), 1);
        assert_eq!(findings("fn f() { let _ = text.parse::<chrono::NaiveTime>(); }").len(), 1);
        pretty_assertions::assert_eq!(findings("fn f() { let _ = \"x\".parse::<NgsiLdDateTime>(); }"), Vec::<String>::new());
        pretty_assertions::assert_eq!(findings("fn f() { let _ = \"x\".parse::<Vec<u8>>(); }"), Vec::<String>::new());
    }

    #[test]
    fn a_from_str_on_a_chrono_type_is_a_finding() {
        assert_eq!(findings("fn f() { let _ = NaiveDate::from_str(\"x\"); }").len(), 1);
        assert_eq!(findings("fn f() { let _ = DateTime::<Utc>::from_str(\"x\"); }").len(), 1);
        assert_eq!(findings("fn f() { let _ = <DateTime<Utc>>::from_str(\"x\"); }").len(), 1);
        assert_eq!(findings("fn f() { let _ = chrono::NaiveTime::from_str(\"x\"); }").len(), 1);
        pretty_assertions::assert_eq!(findings("fn f() { let _ = NgsiLdDate::from_str(\"x\"); }"), Vec::<String>::new());
    }

    #[test]
    fn a_binding_typed_as_a_chrono_type_and_read_by_parse_is_a_finding() {
        assert_eq!(findings("fn f() { let at: DateTime<Utc> = text.parse().expect(\"x\"); }").len(), 1);
        pretty_assertions::assert_eq!(findings("fn f() { let at: DateTime<Utc> = Utc::now(); }"), Vec::<String>::new());
    }

    #[test]
    fn a_strftime_template_or_a_time_regex_is_a_finding() {
        assert_eq!(findings("const F: &str = \"%Y-%m-%dT%H:%M:%SZ\";").len(), 1);
        assert_eq!(findings("const P: &str = \"^[0-9]{2}:[0-9]{2}:[0-9]{2}Z$\";").len(), 1);
        assert_eq!(findings("fn f() { let _ = t.format(\"%H:%M\"); }").len(), 1);
    }

    #[test]
    fn the_sql_lift_keeps_its_to_char_template_and_nothing_else() {
        let file = syn::parse_file("const A: &str = \"HH24:MI:SS.US\"; const B: &str = \"%H\";").unwrap_or_else(|error| panic!("{error}"));
        pretty_assertions::assert_eq!(time_handling_findings(&file, true).len(), 1);
        pretty_assertions::assert_eq!(time_handling_findings(&file, false).len(), 2);
    }

    #[test]
    fn constructing_and_comparing_instants_is_clean() {
        pretty_assertions::assert_eq!(
            findings("fn f() { let at = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0); let later = at + TimeDelta::seconds(1); }"),
            Vec::<String>::new()
        );
    }
}
