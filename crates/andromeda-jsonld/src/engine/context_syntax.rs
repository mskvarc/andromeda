//! A resolved `@context`, read as the context syntax the JSON-LD processor
//! processes.

use crate::{
    context_fault::FaultyContext,
    document_context::ResolvedContext,
    engine::{context_fault_reading::syntax_fault, processor_text::ProcessorTextError},
    error::JsonLdError,
};
use jsonld::syntax::{Context, TryFromJson};
use jstrict::parse::parse_str_value;

/// The context syntax `resolved` is.
///
/// A resolved context names every remote document by URL, so a syntax refusal
/// is in a context written in place, which has no name to report it under.
///
/// # Errors
///
/// Returns [`JsonLdError::ProcessorText`] when the processor's reader refuses
/// the resolved text, which the broker writes itself, and
/// [`JsonLdError::ContextUnprocessable`] when it is not a JSON-LD context.
pub(crate) fn context_syntax(resolved: &ResolvedContext) -> Result<Context, JsonLdError> {
    let value = parse_str_value(resolved.text()).map_err(ProcessorTextError::from)?;
    Context::try_from_json(&value).map_err(|invalid| JsonLdError::ContextUnprocessable {
        context: FaultyContext::Unnamed,
        fault: syntax_fault(&invalid),
    })
}

#[cfg(test)]
mod tests {
    use crate::{context_spec::ContextSpec, engine::context_syntax::context_syntax, error::JsonLdError};
    use andromeda_json_literal::JsonLiteral;

    #[test]
    fn a_resolved_context_is_read_entry_by_entry() {
        let syntax = context_syntax(&ContextSpec::none().resolved_context()).expect("the core context reference is a context");
        assert_eq!(syntax.len(), 1);
    }

    /// JSON-LD 1.1 section 9.15: a number is no context entry.
    #[test]
    fn an_entry_no_context_takes_is_invalid() {
        let spec = ContextSpec::from_body(JsonLiteral::admit("[3]").expect("admissible"));
        assert!(matches!(
            context_syntax(&spec.resolved_context()),
            Err(JsonLdError::ContextUnprocessable { .. })
        ));
    }
}
