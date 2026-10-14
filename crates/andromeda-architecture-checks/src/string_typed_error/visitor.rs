//! The syntax-tree walk that finds string-typed error sites in one source.

use crate::{
    source_scan::{is_test_only, item_is_test_only},
    string_typed_error::{
        StringTypedErrorKind,
        StringTypedErrorSite,
        formatted_text::{FormattedText, bound_idents},
        string_shape::{is_string_conversion_path, is_string_expression, type_mentions_string},
        workspace_index::{TEXT_MEMBER_NAMES, WorkspaceIndex},
    },
};
use quote::ToTokens;
use std::{collections::BTreeSet, mem};
use syn::{
    Expr,
    ExprCall,
    ExprMethodCall,
    ExprStruct,
    Fields,
    File,
    GenericArgument,
    Ident,
    ImplItemFn,
    Item,
    ItemEnum,
    ItemFn,
    ItemImpl,
    ItemStruct,
    Local,
    Member,
    Path,
    PathArguments,
    TraitItemFn,
    Type,
    TypePath,
    UseRename,
    spanned::Spanned,
    visit::{
        Visit,
        visit_expr_call,
        visit_expr_method_call,
        visit_expr_struct,
        visit_impl_item_fn,
        visit_item,
        visit_item_enum,
        visit_item_fn,
        visit_item_impl,
        visit_item_struct,
        visit_local,
        visit_trait_item_fn,
        visit_type_path,
    },
};

/// Foreign error constructors that accept a message, named by their last path
/// segment and reached through an `Error` type: `serde::de::Error::custom`,
/// `io::Error::other`, `io::Error::new`.
const FOREIGN_ERROR_CONSTRUCTORS: &[&str] = &["custom", "new", "other"];

/// Collects the string-typed error sites of one parsed source.
pub(crate) struct StringTypedErrorVisitor<'a> {
    /// Workspace-relative path of the source under inspection.
    path: &'a str,
    /// What the workspace declares about error carriers and text sinks.
    index: &'a WorkspaceIndex,
    /// Names this source imports a foreign `Error` trait or type under
    /// (`use serde::de::Error as DeserializeError`).
    error_aliases: BTreeSet<String>,
    /// The type of the `impl` block being walked, which `Self` names.
    impl_owner: Option<&'a Ident>,
    /// Locals of the function being walked that are bound to formatted text.
    formatted_locals: BTreeSet<String>,
    /// Start positions of the values already reported, so a value reached
    /// through two sinks is reported once.
    reported_values: BTreeSet<(usize, usize)>,
    /// Sites found so far.
    sites: Vec<StringTypedErrorSite>,
}

impl<'a> StringTypedErrorVisitor<'a> {
    /// Every string-typed error site in one parsed source.
    pub(crate) fn inspect(path: &'a str, syntax: &'a File, index: &'a WorkspaceIndex) -> Vec<StringTypedErrorSite> {
        let mut aliases = ErrorAliasCollector { names: BTreeSet::new() };
        aliases.visit_file(syntax);
        let mut visitor = StringTypedErrorVisitor {
            path,
            index,
            error_aliases: aliases.names,
            impl_owner: None,
            formatted_locals: BTreeSet::new(),
            reported_values: BTreeSet::new(),
            sites: Vec::new(),
        };
        visitor.visit_file(syntax);
        visitor.sites
    }

    /// Record one site at the start of `node`.
    fn record(&mut self, node: &impl Spanned, kind: StringTypedErrorKind, subject: String) {
        self.sites.push(StringTypedErrorSite {
            path: self.path.to_string(),
            line: node.span().start().line,
            kind,
            subject,
        });
    }

    /// Record one value site at the start of `value`, unless that value is
    /// already reported.
    fn record_value(&mut self, value: &impl Spanned, kind: StringTypedErrorKind, subject: String) {
        let start = value.span().start();
        if self.reported_values.insert((start.line, start.column)) {
            self.record(value, kind, subject);
        }
    }

