//! A [`CompoundContextHost`] that stores nothing, for tests that render or
//! subscribe without a tenant's hosted `@context`s.

use crate::compound_context_host::{CompoundContextHost, CompoundContextHostFuture, ContextHostingError, WrapperUse};
use andromeda_json_literal::JsonLiteral;

/// Asked to host, it reports that no host is attached rather than naming a
/// context it never stored.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoCompoundContextHost;

impl CompoundContextHost for NoCompoundContextHost {
    fn host<'a>(&'a self, _context: &'a JsonLiteral, _usage: WrapperUse) -> CompoundContextHostFuture<'a> {
        Box::pin(async { Err(ContextHostingError::NoHost) })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compound_context_host::{CompoundContextHost, ContextHostingError, WrapperUse},
        no_compound_context_host::NoCompoundContextHost,
        test_documents::literal,
    };
    use serde_json::json;

    #[tokio::test]
    async fn a_host_that_stores_nothing_reports_that_no_host_is_attached() {
        let context = literal(&json!(["https://example.com/a.jsonld", "https://example.com/b.jsonld"]));

        assert!(matches!(
            NoCompoundContextHost.host(&context, WrapperUse::Response).await,
            Err(ContextHostingError::NoHost)
        ));
    }
}
