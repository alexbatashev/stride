use crate::Table;

#[derive(Debug, Clone)]
pub enum SQLError {
    Custom(String),
}

pub enum SQLBuilder {
    Postgres(crate::postgres::PostgresBuilder),
    Sqlite(crate::sqlite::SqliteBuilder),
}
impl SQLBuilder {
    pub fn build_table(&self, table: &Table) -> Result<String, SQLError> {
        match self {
            SQLBuilder::Postgres(b) => b.build_table(table),
            SQLBuilder::Sqlite(b) => b.build_table(table),
        }
    }
}
