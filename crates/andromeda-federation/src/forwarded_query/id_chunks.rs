//! Keeping a forwarded GET query inside the URL budget (clauses 4.3.6.7 and
//! 5.5.14).
//!
//! A query run under an `EntityMap` names every entity of the map in its `id`
//! parameter, and a GET carries that list in the URL. The list is cut into
//! several requests of one registration, each holding the whole of every other
//! parameter, so a source receives URLs it accepts and the answers merge as the
//! pages of one source.

use crate::{error::FederationError, forwarded_parameter::ForwardedParameter, forwarded_query::ForwardedQuery};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::mem::take;
use url::{Url, form_urlencoded};

/// The most bytes a forwarded GET URL may take, a length every common HTTP
/// server and intermediary accepts.
pub(crate) const FORWARDED_URL_BUDGET: usize = 8_192;

/// The bytes a comma takes in a form-encoded value.
const ENCODED_COMMA_LEN: usize = 3;

/// The bytes `value` takes once form-encoded.
fn encoded_len(value: &str) -> usize {
    form_urlencoded::byte_serialize(value.as_bytes()).map(str::len).sum()
}

/// The bytes the pair `name=value` takes in a query string.
fn pair_len(name: &str, value: &str) -> usize {
    encoded_len(name) + 1 + encoded_len(value)
}

impl ForwardedQuery {
    /// The queries that carry this one to `resource` within
    /// [`FORWARDED_URL_BUDGET`]: the query itself when its URL fits, otherwise
    /// one query per run of identifiers that fits, in the order the client
    /// listed them. `resource` is the URL the query string is appended to.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::ForwardedUrlOverBudget`] when no split fits:
    /// the parameters other than `id` fill the budget, or a single identifier
    /// does not fit beside them.
    pub(crate) fn within_url_budget(self, resource: &Url, registration: &Identifier) -> Result<Vec<ForwardedQuery>, FederationError> {
        let whole = resource.as_str().len() + 1 + self.to_query_string().len();
        if whole <= FORWARDED_URL_BUDGET {
            return Ok(vec![self]);
        }
        let over_budget = |smallest| FederationError::ForwardedUrlOverBudget {
            registration: registration.clone(), // clone: the error owns the registration it names
            budget: FORWARDED_URL_BUDGET,
            smallest,
        };
        let Some(position) = self.pairs.iter().position(|(name, _)| *name == ForwardedParameter::Id) else {
            return Err(over_budget(whole));
        };
        let others = self
            .pairs
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != position)
            .map(|(_, (name, value))| pair_len(&name.to_string(), value) + 1)
            .sum::<usize>();
        let fixed = resource.as_str().len() + 1 + others + pair_len(&ForwardedParameter::Id.to_string(), "");
        let mut runs: Vec<Vec<&str>> = Vec::new();
        let mut open: Vec<&str> = Vec::new();
        let mut open_len = 0;
        for id in self.pairs[position].1.split(',') {
            let alone = encoded_len(id);
            if fixed + alone > FORWARDED_URL_BUDGET {
                return Err(over_budget(fixed + alone));
            }
            let joined = if open.is_empty() { alone } else { open_len + ENCODED_COMMA_LEN + alone };
            if fixed + joined > FORWARDED_URL_BUDGET {
                runs.push(take(&mut open));
                open_len = alone;
            } else {
                open_len = joined;
            }
            open.push(id);
        }
        runs.push(open);
        Ok(runs
            .into_iter()
            .map(|run| {
                let mut pairs = self.pairs.clone(); // clone: every run keeps all the parameters but the identifier list
                pairs[position].1 = run.join(",");
                ForwardedQuery { pairs }
            })
            .collect())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::FederationError,
        forwarded_parameter::ForwardedParameter,
        forwarded_query::{ForwardedQuery, id_chunks::FORWARDED_URL_BUDGET},
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use url::Url;

    const RESOURCE: &str = "http://peer.example.org/ngsi-ld/v1/entities";

