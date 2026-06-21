use crate::postgres::PostgresBackend;
use crate::{Migration, SchemaSet};
use crate::query::{QueryResult, Transaction, Value};
use crate::sql_builder::SQLBuilder;
use crate::sqlite::SqliteBackend;
use std::error::Error as StdError;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

#[derive(Clone)]
pub struct ConnectionPool {
    backend: Arc<Backend>,
}

#[derive(Clone)]
pub enum Backend {
    Postgres(PostgresBackend),
    Sqlite(SqliteBackend),
}

impl ConnectionPool {
    pub fn new(url: &str) -> Result<Self, Box<dyn StdError + Send + Sync>> {
        let backend = if let Some(path) = url.strip_prefix("sqlite:") {
            let sqlite_backend = SqliteBackend::new(path)?;
            Backend::Sqlite(sqlite_backend)
        } else if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            Backend::Postgres(PostgresBackend::new(url)?)
        } else {
            return Err(format!("Unsupported database URL format: {}", url).into());
        };

        Ok(ConnectionPool {
            backend: Arc::new(backend),
        })
    }

    /// Apply a single unnamed migration history. Equivalent to building a
    /// [`Migrator`] with one default-namespace [`SchemaSet`].
    pub async fn initialize_database(
        &self,
        migrations: Vec<Migration>,
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {
        self.migrator()
            .apply(SchemaSet::new("", migrations))
            .run()
            .await
    }

    /// Start composing several named schema fragments onto this pool.
    pub fn migrator(&self) -> Migrator<'_> {
        Migrator {
            pool: self,
            sets: Vec::new(),
        }
    }

    /// Create the ledger if missing and upgrade legacy `(id, hash)` ledgers to
    /// the namespaced `(namespace, id, hash)` layout. Runs in autocommit so the
    /// column probe can fail harmlessly on Postgres.
    async fn ensure_migrations_table(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        self.query(
            "CREATE TABLE IF NOT EXISTS __migrations (namespace TEXT NOT NULL DEFAULT '', \
             id BIGINT NOT NULL, hash BIGINT NOT NULL, PRIMARY KEY (namespace, id));",
        )
        .await?;

        let needs_upgrade = self
            .query("SELECT namespace FROM __migrations LIMIT 1;")
            .await
            .is_err();

        if needs_upgrade {
            // Rebuild rather than ADD COLUMN so the composite (namespace, id)
            // primary key is in place; otherwise a second namespace's id=0 would
            // collide with the default namespace's id=0.
            let transaction = self.transaction().await?;
            self.query("ALTER TABLE __migrations RENAME TO __migrations_legacy;")
                .await?;
            self.query(
                "CREATE TABLE __migrations (namespace TEXT NOT NULL DEFAULT '', \
                 id BIGINT NOT NULL, hash BIGINT NOT NULL, PRIMARY KEY (namespace, id));",
            )
            .await?;
            self.query(
                "INSERT INTO __migrations (namespace, id, hash) \
                 SELECT '', id, hash FROM __migrations_legacy;",
            )
            .await?;
            self.query("DROP TABLE __migrations_legacy;").await?;
            transaction.commit().await?;
        }

        Ok(())
    }

    /// Apply the pending migrations of one namespace. Assumes a transaction is
    /// already active and that the ledger table exists.
    async fn apply_pending(
        &self,
        namespace: &str,
        migrations: &[Migration],
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let hashes = migrations.iter().map(migration_hash).collect::<Vec<_>>();

        let existing = self
            .query_with_params(
                "SELECT hash FROM __migrations WHERE namespace = ? ORDER BY id;",
                vec![Value::Text(namespace.to_string())],
            )
            .await?;
        let existing = existing
            .rows()
            .iter()
            .map(|m| m.get_int("hash").unwrap() as u64)
            .collect::<Vec<_>>();

        for (idx, (e, m)) in existing.iter().zip(hashes.iter()).enumerate() {
            if e != m {
                panic!(
                    "Migration mismatched for namespace {:?} index {}: {:?}",
                    namespace, idx, migrations[idx]
                );
            }
        }

        if existing.len() >= migrations.len() {
            // The database already has this many (or more) migrations applied.
            return Ok(());
        }

        for (idx, (m, hash)) in migrations
            .iter()
            .zip(hashes.iter())
            .enumerate()
            .skip(existing.len())
        {
            for t in m.get_tables() {
                // FIXME correctly handle error
                let sql = t.to_sql(&self.backend).unwrap();
                self.query(&sql).await?;
            }

            for a in m.get_alters() {
                // FIXME correctly handle error
                for sql in a.to_sql(&self.backend).unwrap() {
                    self.query(&sql).await?;
                }
            }

            for r in m.get_raw_queries() {
                self.query(r).await?;
            }

            self.query_with_params(
                "INSERT INTO __migrations (namespace, id, hash) VALUES(?, ?, ?)",
                vec![
                    Value::Text(namespace.to_string()),
                    Value::Integer(idx as i64),
                    Value::Integer(*hash as i64),
                ],
            )
            .await?;
        }

        Ok(())
    }

    /// Execute a raw SQL query asynchronously
    pub async fn query(&self, query: &str) -> Result<QueryResult, Box<dyn StdError + Send + Sync>> {
        match &*self.backend {
            Backend::Postgres(postgres) => postgres.query(query).await,
            Backend::Sqlite(sqlite) => sqlite.query(query).await,
        }
    }

    /// Execute a prepared statement with parameters asynchronously
    pub async fn query_with_params(
        &self,
        query: &str,
        params: Vec<Value>,
    ) -> Result<QueryResult, Box<dyn StdError + Send + Sync>> {
        match &*self.backend {
            Backend::Postgres(postgres) => postgres.query_with_params(query, params).await,
            Backend::Sqlite(sqlite) => sqlite.query_with_params(query, params).await,
        }
    }

    pub async fn transaction(&self) -> Result<Transaction, Box<dyn StdError + Send + Sync>> {
        match &*self.backend {
            Backend::Postgres(postgres) => postgres
                .begin_transaction()
                .await
                .map(|_| Transaction::new(self.backend.clone())),
            Backend::Sqlite(sqlite) => sqlite
                .begin_transaction()
                .await
                .map(|_| Transaction::new(self.backend.clone())),
        }
    }
}

impl Backend {
    pub(crate) fn builder(&self) -> SQLBuilder {
        match self {
            Backend::Postgres(postgres) => postgres.builder(),
            Backend::Sqlite(sqlite) => sqlite.builder(),
        }
    }
}

fn migration_hash(m: &Migration) -> u64 {
    let mut h = DefaultHasher::new();
    m.hash(&mut h);
    h.finish()
}

/// Composes several named [`SchemaSet`]s onto a single pool. Each namespace
/// keeps its own append-only history in the `__migrations` ledger, so fragments
/// defined in different crates can share one database without their migration
/// indices colliding.
///
/// Apply order matters: list a fragment before any fragment that references its
/// tables via foreign keys.
pub struct Migrator<'a> {
    pool: &'a ConnectionPool,
    sets: Vec<SchemaSet>,
}

impl<'a> Migrator<'a> {
    pub fn apply(mut self, set: SchemaSet) -> Self {
        self.sets.push(set);
        self
    }

    pub async fn run(self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        self.pool.ensure_migrations_table().await?;

        let transaction = self.pool.transaction().await?;
        for set in &self.sets {
            self.pool.apply_pending(set.name(), set.migrations()).await?;
        }
        transaction.commit().await
    }
}
