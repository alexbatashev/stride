# minisql

Async SQL query builder, migration system, and `sqlite-vec`-backed vector
search over SQLite (default) and Postgres (`postgres` feature). Ships with
a companion `migrations!` proc-macro
([`minisql-macros`](../minisql-macros)) for declaring type-safe schemas.

Part of [Stride](https://github.com/alexbatashev/stride).

`0.x` — the query builder surface is still evolving; expect breaking changes
between minor versions.

## Usage

```rust
use minisql::Value;
use minisql::{ConnectionPool, Migration, SqlTag, Table};

let pool = ConnectionPool::new("sqlite::memory:").expect("create pool");

let migration = Migration::new().table(
    Table::from_name("users")
        .add::<i64, _>("id", &[SqlTag::PrimaryKey])
        .add::<String, _>("name", &[])
        .add::<i64, _>("age", &[]),
);

pool.initialize_database(vec![migration]).await.expect("migrate");

pool.query_with_params(
    "INSERT INTO users (id, name, age) VALUES (?, ?, ?)",
    vec![
        Value::Integer(1),
        Value::Text("Alice".to_string()),
        Value::Integer(30),
    ],
)
.await
.expect("insert row");

let res = pool
    .query_with_params("SELECT id, name, age FROM users WHERE id = ?", vec![Value::Integer(1)])
    .await
    .expect("select row");

assert_eq!(res.row_count(), 1);
```

See [`minisql-macros`](../minisql-macros/README.md) for the declarative
`migrations!` macro, which generates typed table/column accessors and a
query builder from a schema definition.