    fn resource() -> Url {
        Url::parse(RESOURCE).expect("a valid URL")
    }

    fn registration() -> Identifier {
        "urn:ngsi-ld:csr:peer".parse().expect("a valid identifier")
    }

    fn query(ids: &[String]) -> ForwardedQuery {
        ForwardedQuery {
            pairs: vec![
                (ForwardedParameter::Id, ids.join(",")),
                (ForwardedParameter::Type, "Vehicle".to_owned()),
                (ForwardedParameter::Limit, "1000".to_owned()),
            ],
        }
    }

    fn vehicle_ids(count: usize) -> Vec<String> {
        (0..count).map(|number| format!("urn:ngsi-ld:Vehicle:{number:05}")).collect()
    }

    fn url_len(query: &ForwardedQuery) -> usize {
        RESOURCE.len() + 1 + query.to_query_string().len()
    }

    fn ids_of(query: &ForwardedQuery) -> Vec<String> {
        query
            .value_of(ForwardedParameter::Id)
            .expect("the id parameter is written")
            .split(',')
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn a_query_whose_url_fits_is_sent_whole() {
        let whole = query(&vehicle_ids(50));
        let runs = whole.clone().within_url_budget(&resource(), &registration()).expect("the query fits");
        pretty_assertions::assert_eq!(runs, vec![whole]);
    }

    #[test]
    fn three_thousand_identifiers_split_into_urls_within_the_budget() {
        let ids = vehicle_ids(3_000);
        let runs = query(&ids).within_url_budget(&resource(), &registration()).expect("the identifiers split");
        assert!(runs.len() > 1, "{} runs", runs.len());
        for run in &runs {
            assert!(url_len(run) <= FORWARDED_URL_BUDGET, "{} bytes", url_len(run));
            pretty_assertions::assert_eq!(run.value_of(ForwardedParameter::Type), Some("Vehicle"));
            pretty_assertions::assert_eq!(run.value_of(ForwardedParameter::Limit), Some("1000"));
        }
        pretty_assertions::assert_eq!(runs.iter().flat_map(ids_of).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn the_last_identifier_that_fits_stays_in_one_request_and_one_more_byte_splits_it() {
        let fixed = RESOURCE.len() + 1 + "id=".len() + "&type=Vehicle&limit=1000".len();
        let encoded_comma = 3;
        let room = FORWARDED_URL_BUDGET - fixed - encoded_comma;
        let at_budget = vec!["a".repeat(room / 2), "b".repeat(room - room / 2)];
        let runs = query(&at_budget).within_url_budget(&resource(), &registration()).expect("the identifiers fit");
        pretty_assertions::assert_eq!(runs.len(), 1);
        pretty_assertions::assert_eq!(url_len(&runs[0]), FORWARDED_URL_BUDGET);
        let over_budget = vec!["a".repeat(room / 2), "b".repeat(room - room / 2 + 1)];
        let runs = query(&over_budget)
            .within_url_budget(&resource(), &registration())
            .expect("the identifiers split");
        pretty_assertions::assert_eq!(runs.iter().flat_map(ids_of).collect::<Vec<_>>(), over_budget);
        pretty_assertions::assert_eq!(runs.len(), 2);
    }

    #[test]
    fn one_identifier_longer_than_the_budget_cannot_be_prepared() {
        let error = query(&["a".repeat(FORWARDED_URL_BUDGET)])
            .within_url_budget(&resource(), &registration())
            .expect_err("no split fits");
        assert!(matches!(error, FederationError::ForwardedUrlOverBudget { .. }), "{error}");
    }

    #[test]
    fn parameters_other_than_the_identifiers_filling_the_budget_cannot_be_prepared() {
        let long = ForwardedQuery {
            pairs: vec![(ForwardedParameter::IdPattern, "a".repeat(FORWARDED_URL_BUDGET))],
        };
        let error = long.within_url_budget(&resource(), &registration()).expect_err("no split fits");
        assert!(matches!(error, FederationError::ForwardedUrlOverBudget { .. }), "{error}");
    }
}
