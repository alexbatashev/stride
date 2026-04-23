# MiniSQL Generated API

Use this file when you need exact `migrations!` syntax or the shape of the generated Rust API.

## Schema Macro

```rust
use minisql::{Migration, migrations};
use uuid::Uuid;

migrations! {
    schema {
        table users {
            id: Uuid [PrimaryKey],
            email: String [Unique],
            display_name: Option<String>,
            kind: String [column = "type"],
        }

        table sessions {
            id: Uuid [PrimaryKey],
            user_id: Uuid,
            created_at: String,

            foreign_key(user_id -> users.id);
        }

        raw "CREATE INDEX idx_sessions_user_id ON sessions(user_id);";
    }
}

let migrations: Vec<Migration> = get_migrations();
```

Notes:
- `get_migrations()` is generated next to the macro call.
- Each `table foo { ... }` also generates a `foo` Rust module.
- Keep the macro near code that uses the generated modules so the API is obvious.

## Generated Table Module

For `table users { ... }`, the macro generates:

- `users::Table`: marker type for the DSL
- `users::Row`: typed row with one field per declared column
- `users::id`, `users::email`, ...: typed `Column<_, users::Table>` consts
- `users::insert() -> InsertBuilder`
- `users::select() -> SelectQuery`
- `users::select_cols((...)) -> SelectColsQuery<_>`

If a column uses `[column = "type"]`, Rust code still uses the Rust field name while SQL uses the overridden DB column name.

## Insert Pattern

```rust
let id = Uuid::new_v4();

users::insert()
    .id(id)
    .email("a@example.com")
    .display_name(Option::<&str>::None)
    .execute(&db)
    .await?;
```

Notes:
- Setter names match the Rust field names from the macro.
- `&str` works for `String`.
- `Option<&str>` works for `Option<String>`.
- UUID values round-trip through MiniSQL's `Value::Uuid`.

## Select Pattern

```rust
let rows = users::select()
    .where_(users::email.eq("a@example.com"))
    .limit(10)
    .all(&db)
    .await?;
```

Useful predicate builders:
- `.eq(...)`, `.ne(...)`, `.gt(...)`, `.ge(...)`, `.lt(...)`, `.le(...)`
- `.is_null()`, `.is_not_null()`
- `.and(...)`, `.or(...)`, `.not()`

Ordering:

```rust
let rows = users::select()
    .order_by_asc(users::email)
    .all(&db)
    .await?;
```

`users::select()` returns typed `users::Row` values.

## Projected Select Pattern

```rust
let rows = users::select_cols((users::id, users::email))
    .where_(users::display_name.is_not_null())
    .order_by_desc(users::email)
    .all(&db)
    .await?;
```

This returns tuples, not structs:

```rust
let first: (uuid::Uuid, String) = rows[0].clone();
```

Current limit:
- tuple projections are implemented only for 1, 2, 3, or 4 columns

## When to Drop to Raw SQL

Use `db.query_with_params(...)` instead of the DSL for:

- joins
- updates
- deletes
- aggregates
- custom SQL functions
- anything that is not a single-table `SELECT` or generated `INSERT`

## Current Runtime Limits

- `ConnectionPool::new(...)` only recognizes `sqlite:` URLs.
- `initialize_database(...)` currently rejects more than one migration in the vector.
- `SelectQuery::one()` and projection `.one()` are convenience wrappers over `LIMIT 1`.
- `InsertBuilder::execute()` should be treated as fire-and-forget success reporting, not a trusted inserted-row count.
