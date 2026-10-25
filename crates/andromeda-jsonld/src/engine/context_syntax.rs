//! A resolved `@context`, read as the context syntax the JSON-LD processor
//! processes.

use crate::{document_context::ResolvedContext, engine::processor_text::ProcessorTextError, error::JsonLdError};
use jsonld::syntax::{Context, TryFromJson};
use jstrict::parse::parse_str_value;

/// The context syntax `resolved` is.
///
/// # Errors
///
/// Returns [`JsonLdError::ProcessorText`] when the processor's reader refuses
/// the resolved text, which the broker writes itself, and
/// [`JsonLdError::InlineContextInvalid`] when it is not a JSON-LD context.
pub(crate) fn context_syntax(resolved: &ResolvedContext) -> Result<Context, JsonLdError> {
    let value = parse_str_value(resolved.text()).map_err(ProcessorTextError::from)?;
    Context::try_from_json(&value).map_err(JsonLdError::InlineContextInvalid)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
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
        assert!(matches!(context_syntax(&spec.resolved_context()), Err(JsonLdError::InlineContextInvalid(_))));
    }
}
