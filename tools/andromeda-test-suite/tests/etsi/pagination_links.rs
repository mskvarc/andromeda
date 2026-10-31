//! The pagination links of a query answer, reduced to what the conformance tests compare: the relation, the media type and
//! the `limit` and `offset` of the page each leads to.

use andromeda_test_suite::wire::header_values;
use googletest::prelude::*;
use http::{HeaderMap, header::LINK};
use iri_rs::{IriBuf, IriRefBuf};
use mediatype::MediaType;
use ngsi_ld::http::headers::{Link, LinkValue, RelationType};
use std::fmt::Debug;

/// A pagination link of a query answer as the comparison reads it: its target keeps the path and the `limit` and
/// `offset` parameters, and the relation types and parameters of the link stay as they are.
#[derive(Debug, PartialEq)]
pub(crate) struct PageLink {
    /// The target, with the query reduced to its `limit` and `offset` parameters.
    target: IriRefBuf,
    /// The relation types of the `rel` parameter, in order.
    rels: Vec<RelationType>,
    /// The `type` parameter as text, when present.
    media_type: Option<String>,
    /// The `profile` parameter, when present.
    profile: Option<IriBuf>,
}

impl PageLink {
    /// The link `value` reads as, with its target reduced by [`limit_and_offset`].
    fn reduced(value: &LinkValue) -> Result<PageLink> {
        Ok(PageLink {
            target: limit_and_offset(value.target())?,
            // The comparison owns the links it reads, so each parameter is copied out of the header value.
            rels: value.rels().to_vec(),
            media_type: value.media_type().map(|media_type| media_type.as_str().to_owned()),
            profile: value.profile().cloned(),
        })
    }
}

/// The pagination link of `rel` to `target`, which names the `limit` and `offset` of the page it leads to, with the
/// `type` parameter `media_type` the case expects.
pub(crate) fn page_link(target: &str, rel: RelationType, media_type: &MediaType<'_>) -> Result<PageLink> {
    Ok(PageLink {
        target: IriRefBuf::new(target.to_owned())?,
        rels: vec![rel],
        media_type: Some(media_type.to_string()),
        profile: None,
    })
}

/// `target` with its query reduced to the `limit` and `offset` parameters, in the order written.
fn limit_and_offset(target: &IriRefBuf) -> Result<IriRefBuf> {
    let text = target.as_str();
    let reduced = match text.split_once('?') {
        None => text.to_owned(),
        Some((path, query)) => {
            let kept = query
                .split('&')
                .filter(|pair| matches!(pair.split_once('=').map_or(*pair, |(key, _)| key), "limit" | "offset"))
                .collect::<Vec<_>>()
                .join("&");
            if kept.is_empty() { path.to_owned() } else { format!("{path}?{kept}") }
        }
    };
    Ok(IriRefBuf::new(reduced)?)
}

/// Whether a link-value is a frame the pagination comparison leaves out: the first and last pages, and the JSON-LD
/// context.
fn is_frame(value: &LinkValue) -> bool {
    value.is_context_link() || value.has_rel(&RelationType::First) || value.has_rel(&RelationType::Last)
}

/// The pagination links of the `Link` field of `headers` that remain once the frames are left out, each reduced by
/// [`PageLink::reduced`]. Every field line is read: RFC 9110 clause 5.3 makes several lines one comma-separated list.
fn page_links(headers: &HeaderMap) -> Result<Vec<PageLink>> {
    header_values(headers, &LINK)
        .into_iter()
        .map(|line| Ok(line.parse::<Link>()?))
        .collect::<Result<Vec<_>>>()?
        .iter()
        .flat_map(Link::values)
        .filter(|value| !is_frame(value))
        .map(PageLink::reduced)
        .collect()
}

/// Requires the pagination links of the `Link` field of `headers` to be `expected`, in any order, each compared by
/// its target's `limit` and `offset`.
pub(crate) fn expect_page_links(headers: &HeaderMap, expected: &[PageLink]) -> Result<()> {
    let links = page_links(headers)?;
    expect_that!(links.len(), eq(expected.len()));
    for link in expected {
        expect_that!(&links, contains(eq(link)));
    }
    Ok(())
}
