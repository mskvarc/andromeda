//! The fan-out shared by every forwarded read (clauses 5.7.1.4, 5.7.2.4,
//! 5.7.3.4, 5.7.4.4 and 6.3.17).
//!
//! A current-entity read and a temporal read send the registrations they
//! selected the same kind of GET; they differ only in the resource path and in
//! how one answer is read back, which is what the callers supply.

use crate::{
    FederationService,
    cache::Cacheability,
    error::FederationError,
    outcome::{ForwardGetOutcome, ForwardReadOutcome, MatchedSources},
    request::{EntityMapInUse, TriggeringRequest},
    source_read::SourceRead,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::{
    query::body::Query,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
};
use futures::{StreamExt, stream};
use http::StatusCode;
use url::Url;

/// One forwarded read, resolved to the request it sends: a GET (queryEntity or
/// retrieveEntity) or a POST batch query (queryBatch), each carrying the URL its
/// construction resolved to or the error that stopped it (clause 4.3.6.1).
pub(crate) enum ForwardedRead {
    /// A GET to `/entities` or `/entities/{id}`.
    Get(Result<Url, FederationError>),
    /// A POST to `/entityOperations/query` with an NGSI-LD `Query` body.
    PostQuery {
        /// The `/entityOperations/query` URL, or the error that stopped it.
        url: Result<Url, FederationError>,
        /// The `Query` body (clause 6.23), boxed because it is the widest
        /// member of a read.
        body: Box<Query>,
    },
}

/// What every GET of one fan-out carries, whichever registration receives it.
///
/// These four are one thing (the triggering request as each context source
/// sees it), and none of them varies between the sources contacted. The
/// forwarded requests all resolve against the same JSON-LD context, extend the
/// same `Via` chain and pass through the same headers (clauses 6.3.18, 6.3.19),
/// are cacheable on the same terms (clause 6.3.17), and name the same
/// `EntityMap` (clause 5.5.14).
#[derive(Clone, Copy)]
pub(crate) struct ForwardedGetConditions<'a> {
    /// The JSON-LD context the triggering request resolved to.
    pub(crate) context: &'a ContextSpec,
    /// The request that triggered the forwarding.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// Whether the answers may be served from, and written to, the cache.
    pub(crate) cacheability: Cacheability,
    /// The `EntityMap` the read runs under, which fixes the location each
    /// source is told.
    pub(crate) entity_map: EntityMapInUse<'a>,
}

/// What one contacted registration answered, before the caller folds it.
pub(crate) struct SourceAnswer<T> {
    /// The registration's forwarding mode.
    pub(crate) mode: ForwardingMode,
    /// The error status the source answered with, when it answered with one.
    pub(crate) error_status: Option<u16>,
    /// The entry the answer was read into; `None` for a 404, which
    /// contributes nothing (clause 6.3.17).
    pub(crate) entry: Option<T>,
}

impl FederationService {
    /// Send each target the forwarded read it resolved to (a GET or a POST
    /// batch query), telling each the location of the map in use (clause
    /// 5.5.14), and read each answer with `read`. Both answer with the entities
    /// they carried, so the caller reads them the same way (clause 6.3.17).
    pub(crate) async fn fan_out_read<T>(
        &self,
        targets: Vec<(ContextSourceRegistration, ForwardedRead)>,
        read: impl Fn(&ContextSourceRegistration, ForwardGetOutcome) -> Option<T> + Sync,
        conditions: ForwardedGetConditions<'_>,
    ) -> Vec<SourceAnswer<T>> {
        let read = &read;
        let ForwardedGetConditions {
            context,
            triggering,
            cacheability,
            entity_map,
        } = conditions;
        stream::iter(targets)
            .map(|(registration, forwarded)| async move {
                let source = SourceRead::builder()
                    .registration(&registration)
                    .inbound(context)
                    .triggering(triggering)
                    .linked_map(entity_map.location_for(&registration.id))
                    .build();
                let outcome = match forwarded {
                    ForwardedRead::Get(url) => self.forward_get(source, url, cacheability).await,
                    ForwardedRead::PostQuery { url, body } => self.forward_post_query(source, url, &body).await,
                };
                let error_status = outcome.error_status();
                SourceAnswer {
                    mode: registration.access.mode.unwrap_or_default(),
                    error_status,
                    entry: read(&registration, outcome),
                }
            })
            .buffer_unordered(self.csr_concurrency)
            .collect()
            .await
    }
}

