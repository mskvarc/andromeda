//! How the link-values of one message are laid out over `Link` field lines.
//!
//! RFC 9110 section 5.3 lets a sender split a list-based field such as `Link`
//! over several field lines or join it into one "without changing the
//! semantics of the message", and RFC 8288 section 3 makes a `Link` field value
//! a comma-separated list of link-values. Every layout here is therefore the
//! same message; they differ only in which clients read them correctly. A
//! client reading one field line (Go's `Header.Get`, the Hurl test runner)
//! needs one line, and a client that never splits a line on commas needs one
//! link-value per line.
//!
//! The layout is applied after a message is assembled, so a handler writes its
//! `@context` link (clause 6.3.6) and its pagination links (clause 6.3.10)
//! without knowing how they will be laid out. Each link-value is moved verbatim,
//! every parameter kept as it was written.

use crate::link::{header::Link, quoting::SplitAsciiUnquoted, relation_type::RelationType, value::LinkValue};
use andromeda_configuration::schema::link_field_layout::LinkFieldLayout;
use http::{HeaderMap, HeaderValue, header::LINK};

/// Lay the `Link` field lines of `headers` out as `layout` asks.
///
/// Field lines that are not visible ASCII cannot be split into link-values,
/// so a message carrying one keeps its `Link` field lines as they are.
pub fn apply_link_field_layout(layout: LinkFieldLayout, headers: &mut HeaderMap) {
    let Some(lines) = field_lines(layout, headers.get_all(LINK).iter()) else {
        return;
    };
    headers.remove(LINK);
    for line in lines {
        headers.append(LINK, line);
    }
}

/// The field lines `layout` makes of `lines`, or `None` when they are left as
/// they are: there is no `Link` field, a line is not visible ASCII, or a
/// laid-out line does not encode.
fn field_lines<'a>(layout: LinkFieldLayout, lines: impl Iterator<Item = &'a HeaderValue>) -> Option<Vec<HeaderValue>> {
    let mut values = lines
        .map(|line| line.to_str().ok().map(link_values))
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .flatten()
        .map(|raw| (FieldPosition::of(raw), raw))
        .collect::<Vec<_>>();
    if values.is_empty() {
        return None;
    }
    // Stable, so link-values of one position keep the order they were written in.
    values.sort_by_key(|(position, _)| *position);
    let values = values;
    let groups: Vec<Vec<&str>> = match layout {
        LinkFieldLayout::Combined => vec![values.iter().map(|(_, raw)| *raw).collect()],
        LinkFieldLayout::Grouped => values
            .chunk_by(|(left, _), (right, _)| left.concern() == right.concern())
            .map(|chunk| chunk.iter().map(|(_, raw)| *raw).collect())
            .collect(),
        LinkFieldLayout::Separate => values.iter().map(|(_, raw)| vec![*raw]).collect(),
    };
    groups.iter().map(|group| HeaderValue::from_str(&group.join(", ")).ok()).collect()
}

/// Join `lines`, each a `Link` field value, into the one field value a carrier
/// holding a single `Link` member writes, in the order every layout keeps.
///
/// A carrier with one slot per name, such as the metadata object of an MQTT
/// notification, cannot hold several `Link` lines, so it is always combined.
#[must_use]
pub fn combined_link_value<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut values = lines
        .into_iter()
        .flat_map(link_values)
        .map(|raw| (FieldPosition::of(raw), raw))
        .collect::<Vec<_>>();
    values.sort_by_key(|(position, _)| *position);
    let values = values;
    (!values.is_empty()).then(|| values.iter().map(|(_, raw)| *raw).collect::<Vec<_>>().join(", "))
}

/// The link-values of one field line, trimmed, with RFC 9110 section 5.6.1's
/// empty list elements dropped.
fn link_values(line: &str) -> impl Iterator<Item = &str> {
    SplitAsciiUnquoted::new(line, ",").map(str::trim).filter(|raw| !raw.is_empty())
}

/// Where a link-value stands in the order every layout keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FieldPosition {
    /// The JSON-LD `@context` link (clause 6.3.6).
    Context,
    /// The `first` pagination link.
    First,
    /// The `prev` pagination link (clause 6.3.10).
    Prev,
    /// The `next` pagination link (clause 6.3.10).
    Next,
    /// The `last` pagination link.
    Last,
    /// Any other link-value, including one that does not parse.
    Other,
}

/// The concern a link-value belongs to, one field line each under
/// [`LinkFieldLayout::Grouped`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldConcern {
    /// The `@context` link.
    Context,
    /// The pagination links.
    Pagination,
    /// Every other link-value.
    Other,
}

impl FieldPosition {
    /// The position of the raw link-value `raw`.
    fn of(raw: &str) -> FieldPosition {
        raw.parse::<Link>()
            .ok()
            .and_then(|link| link.values().first().map(FieldPosition::of_value))
            .unwrap_or(FieldPosition::Other)
    }

    /// The position of one parsed link-value.
    fn of_value(value: &LinkValue) -> FieldPosition {
        if value.is_context_link() {
            return FieldPosition::Context;
        }
        [
            (RelationType::First, FieldPosition::First),
            (RelationType::Prev, FieldPosition::Prev),
            (RelationType::Next, FieldPosition::Next),
            (RelationType::Last, FieldPosition::Last),
        ]
        .into_iter()
        .find_map(|(relation, position)| value.has_rel(&relation).then_some(position))
        .unwrap_or(FieldPosition::Other)
    }

