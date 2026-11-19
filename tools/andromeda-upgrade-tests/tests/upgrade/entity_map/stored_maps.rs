//! Every Entity Map v1.0.0 stored is removed by the upgrade, its entries and
//! links with it: such a map records only a digest of its filters as written,
//! which compares with no follow-up request (ETSI GS CIM 009 v1.9.1 clause
//! 5.5.14), so a request naming it is answered as one naming an expired map.

use crate::fixtures::{
    rehearsal::at_v1_0_0,
    statements::{count_rows, insert, insert_returning, instant},
    test_error::UpgradeTestError,
};
use pretty_assertions::assert_eq;
use sea_query::Expr;

#[tokio::test]
async fn every_entity_map_v1_0_0_stored_is_removed_with_its_entries_and_links() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let map_key: i64 = insert_returning(
        database.pool(),
        "entity_maps",
        "entity_map_key",
        vec![
            ("expires_at", instant("2099-01-01T00:00:00Z")),
            ("id", Expr::val("urn:ngsi-ld:EntityMap:stored")),
            ("request_fingerprint", Expr::val("query:00ff")),
        ],
    )
    .await?;
    insert(
        database.pool(),
        "entity_map_entries",
        vec![
            ("entity_map_key", Expr::val(map_key)),
            ("position", Expr::val(0_i32)),
            ("entity_id", Expr::val("urn:ngsi-ld:Sensor:1")),
            ("source", Expr::val("@none")),
        ],
    )
    .await?;
    insert(
        database.pool(),
        "entity_map_links",
        vec![
            ("entity_map_key", Expr::val(map_key)),
            ("registration_id", Expr::val("urn:ngsi-ld:ContextSourceRegistration:1")),
            ("linked_map_id", Expr::val("urn:ngsi-ld:EntityMap:remote")),
        ],
    )
    .await?;

    database.migrate_to_current().await?;

    for table in ["entity_maps", "entity_map_entries", "entity_map_links"] {
        assert_eq!(count_rows(database.pool(), table).await?, 0, "{table}");
    }
    database.drop_database().await?;
    Ok(())
}