    /// The search for formatted text at the current point of the walk.
    const fn formatted_text(&self) -> FormattedText<'_> {
        FormattedText {
            index: self.index,
            locals: &self.formatted_locals,
        }
    }

    /// Record the formatted text inside `value`, which `sink` receives.
    fn inspect_text_sink(&mut self, sink: &impl ToTokens, value: &Expr) {
        if let Some(formatted) = self.formatted_text().first_within(value) {
            let subject = format!("{} <- {}", render(sink), render(formatted));
            self.record_value(formatted, StringTypedErrorKind::FormattedErrorText, subject);
        }
    }

    /// Record a string, or formatted text, that `sink` turns into an error value.
    fn inspect_error_value(&mut self, sink: &impl ToTokens, value: &Expr) {
        if is_string_expression(value) {
            self.record_value(value, StringTypedErrorKind::StringErrorValue, render(sink));
        } else {
            self.inspect_text_sink(sink, value);
        }
    }

    /// Record every field of an error type, or of one of its variants, that holds a string.
    fn inspect_error_fields(&mut self, owner: &str, fields: &Fields) {
        for (position, field) in fields.iter().enumerate() {
            if is_test_only(&field.attrs) || !type_mentions_string(&field.ty) {
                continue;
            }
            let name = field.ident.as_ref().map_or_else(|| position.to_string(), Ident::to_string);
            self.record(&field.ty, StringTypedErrorKind::ErrorField, format!("{owner}.{name}: {}", render(&field.ty)));
        }
    }

    /// Record the formatted text a path call hands an error carrier or a
    /// text-carrying parameter.
    fn inspect_path_call(&mut self, call: &ExprCall, function: &Path) {
        let mut segments = function.segments.iter().rev();
        let Some(name) = segments.next() else {
            return;
        };
        let owner = segments.next();
        if owner.is_some_and(|owner| self.index.is_carrier(owner)) {
            for argument in &call.args {
                self.inspect_text_sink(function, argument);
            }
            return;
        }
        let owner_name = owner.map(|owner| if owner.ident == "Self" { self.impl_owner } else { Some(&owner.ident) });
        let owner_name = owner_name.flatten().map(Ident::to_string);
        for position in self.index.path_text_arguments(owner_name.as_deref(), &name.ident) {
            if let Some(argument) = call.args.iter().nth(position) {
                self.inspect_text_sink(function, argument);
            }
        }
    }

    /// Whether a path is a foreign error constructor reached through an `Error`
    /// type or an alias of one, such as `serde::de::Error::custom` or
    /// `DeserializeError::custom`.
    fn is_foreign_error_constructor(&self, path: &Path) -> bool {
        let mut segments = path.segments.iter().rev();
        let (Some(function), Some(owner)) = (segments.next(), segments.next()) else {
            return false;
        };
        let names_error = owner.ident == "Error" || self.error_aliases.contains(&owner.ident.to_string());
        names_error && FOREIGN_ERROR_CONSTRUCTORS.iter().any(|constructor| function.ident == constructor)
    }

    /// Walk one function body with its own set of formatted locals.
    fn within_function(&mut self, walk: impl FnOnce(&mut StringTypedErrorVisitor<'a>)) {
        let enclosing = mem::take(&mut self.formatted_locals);
        walk(self);
        self.formatted_locals = enclosing;
    }
}