    /// The concern this position belongs to.
    const fn concern(self) -> FieldConcern {
        match self {
            FieldPosition::Context => FieldConcern::Context,
            FieldPosition::First | FieldPosition::Prev | FieldPosition::Next | FieldPosition::Last => FieldConcern::Pagination,
            FieldPosition::Other => FieldConcern::Other,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::link::field_layout::{apply_link_field_layout, combined_link_value};
    use andromeda_configuration::schema::link_field_layout::LinkFieldLayout;
    use http::{HeaderMap, HeaderValue, header::LINK};

    const CONTEXT: &str = r#"<https://example.org/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;
    const FIRST: &str = r#"</entities?limit=1&offset=0>; rel="first"; type="application/json""#;
    const PREV: &str = r#"</entities?limit=1&offset=1>; rel="prev"; type="application/json""#;
    const NEXT: &str = r#"</entities?limit=1&offset=3>; rel="next"; type="application/json""#;
    const LAST: &str = r#"</entities?limit=1&offset=9>; rel="last"; type="application/json""#;
    const OTHER: &str = r#"<https://example.org/doc>; rel="describedby"; title="kept, as written""#;

    /// A message whose `Link` field lines are `lines`, in that order.
    fn headers_with(lines: &[&str]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for line in lines {
            headers.append(LINK, HeaderValue::from_str(line).expect("a visible ASCII line"));
        }
        headers
    }

    /// The `Link` field lines of `headers`, in order.
    fn lines(headers: &HeaderMap) -> Vec<&str> {
        headers.get_all(LINK).iter().map(|line| line.to_str().expect("visible ASCII")).collect()
    }

    /// What every `Link` producing path hands the layout: the `@context` line
    /// and a pagination line written as first, last, prev, next.
    fn context_and_pagination() -> HeaderMap {
        headers_with(&[CONTEXT, &[FIRST, LAST, PREV, NEXT].join(", ")])
    }

    #[test]
    fn combined_joins_every_link_value_into_one_line_context_first_then_first_prev_next_last() {
        let mut headers = context_and_pagination();

        apply_link_field_layout(LinkFieldLayout::Combined, &mut headers);

        pretty_assertions::assert_eq!(lines(&headers), vec![[CONTEXT, FIRST, PREV, NEXT, LAST].join(", ").as_str()]);
    }

    #[test]
    fn grouped_writes_the_context_and_the_pagination_links_on_a_line_each() {
        let mut headers = context_and_pagination();

        apply_link_field_layout(LinkFieldLayout::Grouped, &mut headers);

        pretty_assertions::assert_eq!(lines(&headers), vec![CONTEXT, [FIRST, PREV, NEXT, LAST].join(", ").as_str()]);
    }

    #[test]
    fn separate_writes_every_link_value_on_its_own_line() {
        let mut headers = context_and_pagination();

        apply_link_field_layout(LinkFieldLayout::Separate, &mut headers);

        pretty_assertions::assert_eq!(lines(&headers), vec![CONTEXT, FIRST, PREV, NEXT, LAST]);
    }

    /// RFC 8288 section 3: any other link-value follows the pagination links,
    /// every parameter kept verbatim, a comma in a quoted value included.
    #[test]
    fn another_link_value_follows_the_pagination_links_with_its_parameters_unchanged() {
        let mut combined = headers_with(&[OTHER, NEXT, CONTEXT]);
        apply_link_field_layout(LinkFieldLayout::Combined, &mut combined);
        pretty_assertions::assert_eq!(lines(&combined), vec![[CONTEXT, NEXT, OTHER].join(", ").as_str()]);

        let mut grouped = headers_with(&[OTHER, NEXT, CONTEXT]);
        apply_link_field_layout(LinkFieldLayout::Grouped, &mut grouped);
        pretty_assertions::assert_eq!(lines(&grouped), vec![CONTEXT, NEXT, OTHER]);
    }

    #[test]
    fn a_message_without_a_link_field_is_left_without_one() {
        let mut headers = HeaderMap::new();

        apply_link_field_layout(LinkFieldLayout::Combined, &mut headers);

        assert!(headers.get(LINK).is_none());
    }

    /// RFC 9110 section 5.3: a single-valued line is already every layout.
    #[test]
    fn a_lone_context_link_is_one_line_in_every_layout() {
        for layout in [LinkFieldLayout::Combined, LinkFieldLayout::Grouped, LinkFieldLayout::Separate] {
            let mut headers = headers_with(&[CONTEXT]);

            apply_link_field_layout(layout, &mut headers);

            pretty_assertions::assert_eq!(lines(&headers), vec![CONTEXT], "{layout}");
        }
    }

    #[test]
    fn a_value_that_does_not_parse_as_a_link_is_kept_after_the_ones_that_do() {
        let mut headers = headers_with(&["not a link", NEXT]);

        apply_link_field_layout(LinkFieldLayout::Combined, &mut headers);

        pretty_assertions::assert_eq!(lines(&headers), vec![[NEXT, "not a link"].join(", ").as_str()]);
    }

    #[test]
    fn a_line_that_is_not_visible_ascii_leaves_the_field_as_it_was() {
        let mut headers = headers_with(&[CONTEXT]);
        headers.append(
            LINK,
            HeaderValue::from_bytes(b"<https://example.org/\xe9>").expect("obs-text is a valid field value"),
        );

        apply_link_field_layout(LinkFieldLayout::Combined, &mut headers);

        pretty_assertions::assert_eq!(headers.get_all(LINK).iter().count(), 2);
    }

    #[test]
    fn combined_value_orders_the_context_link_before_one_written_ahead_of_it() {
        pretty_assertions::assert_eq!(combined_link_value([OTHER, CONTEXT]), Some([CONTEXT, OTHER].join(", ")));
        pretty_assertions::assert_eq!(combined_link_value([]), None);
    }
}
