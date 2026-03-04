use std::error::Error as StdError;
use std::sync::Arc;
use std::{collections::HashMap, ops::Deref};

use crate::migration::{Command, SqlType};
use crate::query::{QueryResult, Row, Value};
use crate::sql_builder::SQLBuilder;
use ::sqlite::{self, Connection, State, Value as SqliteValue};
use parking_lot::Mutex;

#[derive(Clone)]
pub struct SqliteBackend {
    connection: Arc<Mutex<InnerConnection>>,
}

struct InnerConnection(Connection);

pub struct SqliteBuilder;

unsafe impl Send for InnerConnection {}
unsafe impl Sync for InnerConnection {}

impl Deref for InnerConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl SqliteBackend {
    pub fn new(path: &str) -> Result<Self, Box<dyn StdError + Send + Sync>> {
        unsafe {
            sqlite::ffi::sqlite3_auto_extension(Some(sqlite_vec::sqlite3_vec_init));
        }
        let connection = Connection::open(path)?;

        Ok(Self {
            connection: Arc::new(Mutex::new(InnerConnection(connection))),
        })
    }

    /// Execute a query synchronously
    fn execute_query(
        &self,
        query: &str,
        params: &[Value],
    ) -> Result<QueryResult, Box<dyn StdError + Send + Sync>> {
        let conn = self.connection.lock();

        // Prepare statement
        let mut statement = conn.prepare(query)?;

        // Bind parameters
        for (idx, param) in params.iter().enumerate() {
            match param {
                Value::Null => statement.bind((idx + 1, SqliteValue::Null))?,
                Value::Integer(i) => statement.bind((idx + 1, *i))?,
                Value::Real(r) => statement.bind((idx + 1, *r))?,
                Value::Text(s) => statement.bind((idx + 1, s.as_str()))?,
                Value::Blob(b) => statement.bind((idx + 1, b.as_slice()))?,
                Value::Uuid(u) => statement.bind((idx + 1, u.as_bytes().as_slice()))?,
            }
        }

        let mut rows = Vec::new();
        let mut affected_rows = 0;

        while let State::Row = statement.next()? {
            let mut values = HashMap::new();

            for i in 0..statement.column_count() {
                let name = statement.column_name(i)?.to_string();
                let value = match statement.column_type(i)? {
                    sqlite::Type::Null => Value::Null,
                    sqlite::Type::Integer => Value::Integer(statement.read::<i64, _>(i)?),
                    sqlite::Type::Float => Value::Real(statement.read::<f64, _>(i)?),
                    sqlite::Type::String => Value::Text(statement.read::<String, _>(i)?),
                    sqlite::Type::Binary => Value::Blob(statement.read::<Vec<u8>, _>(i)?),
                };

                values.insert(name, value);
            }

            rows.push(Row::new(values));
            affected_rows += 1;
        }

        Ok(QueryResult::new(rows, affected_rows))
    }

    /// Execute a raw SQL query asynchronously
    pub async fn query(&self, query: &str) -> Result<QueryResult, Box<dyn StdError + Send + Sync>> {
        // Use tokio to execute the query in a blocking context
        let query_string = query.to_string();
        let backend = self.clone();

        // Run the query on a separate thread to not block the async runtime
        match tokio::task::spawn_blocking(move || {
            backend.execute_query(&query_string, &[] as &[Value])
        })
        .await
        {
            Ok(result) => result,
            Err(e) => Err(format!("Task join error: {}", e).into()),
        }
    }

    /// Execute a prepared statement with parameters asynchronously
    pub async fn query_with_params(
        &self,
        query: &str,
        params: Vec<Value>,
    ) -> Result<QueryResult, Box<dyn StdError + Send + Sync>> {
        let query_string = query.to_string();
        let backend = self.clone();

        // Run the query on a separate thread
        match tokio::task::spawn_blocking(move || backend.execute_query(&query_string, &params))
            .await
        {
            Ok(result) => result,
            Err(e) => Err(format!("Task join error: {}", e).into()),
        }
    }

    pub async fn begin_transaction(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let backend = self.clone();
        match tokio::task::spawn_blocking(move || backend.execute_query("BEGIN TRANSACTION;", &[]))
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Task join error: {}", e).into()),
        }
    }

    pub async fn commit_transaction(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let backend = self.clone();
        match tokio::task::spawn_blocking(move || backend.execute_query("COMMIT;", &[])).await {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Task join error: {}", e).into()),
        }
    }

    pub async fn rollback_transaction(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        let backend = self.clone();
        match tokio::task::spawn_blocking(move || backend.execute_query("ROLLBACK;", &[])).await {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Task join error: {}", e).into()),
        }
    }

    pub fn builder(&self) -> SQLBuilder {
        SQLBuilder::Sqlite(SqliteBuilder {})
    }
}

impl SqliteBuilder {
    pub(crate) fn build_table(
        &self,
        table: &crate::Table,
    ) -> Result<String, crate::sql_builder::SQLError> {
        let foreign_keys = table.foreign_keys.iter().map(|fk| {
            format!(
                "FOREIGN KEY ({}) REFERENCES {} ({}) ON UPDATE CASCADE ON DELETE SET NULL",
                fk.fields.join(", "),
                fk.table,
                fk.foreign_fields.join(", ")
            )
        });

        let columns = table
            .columns
            .iter()
            .map(|cmd| match cmd {
                Command::AddColumn {
                    name,
                    r#type,
                    primary_key,
                    unique,
                } => {
                    let primary_key = if *primary_key { "PRIMARY KEY" } else { "" };

                    let unique = if *unique { "UNIQUE" } else { "" };

                    let ty = sql_type(r#type);

                    format!("{} {} {} {}", name, ty, primary_key, unique)
                }
            })
            .chain(foreign_keys)
            .collect::<Vec<_>>()
            .join(", ");

        Ok(format!("CREATE TABLE {} ({});", table.name, columns))
    }
}

fn sql_type(ty: &SqlType) -> String {
    match ty {
        SqlType::Blob => "BLOB".into(),
        SqlType::Real => "REAL".into(),
        SqlType::Integer => "INTEGER".into(),
        SqlType::Text => "TEXT".into(),
        SqlType::Uuid => "BINARY(16)".into(),
        SqlType::Enum(_) => "TEXT".into(),
        SqlType::BitVector(dim) => format!("bit[{}]", dim),
        SqlType::FloatVector(dim) => format!("float[{}]", dim),
        SqlType::Int8Vector(dim) => format!("int8[{}]", dim),
    }
}
