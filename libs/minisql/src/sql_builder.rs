use crate::Table;

#[derive(Debug, Clone)]
pub enum SQLError {
    Custom(String),
}

pub enum SQLBuilder {
    Sqlite(crate::sqlite::SqliteBuilder),
}
impl SQLBuilder {
    pub fn build_table(&self, table: &Table) -> Result<String, SQLError> {
        match self {
            SQLBuilder::Sqlite(b) => b.build_table(table),
        }
    }
}
