use crate::executor::{instance_walk::WalkRefusal, source_instance_walk::SourceInstanceWalk};
use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    time::date_time::NgsiLdDateTime,
    time_prop::TimeProp,
};
use andromeda_ngsi_ld_model::{
    entity_system_times::EntitySystemTimes,
    temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
};
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use pretty_assertions::assert_eq;
use std::num::NonZeroU32;

const VEHICLE_1: &str = "urn:ngsi-ld:Vehicle:1";
const VEHICLE_2: &str = "urn:ngsi-ld:Vehicle:2";
const SOURCE_A: &str = "urn:ngsi-ld:ContextSourceRegistration:a";
const SOURCE_B: &str = "urn:ngsi-ld:ContextSourceRegistration:b";

fn at(day: u32) -> DateTime<Utc> {
    format!("2024-01-{day:02}T00:00:00Z").parse::<NgsiLdDateTime>().expect("a valid instant").get()
}

fn speed() -> AttributeName {
    "https://example.com/speed".parse().expect("a valid attribute name")
}

fn brand() -> AttributeName {
    "https://example.com/brand".parse().expect("a valid attribute name")
}

fn scope() -> AttributeName {
    properties::SCOPE.as_str().parse().expect("a valid attribute name")
}

fn instance(day: u32) -> InstanceDocument {
    let observed = format!("2024-01-{day:02}T00:00:00Z");
    InstanceDocument::builder()
        .family(Some(AttributeType::Property))
        .observed_at(Some(Slot::Set(observed.parse::<NgsiLdDateTime>().expect("an instant"))))
        .build()
}

/// One entity as a source answers it: the instances of each attribute,
/// observed on the given days of January, and one Scope instance.
fn entity(id: &str, attributes: &[(AttributeName, &[u32])]) -> TemporalHistory {
    let mut members: IndexMap<AttributeName, AttributeMember> = attributes
        .iter()
        .map(|(name, days)| (name.clone(), AttributeMember::Instances(days.iter().map(|day| instance(*day)).collect())))
        .collect();
    members.insert(scope(), AttributeMember::Instances(vec![instance(1)]));
    let document = EntityDocument::new(
        Some(id.parse().expect("an identifier")),
        vec!["https://example.com/Vehicle".parse().expect("a type")],
        None,
        None,
        members,
    );
    TemporalHistory::Instances(TemporalDocument::new(document, EntitySystemTimes::default(), IndexMap::new()))
}

/// What one registration answered on one page.
struct Answer {
    /// The registration answering.
    registration: Identifier,
    /// The entities it answered.
    entities: Vec<TemporalHistory>,
    /// The instant its partial answer was cut at.
    cut: Option<DateTime<Utc>>,
}

/// What `registration` answered: `entities`, cut on day `cut` when it is
/// partial.
fn answer(registration: &str, entities: Vec<TemporalHistory>, cut: Option<u32>) -> Answer {
    Answer {
        registration: registration.parse().expect("a valid URI"),
        entities,
        cut: cut.map(at),
    }
}

/// Trim every answer of one page, as the capture trims what the sources
/// answered.
fn trim(walk: &mut SourceInstanceWalk, answers: &mut [Answer]) -> Result<(), WalkRefusal> {
    answers
        .iter_mut()
        .try_for_each(|answer| walk.trim_answer(Some(&answer.registration), &mut answer.entities, answer.cut))
}

/// One kept entity: its identifier, the days of every attribute but the
/// Scope, and how many Scope instances.
type Kept = (String, Vec<(String, Vec<u32>)>, usize);

/// What each kept entity holds.
fn kept(answers: &[Answer]) -> Vec<Kept> {
    answers
        .iter()
        .flat_map(|answer| &answer.entities)
        .filter_map(|history| match history {
            TemporalHistory::Instances(document) => Some(document),
            TemporalHistory::Aggregates(_) => None,
        })
        .map(|document| {
            let attributes = document
                .entity()
                .attributes()
                .iter()
                .filter(|(name, member)| **name != scope() && !member.instances().is_empty())
                .map(|(name, member)| {
                    let days = member
                        .instances()
                        .iter()
                        .filter_map(|instance| instance.instant_on(TimeProp::Observed))
                        .filter_map(|instant| u32::try_from((instant - at(1)).num_days() + 1).ok())
                        .collect();
                    (name.to_string(), days)
                })
                .collect();
            let scopes = document.instances().filter(|(name, _)| **name == scope()).count();
            (document.id().map(ToString::to_string).unwrap_or_default(), attributes, scopes)
        })
        .collect()
}

fn walk() -> SourceInstanceWalk {
    SourceInstanceWalk::new(TimeInterval::After(at(1)), TimeProp::Observed, None)
}

/// Clauses 5.16.1.4 and 6.3.10: a source answer cut twice is read to the end
/// of the window in resumed passes, each opening at the previous cut, and
/// every instance and the Scope are kept once.
#[test]
fn a_source_answer_cut_twice_is_read_to_the_end_in_resumed_passes() {
    let mut walk = walk();
    let passes = [
        (vec![1, 2], Some(2), Some(TimeInterval::After(at(2)))),
        (vec![2, 3], Some(3), Some(TimeInterval::After(at(3)))),
        (vec![3, 4], None, None),
    ];
    let mut stored = Vec::new();
    for (days, cut, next) in passes {
        let mut answers = vec![answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &days)])], cut)];
        trim(&mut walk, &mut answers).expect("a forward answer");
        stored.extend(kept(&answers));
        assert_eq!(walk.next_pass(), Ok(next));
    }
    let speed = speed().to_string();
    assert_eq!(
        stored,
        vec![
            (VEHICLE_1.to_owned(), vec![(speed.clone(), vec![1, 2])], 1),
            (VEHICLE_1.to_owned(), vec![(speed.clone(), vec![3])], 0),
            (VEHICLE_1.to_owned(), vec![(speed, vec![4])], 0),
        ]
    );
}