/// Append one fan-out target per forwarded request a registration's blocks
/// produce (clause 4.3.6.3). Each target carries the registration that reads its
/// answer back, so the registration is cloned for every request but the last,
/// which moves it; a registration with a single covering block (the common
/// case) is never cloned.
pub(crate) fn expand_registration_targets<Q>(registration: ContextSourceRegistration, requests: Vec<Q>, targets: &mut Vec<(ContextSourceRegistration, Q)>) {
    let mut requests = requests.into_iter();
    let Some(mut current) = requests.next() else {
        return;
    };
    loop {
        if let Some(next) = requests.next() {
            targets.push((registration.clone(), current)); // clone: one target per forwarded request in a multi-block fan-out
            current = next;
        } else {
            targets.push((registration, current));
            break;
        }
    }
}

/// Fold the answers of the contacted registrations into the outcome of the
/// read, `matched` of them exclusive, redirect or inclusive.
///
/// An auxiliary source only supplements the data the others hold (clause
/// 4.3.6.2), so it never makes an exclusive or redirect source share the
/// data, and its answers take no part in deciding whether one does.
pub(crate) fn read_outcome<T>(answers: Vec<SourceAnswer<T>>, matched: usize) -> ForwardReadOutcome<T> {
    let mut primary = answers.iter().filter(|answer| answer.mode != ForwardingMode::Auxiliary);
    let single_source_status = match (primary.next(), primary.next()) {
        // Table 6.4.3.2-3: a registered source alone holding the data answers
        // 501 when it cannot serve the representation, and that answer is
        // the operation's. An inclusive source declining is a clause 6.3.17
        // warning instead, because the rest of the data is still served.
        (Some(only), None) if matched == 1 && matches!(only.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect) => {
            only.error_status.filter(|status| *status == StatusCode::NOT_IMPLEMENTED.as_u16())
        }
        (None | Some(_), _) => None,
    };
    ForwardReadOutcome {
        results: answers.into_iter().filter_map(|answer| answer.entry).collect(),
        sources: MatchedSources {
            registrations: matched,
            single_source_status,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        outcome::ForwardedEntities,
        read_fan_out::{SourceAnswer, read_outcome},
    };
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;

    fn answer(mode: ForwardingMode, error_status: Option<u16>) -> SourceAnswer<ForwardedEntities> {
        SourceAnswer {
            mode,
            error_status,
            entry: None,
        }
    }

    /// Table 6.4.3.2-3: the 501 of the one exclusive source holding the data is
    /// the operation's answer; the same 501 from an inclusive source, or from
    /// one of several sources, is not.
    #[test]
    fn a_lone_proxied_source_declining_the_representation_surfaces_its_501() {
        let exclusive = read_outcome(vec![answer(ForwardingMode::Exclusive, Some(501))], 1);
        pretty_assertions::assert_eq!(exclusive.sources.single_source_status, Some(501));
        let redirect = read_outcome(vec![answer(ForwardingMode::Redirect, Some(501))], 1);
        pretty_assertions::assert_eq!(redirect.sources.single_source_status, Some(501));

        let inclusive = read_outcome(vec![answer(ForwardingMode::Inclusive, Some(501))], 1);
        pretty_assertions::assert_eq!(inclusive.sources.single_source_status, None);
        let forbidden = read_outcome(vec![answer(ForwardingMode::Exclusive, Some(403))], 1);
        pretty_assertions::assert_eq!(forbidden.sources.single_source_status, None);
        let several = read_outcome(vec![answer(ForwardingMode::Exclusive, Some(501)), answer(ForwardingMode::Inclusive, None)], 2);
        pretty_assertions::assert_eq!(several.sources.single_source_status, None);
        pretty_assertions::assert_eq!(several.sources.registrations, 2);
    }

    /// Clause 4.3.6.2: an auxiliary source answering beside the one exclusive
    /// source does not share its data, so the exclusive source's 501 is still
    /// the operation's answer.
    #[test]
    fn an_auxiliary_source_beside_a_lone_proxied_one_leaves_its_501_the_answer() {
        let outcome = read_outcome(vec![answer(ForwardingMode::Auxiliary, None), answer(ForwardingMode::Exclusive, Some(501))], 1);
        pretty_assertions::assert_eq!(outcome.sources.single_source_status, Some(501));
        let auxiliary_alone = read_outcome(vec![answer(ForwardingMode::Auxiliary, Some(501))], 0);
        pretty_assertions::assert_eq!(auxiliary_alone.sources.single_source_status, None);
    }
}
