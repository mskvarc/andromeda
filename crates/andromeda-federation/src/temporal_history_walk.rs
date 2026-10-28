//! Reading each context source's temporal history to the end of the queried
//! window (clauses 5.7.4.4 and 6.3.10).
//!
//! A source answers a temporal query in pages along two dimensions. The
//! entities it holds are paged by its `next` pointers, which the source walk
//! follows; the instances of each entity are cut at the clause 6.3.10 page
//! size, which the source says with a 206 and a `Content-Range` naming the
//! instants it did answer for. An aggregate of the merged instances (clause
//! 5.7.4.4) needs every instance of the window, so the walk asks the source
//! again with the window resumed at the instant the answer stopped at, until
//! no answer is partial. The resumed instant must advance, or a source whose
//! instances share one instant beyond its page size would be asked forever.

use crate::{
    FederationService,
    incomplete_answer::{IncompleteReason, incomplete_answer},
    query_forwarding_error::QueryForwardingError,
    read_fan_out::{ForwardedGetConditions, ForwardedRead, SourceAnswer},
    received_temporal_answer::ReceivedTemporalAnswer,
    request::HistoryBudget,
    result_budget::ResultBudget,
    temporal_answer_reading::ReadTemporalEntity,
};
use andromeda_http_headers::ngsild_content_range::NgsiLdContentRange;
use andromeda_ngsi_ld_lexicon::{
    time::{date_time::NgsiLdDateTime, rendering::instant_text},
    time_rel::TimeRel,
};
use andromeda_ngsi_ld_model::{received::ContextualEntity, registration::ContextSourceRegistration};
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use chrono::{DateTime, Utc};
use futures::{StreamExt, stream};
use url::Url;

/// The window parameters a resumed request rewrites.
const WINDOW_PARAMETERS: [&str; 3] = ["timerel", "timeAt", "endTimeAt"];

impl FederationService {
    /// Walk every target's answer to its last page and its history to the end
    /// of the window, all targets concurrently.
    ///
    /// # Errors
    ///
    /// Returns [`QueryForwardingError::TooManyResults`] once the targets
    /// together answer with more entities than the entity budget allows, and
    /// [`QueryForwardingError::TooManyInstances`] once they answer with more
    /// instances than the instance budget allows (clause 4.12).
    pub(crate) async fn fan_out_history_walk(
        &self,
        targets: Vec<(ContextSourceRegistration, ForwardedRead)>,
        conditions: ForwardedGetConditions<'_>,
        budget: HistoryBudget<'_>,
    ) -> Result<Vec<SourceAnswer<ReceivedTemporalAnswer<ReadTemporalEntity>>>, QueryForwardingError> {
        stream::iter(targets)
            .map(|(registration, first)| async move { self.walk_history(&registration, first, conditions, budget).await })
            .buffer_unordered(self.csr_concurrency)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect()
    }

    /// Walk one target's answer, then resume it past every partial answer
    /// until the window is exhausted. Each walk is read under the contexts its
    /// evolutions were written in before its instances are counted.
    async fn walk_history(
        &self,
        registration: &ContextSourceRegistration,
        first: ForwardedRead,
        conditions: ForwardedGetConditions<'_>,
        budget: HistoryBudget<'_>,
    ) -> Result<SourceAnswer<ReceivedTemporalAnswer<ReadTemporalEntity>>, QueryForwardingError> {
        let first_url = match &first {
            ForwardedRead::Get(Ok(url)) => Some(url.clone()), // clone: every resumed request rewrites the first request's window
            ForwardedRead::Get(Err(_)) | ForwardedRead::PostQuery { .. } => None,
        };
        let walked = self
            .walk_source::<ReceivedTemporalAnswer<ContextualEntity>>(registration, first, conditions, budget.entities)
            .await?;
        let mut answer = self.read_source_answer(walked).await;
        let Some(gathered) = answer.entry.as_mut() else {
            return Ok(answer);
        };
        budget.instances.charge(gathered.instance_count())?;
        let mut resumed_at: Option<DateTime<Utc>> = None;
        while let Some(stopped_at) = least_range_end(&gathered.partial_ranges) {
            let resumed = first_url
                .as_ref()
                .filter(|_| resumed_at.is_none_or(|previous| stopped_at > previous))
                .and_then(|url| resumed_window(url, stopped_at));
            let Some(resumed) = resumed else {
                // The stop stays on the answer: a history that could not be resumed is
                // complete only up to it (clause 6.3.10).
                gathered.warnings.push(incomplete_answer(&registration.id, IncompleteReason::HistoryCutShort));
                break;
            };
            gathered.partial_ranges.clear();
            // The resumed walk gathers entities the first one already counted,
            // so it is bounded on its own rather than charged to the request's
            // budget a second time.
            let resumed_budget = ResultBudget::new(budget.entities.limits());
            let page = self
                .walk_source::<ReceivedTemporalAnswer<ContextualEntity>>(registration, ForwardedRead::Get(Ok(resumed)), conditions, &resumed_budget)
                .await?;
            let Some(page) = page.entry else {
                break;
            };
            let page = self.read_temporal_answer(page).await;
            // The pages of one entity are put together once they are read into
            // the model, each instance once; what is charged is what the page
            // carried, the instances at the instant two pages meet included.
            budget.instances.charge(page.instance_count())?;
            gathered.entities.extend(page.entities);
            gathered.warnings.extend(page.warnings);
            gathered.partial_ranges = page.partial_ranges;
            resumed_at = Some(stopped_at);
        }
        Ok(answer)
    }
}

