//! Live Postgres smoke test. Skipped unless STRIDE_PG_TEST_URL points at a
//! reachable Postgres instance, e.g.
//! `STRIDE_PG_TEST_URL=postgres://postgres:postgres@localhost:5432/postgres`.

use minisql::{ConnectionPool, Migration, SqlTag, Table, Value};
use uuid::Uuid;

fn pg_url() -> Option<String> {
    std::env::var("STRIDE_PG_TEST_URL")
        .ok()
        .filter(|u| !u.is_empty())
}

#[tokio::test]
async fn postgres_migrate_insert_select() {
    let Some(url) = pg_url() else {
        eprintln!("skipping: STRIDE_PG_TEST_URL not set");
        return;
    };

    let pool = ConnectionPool::new(&url).expect("create pool");

    pool.query("DROP TABLE IF EXISTS mini_threads;")
        .await
        .expect("drop");
    // Reset the ledger so the migration re-runs against the freshly dropped table.
    pool.query("DROP TABLE IF EXISTS __migrations;")
        .await
        .expect("drop ledger");

    let migration = Migration::new().table(
        Table::from_name("mini_threads")
            .add::<Uuid, _>("id", &[SqlTag::PrimaryKey])
            .add::<Uuid, _>("owner", &[])
            .add::<String, _>("title", &[]),
    );

    pool.migrator()
        .apply(minisql::SchemaSet::new("pgsmoke", vec![migration]))
        .run()
        .await
        .expect("migrate");

    let id = Uuid::now_v7();
    let owner = Uuid::now_v7();

    // Mirror the DSL-generated INSERT exactly (trailing semicolon included).
    pool.query_with_params(
        "INSERT INTO mini_threads (id, owner, title) VALUES (?, ?, ?);",
        vec![
            Value::Uuid(id),
            Value::Uuid(owner),
            Value::Text("New thread".to_string()),
        ],
    )
    .await
    .expect("insert row");

    let res = pool
        .query_with_params(
            "SELECT id FROM mini_threads WHERE ((id = ?) AND (owner = ?))",
            vec![Value::Uuid(id), Value::Uuid(owner)],
        )
        .await
        .expect("select row");

    assert_eq!(res.row_count(), 1);
}

#[tokio::test]
async fn postgres_insert_null_into_text_column() {
    let Some(url) = pg_url() else {
        eprintln!("skipping: STRIDE_PG_TEST_URL not set");
        return;
    };

    let pool = ConnectionPool::new(&url).expect("create pool");
    pool.query("DROP TABLE IF EXISTS mini_msgs;")
        .await
        .expect("drop");
    pool.query("CREATE TABLE mini_msgs (id UUID PRIMARY KEY, thinking TEXT);")
        .await
        .expect("create");

    // Mirrors a message insert where the optional `thinking` column is NULL.
    pool.query_with_params(
        "INSERT INTO mini_msgs (id, thinking) VALUES (?, ?);",
        vec![Value::Uuid(Uuid::now_v7()), Value::Null],
    )
    .await
    .expect("insert null into text column");
}
