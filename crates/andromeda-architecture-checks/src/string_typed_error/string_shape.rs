//! What counts as a string in type and expression position.
//!
//! `syn::Expr`, `syn::Stmt` and `syn::Type` are `#[non_exhaustive]`, so the few shapes that
//! hold or produce a string are asked for directly with `if let` rather than matched
//! against a catch-all arm.

use syn::{Expr, ExprLit, GenericArgument, Lit, Path, PathArguments, PathSegment, Stmt, Type};

/// Methods that yield a string when their receiver already is one.
const STRING_PRESERVING_METHODS: &[&str] = &["into", "into_boxed_str", "to_owned", "to_lowercase", "to_uppercase"];

/// Standard containers a string can hide inside while still being the value an
/// error carries.
///
/// A string argument of any other generic type is that type's business: a
/// foreign typed parse error such as `InvalidLangTag<String>` or
/// `InvalidIri<String>` is exactly the typed source an error should chain, and
/// the string it holds is the input it rejected.
const STRING_CONTAINERS: &[&str] = &[
    "Arc", "BTreeMap", "BTreeSet", "Box", "Cow", "HashMap", "HashSet", "IndexMap", "IndexSet", "Option", "Rc", "Vec", "VecDeque",
];

/// Whether a type is a `String` or a `str`, directly, behind a reference, or
/// inside a standard container: `String`, `&'static str`, `Box<str>`,
/// `Cow<'_, str>`, `Option<String>`, `Vec<String>`, `(String, u8)`, and so on.
pub(crate) fn type_mentions_string(ty: &Type) -> bool {
    if let Type::Path(path) = ty {
        return path.path.segments.last().is_some_and(segment_holds_string);
    }
    if let Type::Reference(reference) = ty {
        return type_mentions_string(&reference.elem);
    }
    if let Type::Tuple(tuple) = ty {
        return tuple.elems.iter().any(type_mentions_string);
    }
    if let Type::Slice(slice) = ty {
        return type_mentions_string(&slice.elem);
    }
    if let Type::Array(array) = ty {
        return type_mentions_string(&array.elem);
    }
    if let Type::Paren(inner) = ty {
        return type_mentions_string(&inner.elem);
    }
    if let Type::Group(inner) = ty {
        return type_mentions_string(&inner.elem);
    }
    false
}

/// Whether a type's last path segment is a string, or a standard container with
/// a string among its type arguments.
fn segment_holds_string(segment: &PathSegment) -> bool {
    if segment.ident == "String" || segment.ident == "str" {
        return true;
    }
    if !STRING_CONTAINERS.iter().any(|container| segment.ident == container) {
        return false;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    arguments.args.iter().any(|argument| {
        if let GenericArgument::Type(ty) = argument {
            return type_mentions_string(ty);
        }
        false
    })
}

/// Whether an expression evaluates to a string built for an error: a `format!`,
/// a string literal, a `to_string()`, a `String::from`, or a string passed
/// through a string-preserving conversion.
pub(crate) fn is_string_expression(expr: &Expr) -> bool {
    if let Expr::Macro(macro_expr) = expr {
        return macro_expr.mac.path.segments.last().is_some_and(|segment| segment.ident == "format");
    }
    if let Expr::Lit(ExprLit { lit: Lit::Str(_), .. }) = expr {
        return true;
    }
    if let Expr::MethodCall(call) = expr {
        return call.method == "to_string" || (STRING_PRESERVING_METHODS.iter().any(|method| call.method == method) && is_string_expression(&call.receiver));
    }
    if let Expr::Call(call) = expr {
        return matches!(call.func.as_ref(), Expr::Path(path) if is_string_constructor(&path.path));
    }
    if let Expr::Paren(inner) = expr {
        return is_string_expression(&inner.expr);
    }
    if let Expr::Group(inner) = expr {
        return is_string_expression(&inner.expr);
    }
    if let Expr::Reference(inner) = expr {
        return is_string_expression(&inner.expr);
    }
    if let Expr::Block(block) = expr {
        return block.block.stmts.last().is_some_and(|statement| {
            if let Stmt::Expr(tail, None) = statement {
                return is_string_expression(tail);
            }
            false
        });
    }
    false
}

/// Whether a function path converts its argument into a string when used as a
/// `map_err` mapper: `ToString::to_string`, `String::from`.
pub(crate) fn is_string_conversion_path(path: &Path) -> bool {
    path.segments.last().is_some_and(|segment| segment.ident == "to_string") || is_string_constructor(path)
}

/// Whether a path names a `String` constructor: `String::from`, `String::new`.
fn is_string_constructor(path: &Path) -> bool {
    let mut segments = path.segments.iter().rev();
    let (Some(function), Some(owner)) = (segments.next(), segments.next()) else {
        return false;
    };
    owner.ident == "String" && (function.ident == "from" || function.ident == "new")
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::string_typed_error::string_shape::{is_string_expression, type_mentions_string};

    fn string_type(source: &str) -> bool {
        let ty = syn::parse_str(source).unwrap_or_else(|error| panic!("fixture type parses: {error}"));
        type_mentions_string(&ty)
    }

    fn string_expression(source: &str) -> bool {
        let expr = syn::parse_str(source).unwrap_or_else(|error| panic!("fixture expression parses: {error}"));
        is_string_expression(&expr)
    }

    #[test]
    fn owned_borrowed_and_boxed_strings_are_string_types() {
        assert!(string_type("String"));
        assert!(string_type("std::string::String"));
        assert!(string_type("&'static str"));
        assert!(string_type("Box<str>"));
        assert!(string_type("Cow<'static, str>"));
    }

    #[test]
    fn containers_of_strings_are_string_types() {
        assert!(string_type("Option<String>"));
        assert!(string_type("Vec<String>"));
        assert!(string_type("HashMap<String, u8>"));
        assert!(string_type("(String, u8)"));
    }

    #[test]
    fn typed_values_are_not_string_types() {
        assert!(!string_type("ParseIntError"));
        assert!(!string_type("Option<EntityId>"));
        assert!(!string_type("Box<ParseError>"));
        assert!(!string_type("langtag::InvalidLangTag<String>"));
        assert!(!string_type("InvalidIri<String>"));
    }

    #[test]
    fn formatted_literal_and_stringified_values_are_string_expressions() {
        assert!(string_expression("format!(\"bad {value}\")"));
        assert!(string_expression("\"missing\""));
        assert!(string_expression("error.to_string()"));
        assert!(string_expression("String::from(text)"));
        assert!(string_expression("\"missing\".to_owned()"));
        assert!(string_expression("format!(\"bad\").into()"));
        assert!(string_expression("{ let x = 1; format!(\"{x}\") }"));
    }

    #[test]
    fn typed_values_and_conversions_of_them_are_not_string_expressions() {
        assert!(!string_expression("ParseError::Empty"));
        assert!(!string_expression("error.into()"));
        assert!(!string_expression("error.to_owned()"));
        assert!(!string_expression("ParseError::Invalid { source }"));
    }
}