/// The earliest instant any partial answer stopped at: every instance up to it
/// has been answered on every page.
pub(crate) fn least_range_end(ranges: &[NgsiLdContentRange]) -> Option<DateTime<Utc>> {
    ranges.iter().map(NgsiLdContentRange::end).min()
}

/// `first` with its window resumed at `stopped_at` (clause 6.3.10), or `None`
/// when the first request carried no window to resume.
fn resumed_window(first: &Url, stopped_at: DateTime<Utc>) -> Option<Url> {
    let pairs: Vec<(String, String)> = first.query_pairs().map(|(name, value)| (name.into_owned(), value.into_owned())).collect();
    let value_of = |name: &str| pairs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str());
    let instant_of = |name: &str| value_of(name).map(|raw| raw.parse::<NgsiLdDateTime>().map(NgsiLdDateTime::get)).transpose();
    let timerel = value_of("timerel")?.parse::<TimeRel>().ok()?;
    let interval = TimeInterval::from_parts(Some(timerel), instant_of("timeAt").ok()?, instant_of("endTimeAt").ok()?).ok()?;
    let (timerel, time_at, end_time_at) = interval.resumed_at(stopped_at).parts();
    let resumed = [
        ("timerel", timerel.map(|relation| relation.to_string())),
        ("timeAt", time_at.map(instant_text)),
        ("endTimeAt", end_time_at.map(instant_text)),
    ];
    let mut url = first.clone(); // clone: the resumed request is the first one with another window
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        for (name, value) in pairs.iter().filter(|(name, _)| !WINDOW_PARAMETERS.contains(&name.as_str())) {
            query.append_pair(name, value);
        }
        for (name, value) in resumed.iter().filter_map(|(name, value)| value.as_deref().map(|value| (name, value))) {
            query.append_pair(name, value);
        }
    }
    Some(url)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal_history_walk::resumed_window;
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use chrono::{DateTime, Utc};
    use url::Url;

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    /// Clause 6.3.10: what remains of a `before` window after the instant the
    /// answer stopped at is the `between` window from there to its end.
    #[test]
    fn a_before_window_resumes_as_between() {
        let first =
            Url::parse("http://source/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=before&timeAt=2030-01-01T00:00:00Z&limit=1000").expect("a valid URL");
        let resumed = resumed_window(&first, at("2020-01-02T00:00:00Z")).expect("a window to resume");
        pretty_assertions::assert_eq!(
            resumed.query(),
            Some("type=Vehicle&limit=1000&timerel=between&timeAt=2020-01-02T00%3A00%3A00Z&endTimeAt=2030-01-01T00%3A00%3A00Z")
        );
    }

    /// An `after` window resumes as an `after` window opening where the answer
    /// stopped.
    #[test]
    fn an_after_window_resumes_where_the_answer_stopped() {
        let first = Url::parse("http://source/t?timerel=after&timeAt=2019-01-01T00:00:00Z").expect("a valid URL");
        let resumed = resumed_window(&first, at("2020-01-02T00:00:00Z")).expect("a window to resume");
        pretty_assertions::assert_eq!(resumed.query(), Some("timerel=after&timeAt=2020-01-02T00%3A00%3A00Z"));
    }

    /// A request carrying no window has nothing to resume.
    #[test]
    fn a_request_without_a_window_is_not_resumed() {
        let first = Url::parse("http://source/t?type=Vehicle").expect("a valid URL");
        assert!(resumed_window(&first, at("2020-01-02T00:00:00Z")).is_none());
    }
}
