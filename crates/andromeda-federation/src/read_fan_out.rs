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
use andromeda_fan_out::bounded::fan_out_unordered;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    query::body::Query,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
};
use futures::StreamExt;
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
///
/// A fan-out yields exactly one answer for every forwarded read it sends, a
/// read the source failed or answered with 404 included, so the answers of a
/// fan-out count the reads each registration was sent.
pub(crate) struct SourceAnswer<T> {
    /// The registration whose source was asked.
    pub(crate) registration: Identifier,
    /// The registration's forwarding mode.
    pub(crate) mode: ForwardingMode,
    /// The error status the source answered with, when it answered with one.
    pub(crate) error_status: Option<u16>,
    /// The entry the answer was read into; `None` for a 404, which
    /// contributes nothing (clause 6.3.17).
    pub(crate) entry: Option<T>,
}

/// An answer that may name the `EntityMap` its source used for a read or
/// created for a creation (Table 5.2.39-2 `linkedMaps`).
pub(crate) trait LinkedAnswer {
    /// Forget the map the source answered with.
    fn unlink(&mut self);
}

impl FederationService {
    /// Send each target the forwarded read it resolved to (a GET or a POST
    /// batch query), telling each the location of the map in use (clause
    /// 5.5.14), and read each answer with `read`. Both answer with the entities
    /// they carried, so the caller reads them the same way (clause 6.3.17).
    pub(crate) async fn fan_out_read<T: Send>(
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
        fan_out_unordered(targets, self.csr_concurrency, |(registration, forwarded)| async move {
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
            let entry = read(&registration, outcome);
            SourceAnswer {
                mode: registration.access.mode.unwrap_or_default(),
                error_status,
                entry,
                registration: registration.id,
            }
        })
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
/// data, and its answers take no part in deciding whether one does. Only a
/// registration sent a single read keeps the map its source answered with, as
/// [`link_sole_reads`] describes.
pub(crate) fn read_outcome<T: LinkedAnswer>(mut answers: Vec<SourceAnswer<T>>, matched: usize) -> ForwardReadOutcome<T> {
    link_sole_reads(&mut answers);
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

/// Keep linked only the maps of registrations sent a single read.
///
/// Table 5.2.39-2 links one `EntityMap` to a registration, and a map pins the
/// request that created it (clause 5.5.14). A registration sent one read per
/// covering information block, or per run of identifiers that fits a URL,
/// answers each with a map of its own that pins none of the other reads, so it
/// is linked to none of them, even when only one of its reads named a map. Its
/// entities are still attributed to it.
fn link_sole_reads<T: LinkedAnswer>(answers: &mut [SourceAnswer<T>]) {
    let repeated: Vec<bool> = answers
        .iter()
        .map(|answer| answers.iter().filter(|other| other.registration == answer.registration).nth(1).is_some())
        .collect();
    for (answer, repeated) in answers.iter_mut().zip(repeated) {
        if repeated && let Some(entry) = answer.entry.as_mut() {
            entry.unlink();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::read_fan_out::{LinkedAnswer, SourceAnswer, read_outcome};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;

    /// An answer naming only the map its source answered with.
    #[derive(Debug, PartialEq, Eq)]
    struct Linked(Option<&'static str>);

    impl LinkedAnswer for Linked {
        fn unlink(&mut self) {
            self.0 = None;
        }
    }

    fn registration(name: &str) -> Identifier {
        format!("urn:ngsi-ld:ContextSourceRegistration:{name}")
            .parse::<Identifier>()
            .expect("a valid registration identifier")
    }

    fn answer(mode: ForwardingMode, error_status: Option<u16>) -> SourceAnswer<Linked> {
        SourceAnswer {
            registration: registration("only"),
            mode,
            error_status,
            entry: None,
        }
    }

    /// The answer of one read sent the registration `name`, naming `map`.
    fn read_of(name: &str, map: Option<&'static str>) -> SourceAnswer<Linked> {
        SourceAnswer {
            registration: registration(name),
            mode: ForwardingMode::Inclusive,
            error_status: None,
            entry: Some(Linked(map)),
        }
    }

    /// The answer of one read sent the registration `name` that the source
    /// answered with 404, which leaves no entry.
    fn not_found(name: &str) -> SourceAnswer<Linked> {
        SourceAnswer {
            registration: registration(name),
            mode: ForwardingMode::Inclusive,
            error_status: None,
            entry: None,
        }
    }

    /// Table 5.2.39-2 links one map to a registration and clause 5.5.14 pins a
    /// map to the request that created it: a registration sent two reads keeps
    /// neither map, and so does one whose other read named none or was
    /// answered with 404, while a registration sent one read keeps its own.
    #[test]
    fn a_registration_sent_several_reads_keeps_none_of_the_maps_they_answered_with() {
        let outcome = read_outcome(
            vec![
                read_of("blocks", Some("urn:ngsi-ld:EntityMap:vehicles")),
                read_of("sole", Some("urn:ngsi-ld:EntityMap:sole")),
                read_of("blocks", Some("urn:ngsi-ld:EntityMap:buildings")),
                read_of("unmapped", None),
                read_of("unmapped", Some("urn:ngsi-ld:EntityMap:unmapped")),
                not_found("missing"),
                read_of("missing", Some("urn:ngsi-ld:EntityMap:missing")),
            ],
            4,
        );
        pretty_assertions::assert_eq!(
            outcome.results,
            vec![
                Linked(None),
                Linked(Some("urn:ngsi-ld:EntityMap:sole")),
                Linked(None),
                Linked(None),
                Linked(None),
                Linked(None),
            ]
        );
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
