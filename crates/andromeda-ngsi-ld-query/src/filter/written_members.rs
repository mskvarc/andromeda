//! The written spelling of a filter's trailing `[…]` members, carried back
//! onto an expanded filter read from its persisted form (clause 4.9).
//!
//! Expanding a filter rewrites a compound member to the IRI it addresses in a
//! stored structured value, while a `LanguageProperty`'s target language and a
//! raw JSON key are read in the spelling the client wrote. The expanded filter
//! holds both, and its clause 4.9 text holds only the first; the text
//! [`Filter::written_member_text`]
//! renders holds the second, and this module joins the two again.

use crate::filter::{Filter, trailing_group::TrailingGroup};

impl Filter {
    /// This expression, each trailing member's written spelling read from
    /// `written`: the same filter rendered by
    /// [`Filter::written_member_text`]
    /// and parsed back.
    ///
    /// The two are one filter in two spellings, so their Query Terms pair up in
    /// order. A pair whose members do not line up keeps its own spelling, as
    /// does every term of an expression whose term count `written` does not
    /// share.
    #[must_use]
    pub fn with_written_members(mut self, written: &Filter) -> Filter {
        let written_paths = written.terminal_paths();
        let mut own_paths = self.terminal_paths_mut();
        if own_paths.len() == written_paths.len() {
            for (own, written) in own_paths.iter_mut().zip(written_paths) {
                if let (Some(TrailingGroup::Members(own)), Some(written)) = (&mut own.trailing, written.members())
                    && own.addressed.len() == written.addressed.len()
                {
                    own.written.clone_from(&written.addressed);
                }
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{Filter, expansion::expand_filter, json_key_attributes::JsonKeyAttributes},
        namespace_expander::EXAMPLE_NAMESPACE,
    };

    /// The filter as a store reads it back: the expanded text, joined with the
    /// written spelling of its members.
    async fn persisted_round_trip(q: &str) -> Filter {
        let expanded = expand_filter(q.parse::<Filter>().expect("parses"), &JsonKeyAttributes::default(), &EXAMPLE_NAMESPACE)
            .await
            .expect("expands");
        let addressed: Filter = expanded.to_string().parse().expect("the expanded text parses");
        let written: Filter = expanded.written_member_text().parse().expect("the written text parses");
        addressed.with_written_members(&written)
    }

    /// The compound reading still addresses the expanded member, and every term
    /// of a boolean expression gets its own members back.
    #[tokio::test]
    async fn every_term_keeps_its_members_in_both_spellings() {
        let expr = persisted_round_trip("speed>1;(address[city]==\"Berlin\"|name[en]==\"Door\")").await;
        let rendered = (expr.to_string(), expr.written_member_text());
        pretty_assertions::assert_eq!(
            rendered,
            (
                "\"https://example.com/speed\">1;(\"https://example.com/address\"[\"https://example.com/city\"]==\"Berlin\"|\"https://example.com/name\"[\"https://example.com/en\"]==\"Door\")".to_owned(),
                "\"https://example.com/speed\">1;(\"https://example.com/address\"[city]==\"Berlin\"|\"https://example.com/name\"[en]==\"Door\")".to_owned(),
            )
        );
    }

    /// A written text naming other terms leaves the expression as it is.
    #[test]
    fn a_written_text_of_another_shape_changes_nothing() {
        let expanded = "\"https://example.com/name\"[\"https://example.com/en\"]==\"Door\""
            .parse::<Filter>()
            .expect("parses");
        let other = "a==1;b==2".parse::<Filter>().expect("parses");
        pretty_assertions::assert_eq!(
            expanded.clone().with_written_members(&other).written_member_text(),
            expanded.written_member_text()
        );
    }
}