impl<'a> Visit<'a> for StringTypedErrorVisitor<'a> {
    fn visit_item(&mut self, item: &'a Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_struct(&mut self, item: &'a ItemStruct) {
        if self.index.is_error_type(&item.ident) {
            self.inspect_error_fields(&item.ident.to_string(), &item.fields);
        }
        visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'a ItemEnum) {
        if self.index.is_error_type(&item.ident) {
            for variant in &item.variants {
                self.inspect_error_fields(&format!("{}::{}", item.ident, variant.ident), &variant.fields);
            }
        }
        visit_item_enum(self, item);
    }

    fn visit_item_impl(&mut self, item: &'a ItemImpl) {
        let owner = if let Type::Path(self_type) = item.self_ty.as_ref() {
            self_type.path.segments.last().map(|segment| &segment.ident)
        } else {
            None
        };
        let enclosing = mem::replace(&mut self.impl_owner, owner);
        visit_item_impl(self, item);
        self.impl_owner = enclosing;
    }

    fn visit_item_fn(&mut self, item: &'a ItemFn) {
        self.within_function(|visitor| visit_item_fn(visitor, item));
    }

    fn visit_impl_item_fn(&mut self, item: &'a ImplItemFn) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.within_function(|visitor| visit_impl_item_fn(visitor, item));
    }

    fn visit_trait_item_fn(&mut self, item: &'a TraitItemFn) {
        if is_test_only(&item.attrs) {
            return;
        }
        self.within_function(|visitor| visit_trait_item_fn(visitor, item));
    }

    fn visit_local(&mut self, local: &'a Local) {
        let formatted = local
            .init
            .as_ref()
            .is_some_and(|init| !matches!(init.expr.as_ref(), Expr::Closure(_)) && self.formatted_text().first_within(&init.expr).is_some());
        let mut bound = Vec::new();
        bound_idents(&local.pat, &mut bound);
        for ident in bound {
            if formatted {
                self.formatted_locals.insert(ident.to_string());
            } else {
                self.formatted_locals.remove(&ident.to_string());
            }
        }
        visit_local(self, local);
    }

    fn visit_type_path(&mut self, ty: &'a TypePath) {
        if let Some(error) = result_error_type(ty)
            && type_mentions_string(error)
        {
            self.record(ty, StringTypedErrorKind::StringErrorResult, render(ty));
        }
        visit_type_path(self, ty);
    }

    fn visit_expr_call(&mut self, call: &'a ExprCall) {
        if let Expr::Path(function) = call.func.as_ref() {
            if function.path.is_ident("Err") {
                if let Some(argument) = call.args.first() {
                    self.inspect_error_value(call, argument);
                }
            } else if self.is_foreign_error_constructor(&function.path) {
                for argument in &call.args {
                    self.inspect_error_value(call, argument);
                }
            } else {
                self.inspect_path_call(call, &function.path);
            }
        }
        visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'a ExprMethodCall) {
        if let Some(argument) = call.args.first() {
            let sink = || format!(".{}({})", call.method, render(&call.args));
            if call.method == "map_err" || call.method == "ok_or_else" {
                if yields_string(argument) {
                    self.record_value(&call.method, StringTypedErrorKind::StringErrorValue, sink());
                } else if let Expr::Closure(closure) = argument {
                    self.inspect_text_sink(&call.method, &closure.body);
                }
            } else if call.method == "ok_or" {
                self.inspect_error_value(&call.method, argument);
            }
        }
        let index = self.index;
        for &position in index.method_text_arguments(&call.method).into_iter().flatten() {
            if let Some(argument) = call.args.iter().nth(position) {
                self.inspect_text_sink(&call.method, argument);
            }
        }
        visit_expr_method_call(self, call);
    }

    fn visit_expr_struct(&mut self, literal: &'a ExprStruct) {
        let carrier = literal.path.segments.iter().any(|segment| self.index.is_carrier(segment));
        for field in &literal.fields {
            let text_member = matches!(&field.member, Member::Named(name) if TEXT_MEMBER_NAMES.iter().any(|text| name == text));
            if carrier || text_member {
                self.inspect_text_sink(&literal.path, &field.expr);
            }
        }
        visit_expr_struct(self, literal);
    }
}

