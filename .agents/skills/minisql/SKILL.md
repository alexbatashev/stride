---
name: minisql
description: Use Friday's MiniSQL library and `migrations!` macro to add or update SQLite schema, generated table modules, typed inserts, and typed selects. Use when working in this repo on code that defines `migrations!`, calls `get_migrations()`, or uses generated APIs like `table::insert()`, `table::select()`, and `table::select_cols(...)`.
---

# MiniSQL

## Overview

Prefer the `migrations!` macro path over manual `Migration` or `Table` builders. Keep changes small, match existing table-module patterns, and verify against the real limits of this implementation instead of assuming ORM-like features exist.

## Workflow

1. Find the existing `migrations!` block and generated table-module usage before editing anything.
2. Change schema in the macro first.
3. Update call sites to use the generated module API instead of hand-written SQL when the query is a simple single-table insert or select.
4. Keep `get_migrations()` output to one migration unless you also change runtime migration handling.
5. Run focused Rust tests after changes.

## Macro Rules

- Use `migrations!` as the source of truth for schema and generated table modules.
- Define columns as `field: Type` with optional tags or attributes in brackets.
- Use `[PrimaryKey]`, `[Unique]`, and `[column = "actual_db_name"]`.
- Use `foreign_key(local_col -> other_table.other_col);` for foreign keys.
- Use `enable_vectors;` only when the table actually contains vector columns.
- Use `raw "...";` for indexes, views, or SQL that the builder cannot express.
- Keep field identifiers stable because they become Rust API names in the generated module.

## Query Patterns

- Use `table::insert()` for normal inserts.
- Use `table::select()` for `SELECT *` single-table reads.
- Use `table::select_cols((...))` when only a few columns are needed.
- Use `table::update()` with per-column setters for single-table updates.
- Use `table::delete()` for single-table deletes.
- Build predicates with generated column consts like `users::email.eq("x")` and combine them with `.and(...)`, `.or(...)`, `.not()`.
- `update()` and `delete()` require a `.where_(...)` predicate; call `.all()` to intentionally affect every row.
- Use raw SQL with `query_with_params` when the query needs joins, aggregates, or anything beyond the single-table DSL.

## Sharp Edges

- `ConnectionPool` only supports SQLite URLs. In tests, use `sqlite::memory:`.
- `initialize_database` currently panics for more than one migration in the vector. The macro can define multiple migrations, but the runtime does not apply them yet.
- The generated query DSL is single-table only.
- `select_cols` tuple decoding only exists for 1 to 4 columns.
- `InsertBuilder::execute()` returns the query result's `affected_rows()`, which is not a reliable inserted-row count for SQLite writes in this implementation.
- `SelectQuery::one()` and `SelectColsQuery::one()` use `LIMIT 1`; they error on zero rows and silently ignore additional matches.

## Verification

- Run targeted tests in the affected crate first.
- If you touched the MiniSQL crates themselves, run `cargo test -p minisql`.
- If you changed generated module usage in another crate, run that crate's focused tests too.

## Reference

Read [references/generated-api.md](references/generated-api.md) when you need exact macro syntax, generated module members, or concrete examples.