/// An entity of the source's page that a partial answer left out, its every
/// instance past the cut, is kept whole on the resumed pass, its Scope with it.
#[test]
fn an_entity_a_partial_answer_left_out_is_kept_whole_on_the_resumed_pass() {
    let mut walk = walk();
    let mut first = vec![answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[1])])], Some(1))];
    trim(&mut walk, &mut first).expect("a forward answer");
    assert_eq!(walk.next_pass(), Ok(Some(TimeInterval::After(at(1)))));
    let mut resumed = vec![answer(
        SOURCE_A,
        vec![entity(VEHICLE_1, &[(speed(), &[1, 2])]), entity(VEHICLE_2, &[(speed(), &[3])])],
        None,
    )];
    trim(&mut walk, &mut resumed).expect("a forward answer");
    let speed = speed().to_string();
    assert_eq!(
        kept(&resumed),
        vec![
            (VEHICLE_1.to_owned(), vec![(speed.clone(), vec![2])], 0),
            (VEHICLE_2.to_owned(), vec![(speed, vec![3])], 1),
        ]
    );
}

/// The resumed pass opens at the least cut of every source, and what each
/// registration kept of one entity is its own: the source cut later keeps
/// only what lies past its own cut.
#[test]
fn a_resumed_pass_opens_at_the_least_cut_and_keeps_what_each_source_already_answered() {
    let mut walk = walk();
    let mut first = vec![
        answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[1, 2, 3])])], Some(3)),
        answer(SOURCE_B, vec![entity(VEHICLE_1, &[(brand(), &[1])])], Some(1)),
    ];
    trim(&mut walk, &mut first).expect("forward answers");
    assert_eq!(walk.next_pass(), Ok(Some(TimeInterval::After(at(1)))));
    let mut resumed = vec![
        answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[1, 2, 3, 4])])], None),
        answer(SOURCE_B, vec![entity(VEHICLE_1, &[(brand(), &[1, 2])])], None),
    ];
    trim(&mut walk, &mut resumed).expect("forward answers");
    assert_eq!(
        kept(&resumed),
        vec![
            (VEHICLE_1.to_owned(), vec![(speed().to_string(), vec![4])], 0),
            (VEHICLE_1.to_owned(), vec![(brand().to_string(), vec![2])], 0),
        ]
    );
    assert_eq!(walk.next_pass(), Ok(None));
}

/// An entity a complete answer held is not kept again when a resumed pass,
/// opened for another source's cut, answers it again.
#[test]
fn an_entity_a_complete_answer_held_is_not_kept_again_on_a_resumed_pass() {
    let mut walk = walk();
    let mut first = vec![
        answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[1, 2])])], None),
        answer(SOURCE_B, vec![entity(VEHICLE_2, &[(speed(), &[1])])], Some(1)),
    ];
    trim(&mut walk, &mut first).expect("forward answers");
    assert_eq!(walk.next_pass(), Ok(Some(TimeInterval::After(at(1)))));
    let mut resumed = vec![
        answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[1, 2])])], None),
        answer(SOURCE_B, vec![entity(VEHICLE_2, &[(speed(), &[1, 2])])], None),
    ];
    trim(&mut walk, &mut resumed).expect("forward answers");
    assert_eq!(kept(&resumed), vec![(VEHICLE_2.to_owned(), vec![(speed().to_string(), vec![2])], 0)]);
}

/// A `between` window resumes up to its closing instant.
#[test]
fn a_between_window_resumes_up_to_its_closing_instant() {
    let mut walk = SourceInstanceWalk::new(TimeInterval::Between { from: at(1), to: at(20) }, TimeProp::Observed, None);
    let mut answers = vec![answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[1, 5])])], Some(5))];
    trim(&mut walk, &mut answers).expect("a forward answer");
    assert_eq!(walk.next_pass(), Ok(Some(TimeInterval::Between { from: at(5), to: at(20) })));
}

/// A pass cut where the previous one was cut cannot be read past.
#[test]
fn a_pass_cut_no_later_than_the_previous_one_stalls_the_walk() {
    let mut walk = walk();
    let mut first = vec![answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[2])])], Some(2))];
    trim(&mut walk, &mut first).expect("a forward answer");
    assert_eq!(walk.next_pass(), Ok(Some(TimeInterval::After(at(2)))));
    let mut resumed = vec![answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[2])])], Some(2))];
    trim(&mut walk, &mut resumed).expect("a forward answer");
    assert_eq!(walk.next_pass(), Err(WalkRefusal::Stalled { at: at(2) }));
}

/// Clause 6.3.10 pages a partial answer to a `lastN` query backwards, which
/// the capture refuses rather than keep its most recent page alone.
#[test]
fn a_partial_source_answer_to_a_last_n_query_is_refused() {
    let last_n = NonZeroU32::new(5000).expect("a positive lastN");
    let mut walk = SourceInstanceWalk::new(TimeInterval::After(at(1)), TimeProp::Observed, Some(last_n));
    let mut complete = vec![answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[1])])], None)];
    assert_eq!(trim(&mut walk, &mut complete), Ok(()));
    let mut partial = vec![answer(SOURCE_A, vec![entity(VEHICLE_1, &[(speed(), &[1])])], Some(1))];
    assert_eq!(trim(&mut walk, &mut partial), Err(WalkRefusal::Backward));
}
