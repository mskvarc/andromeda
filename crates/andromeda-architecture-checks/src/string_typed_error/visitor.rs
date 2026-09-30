//! The syntax-tree walk that finds string-typed error sites in one source.

use crate::{
    source_scan::{is_test_only, item_is_test_only},
    string_typed_error::{
        StringTypedErrorKind,
        StringTypedErrorSite,
        string_shape::{is_string_conversion_path, is_string_expression, type_mentions_string},
    },
};
use quote::ToTokens;
use std::collections::BTreeSet;
use syn::{
    Attribute,
    Expr,
    ExprCall,
    ExprMethodCall,
    Fields,
    File,
    GenericArgument,
    Ident,
    Item,
    ItemEnum,
    ItemImpl,
    ItemStruct,
    Path,
    PathArguments,
    Token,
    Type,
    TypePath,
    punctuated::Punctuated,
    spanned::Spanned,
    visit::{Visit, visit_expr_call, visit_expr_method_call, visit_item, visit_item_enum, visit_item_struct, visit_type_path},
};

/// Foreign error constructors that accept a message, named by their last path
/// segment and reached through an `Error` type: `serde::de::Error::custom`,
/// `io::Error::other`, `io::Error::new`.
const FOREIGN_ERROR_CONSTRUCTORS: &[&str] = &["custom", "new", "other"];

/// Collects the string-typed error sites of one parsed source.
pub(crate) struct StringTypedErrorVisitor<'a> {
    /// Workspace-relative path of the source under inspection.
    path: &'a str,
    /// Names of the types the source implements `Error` for by hand.
    implemented_errors: BTreeSet<String>,
    /// Sites found so far.
    sites: Vec<StringTypedErrorSite>,
}

impl<'a> StringTypedErrorVisitor<'a> {
    /// Every string-typed error site in one parsed source.
    pub(crate) fn inspect(path: &'a str, syntax: &File) -> Vec<StringTypedErrorSite> {
        let mut collector = ErrorImplCollector { names: BTreeSet::new() };
        collector.visit_file(syntax);
        let mut visitor = StringTypedErrorVisitor {
            path,
            implemented_errors: collector.names,
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

    /// Whether a type is an error: it derives `Error` or this file implements it.
    fn is_error_type(&self, attributes: &[Attribute], ident: &Ident) -> bool {
        derives_error(attributes) || self.implemented_errors.contains(&ident.to_string())
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
}

impl<'ast> Visit<'ast> for StringTypedErrorVisitor<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_is_test_only(item) {
            return;
        }
        visit_item(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
        if self.is_error_type(&item.attrs, &item.ident) {
            self.inspect_error_fields(&item.ident.to_string(), &item.fields);
        }
        visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast ItemEnum) {
        if self.is_error_type(&item.attrs, &item.ident) {
            for variant in &item.variants {
                self.inspect_error_fields(&format!("{}::{}", item.ident, variant.ident), &variant.fields);
            }
        }
        visit_item_enum(self, item);
    }

    fn visit_type_path(&mut self, ty: &'ast TypePath) {
        if let Some(error) = result_error_type(ty)
            && type_mentions_string(error)
        {
            self.record(ty, StringTypedErrorKind::StringErrorResult, render(ty));
        }
        visit_type_path(self, ty);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(function) = call.func.as_ref() {
            let makes_string_error = if function.path.is_ident("Err") {
                call.args.first().is_some_and(is_string_expression)
            } else {
                is_foreign_error_constructor(&function.path) && call.args.iter().any(is_string_expression)
            };
            if makes_string_error {
                self.record(call, StringTypedErrorKind::StringErrorValue, render(call));
            }
        }
        visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        let makes_string_error = call.args.first().is_some_and(|argument| {
            if call.method == "map_err" || call.method == "ok_or_else" {
                return yields_string(argument);
            }
            call.method == "ok_or" && is_string_expression(argument)
        });
        if makes_string_error {
            self.record(
                &call.method,
                StringTypedErrorKind::StringErrorValue,
                format!(".{}({})", call.method, render(&call.args)),
            );
        }
        visit_expr_method_call(self, call);
    }
}

/// Collects the names of the types a source implements `Error` for by hand.
struct ErrorImplCollector {
    /// The implementing type names.
    names: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for ErrorImplCollector {
    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        let implements_error = item
            .trait_
            .as_ref()
            .is_some_and(|(trait_path, _)| trait_path.segments.last().is_some_and(|segment| segment.ident == "Error"));
        if implements_error && let Type::Path(self_type) = item.self_ty.as_ref() {
            self.names.extend(self_type.path.segments.last().map(|segment| segment.ident.to_string()));
        }
    }
}

/// Whether the attributes derive `Error`, under any path spelling
/// (`thiserror::Error`, a `use`-imported `Error`).
fn derives_error(attributes: &[Attribute]) -> bool {
    attributes.iter().filter(|attribute| attribute.path().is_ident("derive")).any(|attribute| {
        attribute
            .parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)
            .is_ok_and(|derived| derived.iter().any(|path| path.segments.last().is_some_and(|segment| segment.ident == "Error")))
    })
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

/// Whether a path is a foreign error constructor reached through an `Error`
/// type, such as `serde::de::Error::custom` or `io::Error::other`.
fn is_foreign_error_constructor(path: &Path) -> bool {
    let mut segments = path.segments.iter().rev();
    let (Some(function), Some(owner)) = (segments.next(), segments.next()) else {
        return false;
    };
    owner.ident == "Error" && FOREIGN_ERROR_CONSTRUCTORS.iter().any(|constructor| function.ident == constructor)
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
    use crate::string_typed_error::{StringTypedErrorKind, visitor::StringTypedErrorVisitor};

    fn kinds(source: &str) -> Vec<StringTypedErrorKind> {
        let syntax = syn::parse_file(source).unwrap_or_else(|error| panic!("fixture parses: {error}"));
        StringTypedErrorVisitor::inspect("fixture.rs", &syntax)
            .into_iter()
            .map(|site| site.kind)
            .collect()
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
}
