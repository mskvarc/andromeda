//! A pattern holding letters outside ASCII selects the same entities at every page size.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, Input, NgsiLdMediaType},
    query::q,
};

/// The names the last sixty Stops cycle through, the Stop's number appended.
const NAMES: [&str; 6] = ["Čierna Voda", "Oravská", "Šafárikova", "Žilina LJE ŽP", "Nám. Ľudovíta Štúra", "Centrum"];

/// The name of Stop `n`: a filler for the first 240, which the first page's short walk reads past, then the six
/// names in turn.
fn name(n: usize) -> String {
    match n.checked_sub(241) {
        Some(position) => match NAMES.get(position % NAMES.len()) {
            Some(name) if name.ends_with("ŽP") => (*name).to_owned(),
            Some(name) => format!("{name} {n:04}"),
            None => format!("Filler {n:04}"),
        },
        None => format!("Filler {n:04}"),
    }
}

/// Pins: Query Entities with q name~=<pattern> over 300 Stops, the 60 named outside ASCII sorting after the first
/// 240, answers the same Stops in identifier order with the same total at limit 1, 50 and 1000, for a pattern holding
/// `Č`, `Š`, `Ž` or `Ľ` in an alternation of letters, an alternation of words, a literal, a bracket expression and
/// an anchored prefix
/// Clause: 4.9, 5.5.9
#[gtest]
#[tokio::test]
async fn n_qry_qlg_0061_a_pattern_holding_letters_outside_ascii_selects_the_same_entities_at_every_page_size() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let stop_type = namespace.entity_type("Stop")?;
    let entities = (1..=300)
        .map(|n| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Stop", format!("{n:04}"))?)
                .entity_type(stop_type.clone())
                .property("name", name(n).as_str())
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&entities).send().await?.done()?;
    // The names each pattern selects, by their position in NAMES; `Štúra` is a capital `Š` before a lower-case letter.
    for (pattern, selected) in [
        ("^(O|Č)", &[0, 1][..]),
        ("^(Šaf|Cen)", &[2, 5]),
        ("LJE ŽP$", &[3]),
        ("Nám\\. Ľ.*a", &[4]),
        ("[ČŠ][a-z]", &[0, 2, 4]),
        ("^Šaf", &[2]),
    ] {
        let expected_ids = (241..=300)
            .filter(|n| selected.contains(&((n - 241) % NAMES.len())))
            .map(|n| namespace.urn("Stop", format!("{n:04}")))
            .collect::<Result<Vec<_>, _>>()?;
        for limit in [1, 50, 1000] {
            let page = broker
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .entities()
                .query()
                .entity_type(&stop_type)
                .q(q::attr("name").matches(format!("\"{pattern}\"")))
                .limit(limit)
                .offset(0)
                .count(ResultsCount::Include)
                .send()
                .await?;
            expect_that!(page.total(), some(eq(u64::try_from(expected_ids.len())?)), "{pattern} at limit {limit}");
            let shown = usize::try_from(limit)?.min(expected_ids.len());
            expect_that!(
                page.items(),
                ids_in_order(expected_ids.get(..shown).unwrap_or_default()),
                "{pattern} at limit {limit}"
            );
        }
    }
    Ok(())
}
