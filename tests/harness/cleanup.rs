use sqlx::PgPool;
use url::Url;

const TRUNCATE_ALL: &str = "\
    TRUNCATE \
        entities, \
        jsonld_contexts, \
        temporal_entity_attributes, \
        attribute_history, \
        subscriptions, \
        context_source_registrations, \
        csource_subscriptions, \
        snapshots \
    CASCADE";

pub async fn clean_database(db_url: &str) {
    let pool = PgPool::connect(db_url).await.expect("failed to connect to database for cleanup");

    sqlx::query(TRUNCATE_ALL).execute(&pool).await.expect("failed to truncate tables");
    clean_named_tenant_databases(db_url, &pool).await;

    pool.close().await;
}

async fn clean_named_tenant_databases(main_db_url: &str, main_pool: &PgPool) {
    let has_tenants_table: bool = sqlx::query_scalar("SELECT to_regclass('public.tenants') IS NOT NULL")
        .fetch_one(main_pool)
        .await
        .unwrap_or(false);
    if !has_tenants_table {
        return;
    }

    let tenant_dbs: Vec<String> = sqlx::query_scalar("SELECT db_name FROM tenants WHERE is_unnamed_default = false")
        .fetch_all(main_pool)
        .await
        .unwrap_or_default();

    for db_name in tenant_dbs {
        let Ok(tenant_url) = replace_database_name(main_db_url, &db_name) else {
            continue;
        };
        let Ok(tenant_pool) = PgPool::connect(&tenant_url).await else {
            continue;
        };
        let _ = sqlx::query(TRUNCATE_ALL).execute(&tenant_pool).await;
        tenant_pool.close().await;
    }

    let _ = sqlx::query("DELETE FROM tenants WHERE is_unnamed_default = false").execute(main_pool).await;
}

fn replace_database_name(base_url: &str, db_name: &str) -> Result<String, url::ParseError> {
    let mut parsed = Url::parse(base_url)?;
    parsed.set_path(&format!("/{db_name}"));
    Ok(parsed.to_string())
}
