use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::{Arc, Mutex};

use postgres::types::{ToSql, Type};
use postgres::{Client, NoTls};

use crate::migration::{Command, SqlType};
use crate::query::{QueryResult, Row, Value};
use crate::sql_builder::SQLBuilder;

#[derive(Clone)]
pub struct PostgresBackend {
    client: Arc<Mutex<Client>>,
}

pub struct PostgresBuilder;

impl PostgresBackend {
    pub fn new(url: &str) -> Result<Self, Box<dyn StdError + Send + Sync>> {
        Ok(Self {
            client: Arc::new(Mutex::new(Client::connect(url, NoTls)?)),
        })
    }

    pub async fn query(&self, query: &str) -> Result<QueryResult, Box<dyn StdError + Send + Sync>> {
        self.query_with_params(query, Vec::new()).await
    }

    pub async fn query_with_params(
        &self,
        query: &str,
        params: Vec<Value>,
    ) -> Result<QueryResult, Box<dyn StdError + Send + Sync>> {
        let query = postgres_placeholders(query);
        let params = postgres_params(params);
        let param_refs = params
            .iter()
            .map(|p| &**p as &(dyn ToSql + Sync))
            .collect::<Vec<_>>();

        let mut client = self
            .client
            .lock()
            .map_err(|_| "postgres client lock is poisoned".to_string())?;

        if returns_rows(&query) {
            let rows = client.query(&query, &param_refs)?;
            let rows = rows
                .into_iter()
                .map(postgres_row)
                .collect::<Result<Vec<_>, _>>()?;
            let affected_rows = rows.len();
            Ok(QueryResult::new(rows, affected_rows))
        } else {
            let affected_rows = client.execute(&query, &param_refs)? as usize;
            Ok(QueryResult::new(Vec::new(), affected_rows))
        }
    }

    pub async fn begin_transaction(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        self.query("BEGIN;").await.map(|_| ())
    }

    pub async fn commit_transaction(&self) -> Result<(), Box<dyn StdError + Send + Sync>> {
        self.query("COMMIT;").await.map(|_| ())
    }

    pub(crate) fn rollback_transaction_fire_and_forget(&self) {
        if let Ok(mut client) = self.client.lock() {
            let _ = client.simple_query("ROLLBACK;");
        }
    }

    pub fn builder(&self) -> SQLBuilder {
        SQLBuilder::Postgres(PostgresBuilder {})
    }
}

fn postgres_params(params: Vec<Value>) -> Vec<Box<dyn ToSql + Sync>> {
    params
        .into_iter()
        .map(|param| match param {
            Value::Null => Box::new(Option::<i32>::None) as Box<dyn ToSql + Sync>,
            Value::Integer(i) => Box::new(i),
            Value::Real(r) => Box::new(r),
            Value::Text(s) => Box::new(s),
            Value::Blob(b) => Box::new(b),
            Value::Uuid(u) => Box::new(u),
        })
        .collect()
}

fn postgres_placeholders(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    let mut idx = 1;

    for ch in query.chars() {
        if ch == '?' {
            out.push('$');
            out.push_str(&idx.to_string());
            idx += 1;
        } else {
            out.push(ch);
        }
    }

    out
}

fn returns_rows(query: &str) -> bool {
    let query = query.trim_start().to_ascii_uppercase();
    query.starts_with("SELECT") || query.starts_with("WITH") || query.contains(" RETURNING ")
}

fn postgres_row(row: postgres::Row) -> Result<Row, Box<dyn StdError + Send + Sync>> {
    let mut values = HashMap::new();

    for (idx, column) in row.columns().iter().enumerate() {
        let ty = column.type_();
        let value = if *ty == Type::INT2 {
            row.try_get::<_, Option<i16>>(idx)?
                .map(|v| Value::Integer(v as i64))
                .unwrap_or(Value::Null)
        } else if *ty == Type::INT4 {
            row.try_get::<_, Option<i32>>(idx)?
                .map(|v| Value::Integer(v as i64))
                .unwrap_or(Value::Null)
        } else if *ty == Type::INT8 {
            row.try_get::<_, Option<i64>>(idx)?
                .map(Value::Integer)
                .unwrap_or(Value::Null)
        } else if *ty == Type::FLOAT4 {
            row.try_get::<_, Option<f32>>(idx)?
                .map(|v| Value::Real(v as f64))
                .unwrap_or(Value::Null)
        } else if *ty == Type::FLOAT8 {
            row.try_get::<_, Option<f64>>(idx)?
                .map(Value::Real)
                .unwrap_or(Value::Null)
        } else if *ty == Type::BOOL {
            row.try_get::<_, Option<bool>>(idx)?
                .map(|v| Value::Integer(if v { 1 } else { 0 }))
                .unwrap_or(Value::Null)
        } else if *ty == Type::BYTEA {
            row.try_get::<_, Option<Vec<u8>>>(idx)?
                .map(Value::Blob)
                .unwrap_or(Value::Null)
        } else if *ty == Type::UUID {
            row.try_get::<_, Option<uuid::Uuid>>(idx)?
                .map(Value::Uuid)
                .unwrap_or(Value::Null)
        } else if *ty == Type::TEXT
            || *ty == Type::VARCHAR
            || *ty == Type::BPCHAR
            || *ty == Type::NAME
        {
            row.try_get::<_, Option<String>>(idx)?
                .map(Value::Text)
                .unwrap_or(Value::Null)
        } else {
            return Err(format!("unsupported postgres column type: {}", ty.name()).into());
        };

        values.insert(column.name().to_string(), value);
    }

    Ok(Row::new(values))
}

impl PostgresBuilder {
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
        SqlType::Blob => "BYTEA".into(),
        SqlType::Real => "DOUBLE PRECISION".into(),
        SqlType::Integer => "BIGINT".into(),
        SqlType::Text => "TEXT".into(),
        SqlType::Uuid => "UUID".into(),
        SqlType::Enum(_) => "TEXT".into(),
        SqlType::BitVector(_) => "BYTEA".into(),
        SqlType::FloatVector(_) => "BYTEA".into(),
        SqlType::Int8Vector(_) => "BYTEA".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{PostgresBuilder, postgres_placeholders};
    use crate::{SqlTag, Table};

    #[test]
    fn converts_question_mark_placeholders_to_postgres_placeholders() {
        assert_eq!(
            postgres_placeholders("SELECT * FROM users WHERE id = ? AND name = ? LIMIT ?"),
            "SELECT * FROM users WHERE id = $1 AND name = $2 LIMIT $3"
        );
    }

    #[test]
    fn builds_postgres_create_table_sql() {
        let table = Table::from_name("users")
            .add::<i64, _>("id", &[SqlTag::PrimaryKey])
            .add::<uuid::Uuid, _>("external_id", &[SqlTag::Unique])
            .add::<String, _>("name", &[])
            .add::<crate::FloatVec<3>, _>("embedding", &[]);

        assert_eq!(
            PostgresBuilder.build_table(&table).unwrap(),
            "CREATE TABLE users (id BIGINT PRIMARY KEY , external_id UUID  UNIQUE, name TEXT  , embedding BYTEA  );"
        );
    }
}
