use crate::Table;
use crate::migration::{AlterAction, AlterTable, SqlType};

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

    pub fn build_alter_table(&self, alter: &AlterTable) -> Result<Vec<String>, SQLError> {
        match self {
            SQLBuilder::Postgres(b) => b.build_alter_table(alter),
            SQLBuilder::Sqlite(b) => b.build_alter_table(alter),
        }
    }
}

// ALTER TABLE syntax is shared across backends; only column type spelling
// differs, so each backend passes its own `sql_type` mapping.
pub(crate) fn build_alter_statements(
    table: &str,
    actions: &[AlterAction],
    sql_type: fn(&SqlType) -> String,
) -> Vec<String> {
    actions
        .iter()
        .map(|action| match action {
            AlterAction::AddColumn { name, r#type } => format!(
                "ALTER TABLE {} ADD COLUMN {} {};",
                table,
                name,
                sql_type(r#type)
            ),
            AlterAction::DropColumn { name } => {
                format!("ALTER TABLE {} DROP COLUMN {};", table, name)
            }
            AlterAction::RenameColumn { from, to } => {
                format!("ALTER TABLE {} RENAME COLUMN {} TO {};", table, from, to)
            }
            AlterAction::RenameTable { to } => {
                format!("ALTER TABLE {} RENAME TO {};", table, to)
            }
        })
        .collect()
}
