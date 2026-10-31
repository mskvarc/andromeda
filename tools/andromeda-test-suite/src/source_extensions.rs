//! The extension tokens a broker lists under `extensions` in the `contextSourceExtras` of its Context Source
//! Identity (Table 5.2.40-1 leaves the content of the extras to the implementation).

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::ContextSourceIdentity;
use serde_json::Value;

/// Why an identity does not hold a list of extension tokens.
#[derive(Debug, thiserror::Error)]
pub enum SourceExtensionsError {
    /// The identity carries no `contextSourceExtras`.
    #[error("the identity has no contextSourceExtras")]
    NoExtras,
    /// The extras are not JSON.
    #[error("the contextSourceExtras are not JSON")]
    NotJson(#[source] serde_json::Error),
    /// The extras hold no `extensions` array of strings.
    #[error("the contextSourceExtras hold no extensions array of strings")]
    NoExtensions,
}

/// The tokens `identity` lists under `contextSourceExtras.extensions`, in the order listed.
///
/// # Errors
///
/// Returns [`SourceExtensionsError`] when the identity has no extras or they hold no array of strings under
/// `extensions`.
pub fn extensions(identity: &ContextSourceIdentity) -> Result<Vec<String>, SourceExtensionsError> {
    let extras = identity.extras.as_ref().ok_or(SourceExtensionsError::NoExtras)?;
    let document: Value = serde_json::from_str(&extras.to_string()).map_err(SourceExtensionsError::NotJson)?;
    let Some(Value::Array(items)) = document.get("extensions") else {
        return Err(SourceExtensionsError::NoExtensions);
    };
    items
        .iter()
        .map(|item| item.as_str().map(str::to_owned).ok_or(SourceExtensionsError::NoExtensions))
        .collect()
}

/// Matches a list of extension tokens that holds `token`.
#[derive(Debug, Clone, MatcherBase)]
pub struct ListsExtension {
    /// The token required.
    token: String,
}

/// A list of extension tokens that holds `token`.
#[must_use]
pub fn lists_extension(token: &str) -> ListsExtension {
    ListsExtension { token: token.to_owned() }
}

impl Matcher<&Vec<String>> for ListsExtension {
    fn matches(&self, actual: &Vec<String>) -> MatcherResult {
        if actual.contains(&self.token) {
            MatcherResult::Match
        } else {
            MatcherResult::NoMatch
        }
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("lists the extension {}", self.token).into(),
            MatcherResult::NoMatch => format!("does not list the extension {}", self.token).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::source_extensions::{SourceExtensionsError, extensions, lists_extension};
    use googletest::prelude::*;
    use ngsi_ld::model::ContextSourceIdentity;

    fn identity(text: &str) -> Result<ContextSourceIdentity> {
        Ok(ContextSourceIdentity::parse(text)?)
    }

    #[gtest]
    fn the_tokens_under_extensions_are_listed_in_order() -> Result<()> {
        let identity = identity(
            r#"{"id":"urn:ngsi-ld:ContextSource:x","type":"ContextSourceIdentity","contextSourceAlias":"a","contextSourceUptime":"PT1S","contextSourceTimeAt":"2026-01-01T00:00:00Z","contextSourceExtras":{"extensions":["one","two"]}}"#,
        )?;
        let listed = extensions(&identity)?;
        expect_that!(listed, elements_are![eq("one"), eq("two")]);
        expect_that!(&listed, lists_extension("two"));
        expect_that!(&listed, not(lists_extension("three")));
        Ok(())
    }

    #[gtest]
    fn an_identity_without_extras_lists_no_extensions() -> Result<()> {
        let identity = identity(
            r#"{"id":"urn:ngsi-ld:ContextSource:x","type":"ContextSourceIdentity","contextSourceAlias":"a","contextSourceUptime":"PT1S","contextSourceTimeAt":"2026-01-01T00:00:00Z"}"#,
        )?;
        expect_that!(
            extensions(&identity),
            err(predicate(|error: &SourceExtensionsError| matches!(error, SourceExtensionsError::NoExtras)))
        );
        Ok(())
    }
}
