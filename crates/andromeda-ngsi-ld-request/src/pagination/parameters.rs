//! The `limit`, `offset` and `cursor` parameters every paged collection reads
//! (clauses 5.5.9 and 6.3.10).

use crate::pagination::{
    cursor_request::{CursorRequest, CursorRequestError},
    page_context::{PageContext, PageContextError},
};
use serde::{Deserialize, Deserializer, de::Error as DeserializeError};
use strum::Display;
use thiserror::Error;

/// The pagination parameters of a request whose pointers are dereferenceable:
/// a query-string binding (clause 6.3.10).
///
/// Parsing enforces no rule between them -- `cursor` beside `offset`, a cursor
/// on a collection that cannot be walked by one, `limit=0` without a count are
/// all decided by [`crate::pagination::policy::PaginationPolicy::window`], so
/// every collection refuses them alike.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageParameters {
    /// The page size asked for (clause 6.3.10 `limit`).
    pub limit: Option<u32>,
    /// How many elements to skip before the page (clause 5.5.9.2 `offset`).
    pub offset: Option<u32>,
    /// The opaque pointer a followed `Link` carried, or the empty value that
    /// starts a cursor walk (clause 5.5.9.1).
    pub cursor: Option<CursorRequest>,
    /// The `@context` the walk a followed pointer belongs to is read under,
    /// absent on a request that follows no pointer (clause 5.5.9.1).
    pub context: Option<PageContext>,
}

impl PageParameters {
    /// Whether the request named none of the pagination parameters.
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        self.limit.is_none() && self.offset.is_none() && self.cursor.is_none() && self.context.is_none()
    }
}

/// The pagination parameters of a binding that carries its query in the
/// request body (clauses 6.23.3.1 and 6.24.3.1).
///
/// Such a binding has no dereferenceable pointer to carry a cursor in, so it
/// declares none: a `cursor` there is a parameter the operation does not
/// define, an `InvalidRequest` (clause 6.3.20).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OffsetPageParameters {
    /// The page size asked for (clause 6.3.10 `limit`).
    pub limit: Option<u32>,
    /// How many elements to skip before the page (clause 5.5.9.2 `offset`).
    pub offset: Option<u32>,
}

impl From<OffsetPageParameters> for PageParameters {
    fn from(page: OffsetPageParameters) -> PageParameters {
        PageParameters {
            limit: page.limit,
            offset: page.offset,
            cursor: None,
            context: None,
        }
    }
}

/// One numeric pagination parameter, named as the request spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[strum(serialize_all = "camelCase")]
pub enum PageParameter {
    /// The `limit` parameter.
    Limit,
    /// The `offset` parameter.
    Offset,
}

/// A pagination parameter whose value names nothing.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PageParameterError {
    /// A `limit` or `offset` that is not a non-negative integer.
    #[error("invalid {parameter} value: not a non-negative integer")]
    NotANumber {
        /// The parameter at fault.
        parameter: PageParameter,
    },
    /// A `cursor` that is not a token the broker emitted.
    #[error(transparent)]
    Cursor(#[from] CursorRequestError),
    /// A `pageContext` that is not a context the broker emitted.
    #[error(transparent)]
    Context(#[from] PageContextError),
}

/// The wire form of [`PageParameters`].
#[derive(Deserialize)]
struct PageParametersWire {
    /// The raw `limit`.
    limit: Option<String>,
    /// The raw `offset`.
    offset: Option<String>,
    /// The raw `cursor`.
    cursor: Option<String>,
    /// The raw `pageContext`.
    #[serde(rename = "pageContext")]
    context: Option<String>,
}

/// The wire form of [`OffsetPageParameters`].
#[derive(Deserialize)]
struct OffsetPageParametersWire {
    /// The raw `limit`.
    limit: Option<String>,
    /// The raw `offset`.
    offset: Option<String>,
}

// Hand-written because the parameters are read inside a parameter set that
// flattens them, where serde hands every value over as text: a derived
// `Option<u32>` would refuse the text a query string spells a number with.
impl<'de> Deserialize<'de> for PageParameters {
    fn deserialize<D>(deserializer: D) -> Result<PageParameters, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PageParametersWire::deserialize(deserializer)?;
        Ok(PageParameters {
            limit: page_number(wire.limit.as_deref(), PageParameter::Limit).map_err(DeserializeError::custom)?,
            offset: page_number(wire.offset.as_deref(), PageParameter::Offset).map_err(DeserializeError::custom)?,
            cursor: wire
                .cursor
                .as_deref()
                .map(CursorRequest::try_from)
                .transpose()
                .map_err(|error| DeserializeError::custom(PageParameterError::from(error)))?,
            context: wire
                .context
                .as_deref()
                .map(PageContext::try_from)
                .transpose()
                .map_err(|error| DeserializeError::custom(PageParameterError::from(error)))?,
        })
    }
}

