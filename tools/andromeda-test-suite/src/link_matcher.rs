//! Requirements on a typed `Link` header (RFC 8288 section 3): a link-value naming a relation with the media type it
//! hints, and the order the relations of the header come in.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::http::headers::{Link, LinkValue, RelationType};

/// A link-value naming a relation, and the media type it hints when one is required.
#[derive(Debug, Clone, MatcherBase)]
pub struct LinkValueMatcher {
    /// The relation the value names among its `rel` types.
    relation: RelationType,
    /// The `type` parameter the value carries, or `None` for any.
    media_type: Option<String>,
}

/// A link-value whose `rel` names `relation`; on a whole header, one of its link-values.
#[must_use]
pub const fn link_value(relation: RelationType) -> LinkValueMatcher {
    LinkValueMatcher { relation, media_type: None }
}

impl LinkValueMatcher {
    /// Also requires the `type` parameter `media_type`, compared case-insensitively as media types are (RFC 9110
    /// section 8.3.1).
    #[must_use]
    pub fn with_media_type(self, media_type: &str) -> LinkValueMatcher {
        LinkValueMatcher {
            media_type: Some(media_type.to_owned()),
            ..self
        }
    }

    /// Whether `value` names the relation and hints the media type.
    fn accepts(&self, value: &LinkValue) -> bool {
        value.has_rel(&self.relation)
            && self
                .media_type
                .as_ref()
                .is_none_or(|expected| value.media_type().is_some_and(|actual| actual.to_string().eq_ignore_ascii_case(expected)))
    }

    /// The requirement in words.
    fn requirement(&self) -> String {
        match &self.media_type {
            Some(media_type) => format!("a link-value of relation {} and type {media_type}", self.relation),
            None => format!("a link-value of relation {}", self.relation),
        }
    }
}

impl Matcher<&LinkValue> for LinkValueMatcher {
    fn matches(&self, actual: &LinkValue) -> MatcherResult {
        self.accepts(actual).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is {}", self.requirement()).into(),
            MatcherResult::NoMatch => format!("isn't {}", self.requirement()).into(),
        }
    }
}

impl Matcher<&Link> for LinkValueMatcher {
    fn matches(&self, actual: &Link) -> MatcherResult {
        actual.values().iter().any(|value| self.accepts(value)).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has {}", self.requirement()).into(),
            MatcherResult::NoMatch => format!("has no {}", self.requirement()).into(),
        }
    }
}

/// A header whose relations hold some in a given order, any others between them.
#[derive(Debug, Clone, MatcherBase)]
pub struct RelationOrderMatcher {
    /// The relations, in the order the header names them.
    relations: Vec<RelationType>,
}

/// A header naming `relations` in this order across its link-values, others allowed before, between and after them.
#[must_use]
pub fn relations_in_order(relations: impl IntoIterator<Item = RelationType>) -> RelationOrderMatcher {
    RelationOrderMatcher {
        relations: relations.into_iter().collect(),
    }
}

impl Matcher<&Link> for RelationOrderMatcher {
    fn matches(&self, actual: &Link) -> MatcherResult {
        let mut named = actual.values().iter().flat_map(LinkValue::rels);
        self.relations.iter().all(|relation| named.any(|candidate| candidate == relation)).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        let relations = self.relations.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        match result {
            MatcherResult::Match => format!("names the relations {relations} in this order").into(),
            MatcherResult::NoMatch => format!("doesn't name the relations {relations} in this order").into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::link_matcher::{link_value, relations_in_order};
    use googletest::prelude::*;
    use iri_rs::IriRefBuf;
    use ngsi_ld::{
        http::headers::{Link, LinkValue, RelationType},
        model::NgsiLdMediaType,
    };

    /// A header with the context link, then the previous and next pages.
    fn paged() -> Result<Link> {
        let context = LinkValue::json_ld_context(IriRefBuf::new("http://context.test/core.jsonld".to_owned())?);
        let prev = LinkValue::builder(IriRefBuf::new("/ngsi-ld/v1/entities?offset=0".to_owned())?)
            .rel(RelationType::Prev)
            .media_type(NgsiLdMediaType::Json.media_type())
            .build();
        let next = LinkValue::builder(IriRefBuf::new("/ngsi-ld/v1/entities?offset=2".to_owned())?)
            .rel(RelationType::Next)
            .build();
        Ok(Link::try_from_values(vec![context, prev, next])?)
    }

    #[gtest]
    fn a_link_value_is_matched_by_relation_and_media_type() -> Result<()> {
        let link = paged()?;
        expect_that!(&link, link_value(RelationType::Prev).with_media_type("application/json"));
        expect_that!(&link, link_value(RelationType::Next));
        expect_that!(&link, not(link_value(RelationType::Next).with_media_type("application/json")));
        expect_that!(&link, not(link_value(RelationType::Last)));
        verify_that!(
            link.values().first(),
            some(link_value(RelationType::JsonLdContext).with_media_type("APPLICATION/LD+JSON"))
        )
    }

    #[gtest]
    fn relations_are_matched_in_the_order_the_header_names_them() -> Result<()> {
        let link = paged()?;
        expect_that!(&link, relations_in_order([RelationType::JsonLdContext, RelationType::Next]));
        expect_that!(&link, relations_in_order([RelationType::Prev, RelationType::Next]));
        verify_that!(&link, not(relations_in_order([RelationType::Next, RelationType::Prev])))
    }
}
