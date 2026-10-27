//! How the link-values of one message are laid out over `Link` field lines.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// How the link-values of one message are laid out over `Link` field lines.
///
/// RFC 9110 section 5.3 lets a sender split a list-based field such as `Link`
/// over several field lines or join it into one "without changing the
/// semantics of the message", so every layout is the same message; they differ
/// only in which clients read them correctly.
///
/// Whatever the layout, the link-values keep one order: the JSON-LD `@context`
/// link first, then the pagination links as `first`, `prev`, `next`, `last`,
/// then every other link-value in the order it was written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(
    description = "How the `Link` values of one response or HTTP notification are spread over field lines. Every layout means the same; they differ in which clients read them correctly. The order is always the `@context` link, the pagination links, then any other value."
)]
pub enum LinkFieldLayout {
    /// Every link-value on one field line, separated by `, `.
    #[default]
    #[schemars(description = "Puts every `Link` value on one field line, separated by commas, for clients that read only one field line.")]
    Combined,
    /// One field line per concern: the `@context` link, the pagination links,
    /// and every other link-value, each concern that is present on its own line.
    #[schemars(description = "Gives the `@context` link, the pagination links and any other values a field line each.")]
    Grouped,
    /// Every link-value on its own field line.
    #[schemars(description = "Gives every `Link` value its own field line, for clients that never split a line on commas.")]
    Separate,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::schema::link_field_layout::LinkFieldLayout;

    #[test]
    fn the_layout_round_trips_through_its_kebab_case_token() {
        for (layout, token) in [
            (LinkFieldLayout::Combined, "combined"),
            (LinkFieldLayout::Grouped, "grouped"),
            (LinkFieldLayout::Separate, "separate"),
        ] {
            pretty_assertions::assert_eq!(layout.to_string(), token);
            pretty_assertions::assert_eq!(token.parse::<LinkFieldLayout>().expect("a layout token"), layout);
        }
        pretty_assertions::assert_eq!(LinkFieldLayout::default(), LinkFieldLayout::Combined);
    }
}
