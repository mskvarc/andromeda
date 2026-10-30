//! The URL parameters the POST binding of "Query Temporal Evolution of
//! Entities" carries beside its clause 5.2.23 body.
//!
//! Table 5.2.23-1 gives the `Query` data type no pagination member, no
//! representation member and no local-scope flag, while Table 6.2-1 lists
//! `POST /temporal/entityOperations/query` among the HTTP operations of clause
//! 5.7.4. Clause 6.3.10 therefore requires `limit` (and, for transparent
//! pagination, `offset`) on this binding, clause 6.3.13 requires `count`, and
//! clauses 6.3.11 and 6.3.12 name the operation explicitly when they require
//! `options` and `format`. Those are exactly what the URL carries here;
//! everything else the query says travels in the body.

use crate::{
    options::TemporalRepresentationOptions,
    pagination::parameters::OffsetPageParameters,
    query::temporal::TemporalQueryParameters,
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
    representation_parameter::WrittenValue,
};
use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
use serde::Deserialize;

/// The query string of `POST /temporal/entityOperations/query`.
///
/// Unknown parameters are refused rather than ignored: clause 6.3.20 answers a
/// parameter the operation cannot use with `InvalidRequest`, and the query
/// itself belongs in the body on this binding.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalPostQueryParams {
    /// The page asked for (clause 6.3.10 `limit` and `offset`). The binding
    /// carries no dereferenceable pointer, so it declares no `cursor`.
    #[serde(flatten)]
    pub page: OffsetPageParameters,
    /// Whether the response reports the total number of matches
    /// (clause 6.3.13 `count`).
    pub count: Option<bool>,
    /// The temporal representation the response uses (clause 6.3.12 `format`).
    pub format: Option<WrittenValue<TemporalFormat>>,
    /// The deprecated representation selector and the system-attribute
    /// selector (clauses 6.3.11 and 6.3.12 `options`).
    pub options: Option<WrittenValue<TemporalRepresentationOptions>>,
    /// Whether the operation is confined to this broker (clause 5.5.13), which
    /// clause 5.7.4.4 admits as an entity selection criterion of its own.
    pub local: Option<bool>,
}

query_parameter_set!(TemporalPostQueryParams { Count, Format, Options, Local } + OffsetPageParameters);

impl TemporalPostQueryParams {
    /// Put the URL's half of the query onto the parameters the clause 5.2.23
    /// body produced.
    ///
    /// Nothing is overwritten: Table 5.2.23-1 defines none of these members, so
    /// the body conversion leaves every one of them absent and the URL is the
    /// only place they can come from.
    pub fn apply_to(self, params: &mut TemporalQueryParameters) {
        let TemporalPostQueryParams {
            page,
            count,
            format,
            options,
            local,
        } = self;
        params.page = page.into();
        params.count = count;
        params.format = format;
        params.options = options;
        params.local = local;
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        options::TemporalRepresentationOptions,
        pagination::parameters::PageParameters,
        query::temporal::{TemporalQueryParameters, post_parameters::TemporalPostQueryParams},
        representation_parameter::WrittenValue,
    };
    use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;

    /// Clause 6.3.10 with clause 6.3.13: the POST binding reads its page and
    /// its count off the URL, because Table 5.2.23-1 defines neither member.
    #[test]
    fn the_post_binding_reads_its_page_and_count_from_the_url() {
        let params = serde_urlencoded::from_str::<TemporalPostQueryParams>("limit=5&offset=10&count=true").expect("query parses");
        pretty_assertions::assert_eq!(params.page.limit, Some(5));
        pretty_assertions::assert_eq!(params.page.offset, Some(10));
        pretty_assertions::assert_eq!(params.count, Some(true));
    }

    /// Clauses 6.3.11 and 6.3.12 name "Query Temporal Evolutions of Entities"
    /// among the POST operations that shall support `format` and `options`.
    #[test]
    fn the_post_binding_reads_its_representation_from_the_url() {
        let params = serde_urlencoded::from_str::<TemporalPostQueryParams>("format=temporalValues&options=sysAttrs").expect("query parses");
        pretty_assertions::assert_eq!(params.format, Some(WrittenValue::Supported(TemporalFormat::TemporalValues)));
        pretty_assertions::assert_eq!(
            params.options,
            Some(WrittenValue::Supported(TemporalRepresentationOptions {
                format: None,
                sys_attrs: SysAttrsInclusion::Include,
            }))
        );
    }

    /// Clause 6.3.20: a parameter the operation cannot use is an invalid
    /// request, so the binding refuses it instead of dropping it silently.
    #[test]
    fn a_parameter_the_post_binding_does_not_define_is_refused() {
        assert!(serde_urlencoded::from_str::<TemporalPostQueryParams>("timerel=after").is_err());
    }

    /// The URL fills exactly the members Table 5.2.23-1 leaves the body unable
    /// to express, and the page it names is the one clause 5.5.9 cuts.
    #[test]
    fn applying_the_url_parameters_fills_the_members_the_body_cannot_carry() {
        let url = serde_urlencoded::from_str::<TemporalPostQueryParams>("limit=3&offset=6&count=true&local=true").expect("query parses");
        let mut params = TemporalQueryParameters::default();

        url.apply_to(&mut params);

        pretty_assertions::assert_eq!(
            params.page,
            PageParameters {
                limit: Some(3),
                offset: Some(6),
                cursor: None,
                context: None,
                query: None,
            }
        );
        pretty_assertions::assert_eq!(params.count, Some(true));
        assert!(params.is_local());
    }

    /// An empty query string leaves the body's query untouched, so the shared
    /// clause 5.5.9 defaults decide the page.
    #[test]
    fn an_empty_query_string_leaves_the_body_query_untouched() {
        let url = serde_urlencoded::from_str::<TemporalPostQueryParams>("").expect("query parses");
        let mut params = TemporalQueryParameters::default();

        url.apply_to(&mut params);

        pretty_assertions::assert_eq!(params.page, PageParameters::default());
        pretty_assertions::assert_eq!(params.count, None);
        assert!(!params.is_local());
    }
}
