use crate::Migration;
use crate::postgres::PostgresBackend;
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
        let backend = if url.starts_with("sqlite:") {
            let path = &url[7..]; // Skip "sqlite:"
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

    pub async fn initialize_database(
        &self,
        migrations: Vec<Migration>,
    ) -> Result<(), Box<dyn StdError + Send + Sync>> {
        if migrations.len() > 1 {
            unimplemented!("Actual migration process is not here yet");
        }

        let hashes = migrations
            .iter()
            .map(|m| {
                let mut h = DefaultHasher::new();
                m.hash(&mut h);
                h.finish()
            })
            .collect::<Vec<_>>();

        self.query("CREATE TABLE IF NOT EXISTS __migrations (id BIGINT PRIMARY KEY, hash BIGINT NOT NULL);").await?;

        let existing_migrations = self.query("SELECT * FROM __migrations;").await?;

        let existing_migrations = existing_migrations
            .rows()
            .iter()
            .map(|m| m.get_int("hash").unwrap() as u64)
            .collect::<Vec<_>>();

        for (idx, (e, m)) in existing_migrations.iter().zip(hashes.iter()).enumerate() {
            if e != m {
                panic!(
                    "Migration mismatched for index {}: {:?}",
                    idx, migrations[idx]
                );
            }
        }

        if existing_migrations.len() >= migrations.len() {
            // We have a newer version of database. Should be ok to run as-is.
            return Ok(());
        }

        let transaction = self.transaction().await?;

        for (idx, (m, hash)) in migrations
            .iter()
            .zip(hashes.iter())
            .enumerate()
            .skip(existing_migrations.len())
        {
            for t in m.get_tables() {
                // FIXME correctly handle error
                let sql = t.to_sql(&self.backend).unwrap();
                self.query(&sql).await?;
            }

            for r in m.get_raw_queries() {
                self.query(&r).await?;
            }

            self.query_with_params(
                "INSERT INTO __migrations (id, hash) VALUES(?, ?)",
                vec![Value::Integer(idx as i64), Value::Integer(*hash as i64)],
            )
            .await?;
        }

        transaction.commit().await
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