// Hand-written for the same reason as `PageParameters`.
impl<'de> Deserialize<'de> for OffsetPageParameters {
    fn deserialize<D>(deserializer: D) -> Result<OffsetPageParameters, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = OffsetPageParametersWire::deserialize(deserializer)?;
        Ok(OffsetPageParameters {
            limit: page_number(wire.limit.as_deref(), PageParameter::Limit).map_err(DeserializeError::custom)?,
            offset: page_number(wire.offset.as_deref(), PageParameter::Offset).map_err(DeserializeError::custom)?,
        })
    }
}

/// Read one numeric pagination parameter from the text the request spells it
/// with.
fn page_number(raw: Option<&str>, parameter: PageParameter) -> Result<Option<u32>, PageParameterError> {
    raw.map(|text| text.parse().map_err(|_| PageParameterError::NotANumber { parameter }))
        .transpose()
}

#[cfg(test)]
mod tests {
    use crate::pagination::{
        cursor_request::CursorRequest,
        parameters::{OffsetPageParameters, PageParameters},
    };
    use andromeda_ngsi_ld_query::cursor::PageCursor;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;

    /// A parameter set that flattens the page, as every paged collection does.
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Carrier {
        #[serde(flatten)]
        page: PageParameters,
    }

    /// The body-binding counterpart of [`Carrier`].
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct OffsetCarrier {
        #[serde(flatten)]
        page: OffsetPageParameters,
    }

    fn parsed(query: &str) -> PageParameters {
        serde_urlencoded::from_str::<Carrier>(query).expect("parses").page
    }

    fn rejection(query: &str) -> String {
        serde_urlencoded::from_str::<Carrier>(query).expect_err("rejected").to_string()
    }

    /// Under `flatten` the numbers arrive as text and are read from it.
    #[test]
    fn numbers_are_read_from_text_under_flatten() {
        assert_eq!(
            parsed("limit=10&offset=5"),
            PageParameters {
                limit: Some(10),
                offset: Some(5),
                cursor: None,
                context: None,
            }
        );
    }

    /// A malformed number is bad data, not an undefined parameter.
    #[test]
    fn a_non_numeric_limit_is_refused() {
        let message = rejection("limit=ten");
        assert!(!message.contains("unknown field"), "{message}");
        assert!(message.contains("limit"), "{message}");
    }

    /// The flattened page claims only its own parameters, so a parameter the
    /// operation does not define is still reported as unknown (clause 6.3.20).
    #[test]
    fn an_unknown_sibling_parameter_is_still_unknown() {
        assert!(rejection("limit=1&bogus=1").contains("unknown field"));
    }

    /// An empty `cursor=` is the start of a walk.
    #[test]
    fn an_empty_cursor_parameter_is_a_walk_start() {
        assert_eq!(parsed("cursor=").cursor, Some(CursorRequest::Start));
    }

    /// Parsing judges no combination: the policy does.
    #[test]
    fn cursor_and_offset_are_parsed_without_judgement() {
        let token = PageCursor::End.encode();
        let page = parsed(&format!("cursor={token}&offset=1"));
        assert_eq!(page.cursor, Some(CursorRequest::Continue(PageCursor::End)));
        assert_eq!(page.offset, Some(1));
    }

    /// A malformed cursor is bad data.
    #[test]
    fn a_malformed_cursor_is_refused_as_bad_data() {
        let message = rejection("cursor=%25%25%25");
        assert!(!message.contains("unknown field"), "{message}");
        assert!(message.contains("cursor"), "{message}");
    }

    /// A body binding declares no `cursor`, so one is an undefined parameter.
    #[test]
    fn the_post_binding_refuses_a_cursor() {
        let message = serde_urlencoded::from_str::<OffsetCarrier>("cursor=x").expect_err("rejected").to_string();
        assert!(message.contains("unknown field"), "{message}");
    }

    /// The body binding reads its numbers as the query-string binding does.
    #[test]
    fn the_post_binding_reads_limit_and_offset() {
        let page = serde_urlencoded::from_str::<OffsetCarrier>("limit=3&offset=6").expect("parses").page;
        assert_eq!(
            PageParameters::from(page),
            PageParameters {
                limit: Some(3),
                offset: Some(6),
                cursor: None,
                context: None,
            }
        );
    }

    /// A followed pointer's `pageContext` is read as the context it spells.
    #[test]
    fn a_page_context_is_read_from_the_pointer() {
        let page = parsed("limit=1&offset=1&pageContext=https%3A%2F%2Fexample.com%2Fa.jsonld");
        assert_eq!(
            page.context.map(|context| context.as_str().to_owned()),
            Some("https://example.com/a.jsonld".to_owned())
        );
    }

    /// A `pageContext` that is not an IRI is bad data, not an unknown parameter.
    #[test]
    fn a_malformed_page_context_is_refused_as_bad_data() {
        let message = rejection("pageContext=not%20an%20iri");
        assert!(!message.contains("unknown field"), "{message}");
        assert!(message.contains("pageContext"), "{message}");
    }

    /// A request naming none of the parameters is absent of pagination.
    #[test]
    fn absent_parameters_are_absent() {
        assert!(parsed("").is_absent());
        assert!(!parsed("limit=1").is_absent());
        assert!(!parsed("cursor=").is_absent());
    }
}
