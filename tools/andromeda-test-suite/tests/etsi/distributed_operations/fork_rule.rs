//! The request rule of a stubbed context source answer.
//!
//! The rule reads the request target lower-cased, comparing without case: the path is the stub's path followed by any
//! number of `/`, and a stub naming query tokens requires a query holding each token. A stub's `attrs` rule restricts
//! every `attrs` parameter of the query to the listed names, separated by `,` or `%2C`; an empty list admits only an
//! empty `attrs`. A percent-encoded `:` or `@` reads as the character it encodes.

use http::Method;
use serde_json::Value;
use wiremock::{Match, Request};

/// What a stub requires of the request target after its path.
#[derive(Debug)]
enum TargetRule {
    /// The path followed by `/` alone, with any query or none.
    Resource,
    /// The path followed by `/` alone, then a query holding every token and an `attrs` naming only `attrs`.
    Query {
        /// The texts the query holds, lower-cased.
        tokens: Vec<String>,
        /// The names an `attrs` parameter may list, lower-cased; `None` leaves `attrs` free.
        attrs: Option<Vec<String>>,
    },
    /// The path followed by `/` alone and no query, or any target below `base` with a query that holds every token
    /// in its path or its query.
    ResourceOrTokens {
        /// The prefix of every target the token alternative admits.
        base: String,
        /// The texts the target holds, lower-cased.
        tokens: Vec<String>,
    },
    /// The path, then exactly the query `query` followed by `/` alone: a stub whose target names its query literally.
    Literal {
        /// The query the target holds, lower-cased.
        query: String,
    },
}

/// The rule a stubbed answer of the context source matches requests with.
#[derive(Debug)]
pub(crate) struct ForkRule {
    /// The method the request uses.
    method: Method,
    /// The path the target starts with, lower-cased.
    path: String,
    /// What the target holds after the path.
    target: TargetRule,
}

/// The lower-cased texts of `items`.
fn lowered(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_lowercase()).collect()
}

/// `text` with the percent-encoded `:` and `@` decoded, the way the rule reads an id or a name.
fn decoded(text: &str) -> String {
    text.replace("%3a", ":").replace("%40", "@")
}

impl ForkRule {
    /// The stub of `method` on the resource `path`, with any query.
    pub(crate) fn resource(method: Method, path: &str) -> ForkRule {
        ForkRule {
            method,
            path: path.to_lowercase(),
            target: TargetRule::Resource,
        }
    }

    /// The stub of `method` on `path` whose query holds every one of `tokens` and whose `attrs` lists only `attrs`
    /// when `attrs` names a list.
    pub(crate) fn query(method: Method, path: &str, tokens: &[&str], attrs: Option<&[&str]>) -> ForkRule {
        ForkRule {
            method,
            path: path.to_lowercase(),
            target: TargetRule::Query {
                tokens: lowered(tokens),
                attrs: attrs.map(lowered),
            },
        }
    }

    /// The stub of `method` on the resource `path` without a query, or on any target below `base` that has a query
    /// and holds every one of `tokens`.
    pub(crate) fn resource_or_tokens(method: Method, base: &str, path: &str, tokens: &[&str]) -> ForkRule {
        ForkRule {
            method,
            path: path.to_lowercase(),
            target: TargetRule::ResourceOrTokens {
                base: base.to_lowercase(),
                tokens: lowered(tokens),
            },
        }
    }

    /// The stub of `method` on exactly `path` with the query `query`.
    pub(crate) fn literal(method: Method, path: &str, query: &str) -> ForkRule {
        ForkRule {
            method,
            path: path.to_lowercase(),
            target: TargetRule::Literal { query: query.to_lowercase() },
        }
    }

    /// Whether `rest` is the remainder of a target after the stub's path that names the same resource.
    fn only_slashes(rest: &str) -> bool {
        rest.chars().all(|character| character == '/')
    }

    /// Whether every `attrs` parameter of `query` lists only `allowed`.
    fn attrs_allowed(query: &str, allowed: &[String]) -> bool {
        query.split('&').filter_map(|pair| pair.strip_prefix("attrs=")).all(|value| {
            let value = decoded(value);
            value.is_empty()
                || (!allowed.is_empty()
                    && value
                        .replace("%2c", ",")
                        .split(',')
                        .all(|name| allowed.iter().any(|candidate| candidate == name)))
        })
    }
}

/// The body rule of a stubbed `POST /entityOperations/query`: the first item of the body's `entities` names the
/// entity type and the id.
#[derive(Debug)]
pub(crate) struct QueriedEntity {
    /// The `type` the first item names.
    pub(crate) entity_type: String,
    /// The `id` the first item names.
    pub(crate) id: String,
}

impl Match for QueriedEntity {
    fn matches(&self, request: &Request) -> bool {
        serde_json::from_slice::<Value>(&request.body).is_ok_and(|body| {
            body.pointer("/entities/0/type").and_then(Value::as_str) == Some(self.entity_type.as_str())
                && body.pointer("/entities/0/id").and_then(Value::as_str) == Some(self.id.as_str())
        })
    }
}

impl Match for ForkRule {
    fn matches(&self, request: &Request) -> bool {
        if request.method != self.method {
            return false;
        }
        let path = request.url.path().to_lowercase();
        let query = request.url.query().map(str::to_lowercase);
        match &self.target {
            TargetRule::Resource => path.strip_prefix(&self.path).is_some_and(ForkRule::only_slashes),
            TargetRule::Query { tokens, attrs } => {
                let Some(query) = query else {
                    return false;
                };
                let readable = decoded(&query);
                path.strip_prefix(&self.path).is_some_and(ForkRule::only_slashes)
                    && tokens.iter().all(|token| readable.contains(token.as_str()))
                    && attrs.as_ref().is_none_or(|allowed| ForkRule::attrs_allowed(&query, allowed))
            }
            TargetRule::ResourceOrTokens { base, tokens } => match query {
                None => path.strip_prefix(&self.path).is_some_and(ForkRule::only_slashes),
                Some(query) => path.strip_prefix(base.as_str()).is_some_and(|rest| {
                    let readable = decoded(&format!("{rest}?{query}"));
                    tokens.iter().all(|token| readable.contains(token.as_str()))
                }),
            },
            TargetRule::Literal { query: literal } => {
                path == self.path && query.is_some_and(|query| query.strip_prefix(literal.as_str()).is_some_and(ForkRule::only_slashes))
            }
        }
    }
}