/// Collects the names a source imports an `Error` under by renaming it.
struct ErrorAliasCollector {
    /// The alias names.
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for ErrorAliasCollector {
    fn visit_use_rename(&mut self, rename: &'ast UseRename) {
        if rename.ident == "Error" {
            self.names.insert(rename.rename.to_string());
        }
    }
}

/// The error type argument of a two-argument `Result<T, E>`.
fn result_error_type(ty: &TypePath) -> Option<&Type> {
    let segment = ty.path.segments.last()?;
    if segment.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    let mut types = arguments.args.iter().filter_map(|argument| {
        if let GenericArgument::Type(ty) = argument {
            return Some(ty);
        }
        None
    });
    types.next()?;
    types.next()
}

/// Whether a mapper argument yields a string: a closure whose body is a string
/// expression, or a path to a string conversion such as `ToString::to_string`.
fn yields_string(argument: &Expr) -> bool {
    if let Expr::Closure(closure) = argument {
        return is_string_expression(&closure.body);
    }
    if let Expr::Path(path) = argument {
        return is_string_conversion_path(&path.path);
    }
    false
}

/// The source text of a syntax node, for the report.
fn render(node: &impl ToTokens) -> String {
    node.to_token_stream().to_string()
}

#[cfg(test)]
mod tests {
    use crate::string_typed_error::{StringTypedErrorKind, visitor::StringTypedErrorVisitor, workspace_index::WorkspaceIndex};

    fn parse(source: &str) -> syn::File {
        syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"))
    }

    fn kinds_with_declarations(declarations: &str, source: &str) -> Vec<StringTypedErrorKind> {
        let declarations = parse(declarations);
        let syntax = parse(source);
        let index = WorkspaceIndex::collect([&declarations, &syntax]);
        StringTypedErrorVisitor::inspect("fixture.rs", &syntax, &index)
            .into_iter()
            .map(|site| site.kind)
            .collect()
    }

    fn kinds(source: &str) -> Vec<StringTypedErrorKind> {
        kinds_with_declarations("", source)
    }

    #[test]
    fn a_string_payload_in_a_derived_error_variant_is_flagged() {
        let source = "#[derive(Debug, thiserror::Error)] enum ParseError { #[error(\"bad {0}\")] Invalid(String) }";
        assert_eq!(kinds(source), vec![StringTypedErrorKind::ErrorField]);
    }

    #[test]
    fn a_named_message_field_in_a_derived_error_struct_is_flagged() {
        let source = "#[derive(Debug, Error)] #[error(\"{message}\")] struct StoreError { message: Box<str> }";
        assert_eq!(kinds(source), vec![StringTypedErrorKind::ErrorField]);
    }

    #[test]
    fn a_static_str_field_in_a_hand_implemented_error_is_flagged() {
        let source = "struct Rejection { reason: &'static str } impl std::error::Error for Rejection {}";
        assert_eq!(kinds(source), vec![StringTypedErrorKind::ErrorField]);
    }

    #[test]
    fn a_string_field_outside_an_error_type_is_not_flagged() {
        assert_eq!(kinds("#[derive(Debug, Clone)] struct Label { text: String }"), Vec::new());
    }

    #[test]
    fn typed_fields_and_sources_in_an_error_type_are_not_flagged() {
        let source = "#[derive(Debug, Error)] enum ParseError { #[error(\"bad count\")] Count { #[source] source: ParseIntError, id: EntityId } }";
        assert_eq!(kinds(source), Vec::new());
    }

    #[test]
    fn a_string_error_result_is_flagged_in_any_position() {
        assert_eq!(
            kinds("fn parse(text: &str) -> Result<u8, String> { todo!() }"),
            vec![StringTypedErrorKind::StringErrorResult]
        );
        assert_eq!(
            kinds("type Parsed = std::result::Result<u8, &'static str>;"),
            vec![StringTypedErrorKind::StringErrorResult]
        );
    }

    #[test]
    fn a_typed_error_result_and_a_string_success_value_are_not_flagged() {
        assert_eq!(kinds("fn parse() -> Result<String, ParseError> { todo!() }"), Vec::new());
        assert_eq!(kinds("fn parse() -> io::Result<String> { todo!() }"), Vec::new());
    }

    #[test]
    fn string_values_built_as_errors_are_flagged() {
        let source = r#"fn f() {
            let _ = Err(format!("bad {x}"));
            let _ = parse().map_err(|error| error.to_string());
            let _ = parse().map_err(ToString::to_string);
            let _ = lookup().ok_or("missing");
            let _ = lookup().ok_or_else(|| format!("missing {key}"));
            let _ = serde::de::Error::custom(format!("bad {x}"));
            let _ = io::Error::other("closed".to_owned());
        }"#;
        assert_eq!(kinds(source), vec![StringTypedErrorKind::StringErrorValue; 7]);
    }

    #[test]
    fn typed_values_built_as_errors_are_not_flagged() {
        let source = r#"fn f() {
            let _ = Err(ParseError::Empty);
            let _ = parse().map_err(|source| ParseError::Count { source });
            let _ = parse().map_err(ParseError::from);
            let _ = lookup().ok_or(LookupError::Missing);
            let _ = D::Error::custom(error);
            let _ = Ok::<_, ParseError>(format!("fine"));
        }"#;
        assert_eq!(kinds(source), Vec::new());
    }

    #[test]
    fn test_only_items_are_not_inspected() {
        let source = "#[cfg(test)] mod tests { fn parse() -> Result<u8, String> { Err(format!(\"x\")) } }";
        assert_eq!(kinds(source), Vec::new());
    }

    #[test]
    fn an_error_implemented_in_another_file_has_its_string_fields_flagged() {
        let declarations = "impl std::error::Error for Rejection {}";
        let source = "struct Rejection { reason: String }";
        assert_eq!(kinds_with_declarations(declarations, source), vec![StringTypedErrorKind::ErrorField]);
    }

    #[test]
    fn a_string_handed_to_an_aliased_foreign_error_constructor_is_flagged() {
        let source = r#"use serde::de::Error as DeserializeError;
        fn f() { let _ = DeserializeError::custom("bad"); }"#;
        assert_eq!(kinds(source), vec![StringTypedErrorKind::StringErrorValue]);
    }

    #[test]
    fn formatted_text_inside_an_error_value_is_flagged() {
        let source = r#"fn f() {
            let _ = Err(public_error(ErrorType::NotFound, format!("entity {id} not found"), &uri));
            let _ = parse().map_err(|error| rejection(format!("invalid: {error}"), uri));
            let _ = lookup().ok_or_else(|| problem(format!("missing {key}")));
            let _ = lookup().ok_or(problem(format!("missing {key}")));
        }"#;
        assert_eq!(kinds(source), vec![StringTypedErrorKind::FormattedErrorText; 4]);
    }

    #[test]
    fn formatted_text_handed_to_an_error_or_warning_constructor_is_flagged() {
        let source = r#"fn f() {
            let _ = NgsiLdWarning::RevalidationFailed(format!("source {id} answered garbage"));
            let _ = StoreError::new(format!("closed {pool}"));
        }"#;
        assert_eq!(kinds(source), vec![StringTypedErrorKind::FormattedErrorText; 2]);
    }

    #[test]
    fn formatted_text_in_a_text_field_or_a_carrier_literal_is_flagged() {
        let source = r#"fn f() {
            let _ = Disposition::Retry { detail: format!("{count} attempts failed") };
            let _ = QueryWarning { entity_id, note: format!("{entity_id} unreadable") };
        }"#;
        assert_eq!(kinds(source), vec![StringTypedErrorKind::FormattedErrorText; 2]);
    }

    #[test]
    fn formatted_text_handed_to_a_text_parameter_is_flagged() {
        let declarations = r"
            fn failure_detail(message: impl Into<String>) -> Details { todo!() }
            impl Report { fn public(kind: Kind, detail: impl Into<String>) -> Report { todo!() } }
            trait Queue { fn mark_dead(&self, id: Id, error: &str); }
        ";
        let source = r#"fn f() {
            let _ = failure_detail(format!("failed: {error}"));
            let _ = Report::public(Kind::Invalid, format!("bad {value}"));
            queue.mark_dead(id, &format!("{attempts} attempts"));
        }"#;
        assert_eq!(kinds_with_declarations(declarations, source), vec![StringTypedErrorKind::FormattedErrorText; 3]);
    }

    #[test]
    fn formatted_text_reaching_a_carrier_through_a_local_or_a_helper_is_flagged() {
        let source = r#"
            fn describe(status: u16) -> String { format!("answered HTTP {status}") }
            fn f() {
                let detail = match limit { Some(limit) => format!("over {limit}"), None => format!("over the limit") };
                let _ = Report { detail };
                let _ = NgsiLdWarning::Miscellaneous(describe(status));
            }
        "#;
        assert_eq!(kinds(source), vec![StringTypedErrorKind::FormattedErrorText; 2]);
    }

    #[test]
    fn displayed_typed_errors_and_literals_handed_to_a_carrier_are_not_flagged() {
        let declarations = "impl Report { fn public(kind: Kind, detail: impl Into<String>) -> Report { todo!() } }";
        let source = r#"fn f() {
            let detail = format!("over {limit}");
            let detail = Refusal::OverLimit { limit }.to_string();
            let _ = Report::public(Kind::Invalid, detail);
            let _ = Report::public(Kind::Invalid, "the body is empty");
            let _ = NgsiLdWarning::RevalidationFailed(refusal.to_string());
            let _ = Disposition::Retry { detail: failure.to_string() };
        }"#;
        assert_eq!(kinds_with_declarations(declarations, source), Vec::new());
    }

    #[test]
    fn formatted_text_outside_any_error_carrier_is_not_flagged() {
        let source = r#"fn f() {
            let location = format!("/ngsi-ld/v1/entities/{id}");
            let _ = with_location(response, location);
            let _ = Url::parse(&format!("http://{host}/"));
            lines.push(format!("{title} ({count})"));
        }"#;
        assert_eq!(kinds(source), Vec::new());
    }

    #[test]
    fn formatted_text_reached_through_two_sinks_is_reported_once() {
        let source = r#"use serde::de::Error as DeserializeError;
        fn f() { let _ = Err(DeserializeError::custom(format!("bad {value}"))); }"#;
        assert_eq!(kinds(source).len(), 1);
    }

    #[test]
    fn a_closure_bound_to_a_local_is_not_formatted_text() {
        let source = r#"use serde::de::Error as DeserializeError;
        fn f() { let out_of_range = || DeserializeError::custom(format!("{value} is out of range")); let _ = Err(out_of_range()); }"#;
        assert_eq!(kinds(source).len(), 1);
    }

    #[test]
    fn a_method_name_some_declaration_does_not_format_is_not_formatted_text() {
        let declarations = r#"
            impl Site { fn report(&self) -> String { format!("{}", self.path) } }
            impl Refusal { fn report(&self) -> Report { todo!() } }
        "#;
        let source = "fn f() { let _ = parse().map_err(|refusal| refusal.report()); }";
        assert_eq!(kinds_with_declarations(declarations, source), Vec::new());
    }

    #[test]
    fn a_method_every_declaration_of_which_formats_is_formatted_text() {
        let declarations = r#"impl Outcome { fn failure_detail(&self) -> Option<String> { Some(format!("{} failed", self.id)) } }"#;
        let source = "fn f() { let _ = NgsiLdWarning::MiscellaneousWarning(outcome.failure_detail().unwrap_or_default()); }";
        assert_eq!(kinds_with_declarations(declarations, source), vec![StringTypedErrorKind::FormattedErrorText]);
    }

    #[test]
    fn formatted_text_reaching_a_carrier_through_a_destructured_local_is_flagged() {
        let source = r#"fn f() {
            let (classification, detail) = match outcome { Outcome::Loop => (Kind::Loop, format!("{id} loops")), Outcome::Failed(error) => (Kind::Failed, error.to_string()) };
            let _ = BatchProblemDetail { kind: classification, detail };
        }"#;
        assert_eq!(kinds(source), vec![StringTypedErrorKind::FormattedErrorText]);
    }
}
