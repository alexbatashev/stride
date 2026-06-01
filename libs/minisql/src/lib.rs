mod connection_pool;
mod dsl;
mod migration;
mod postgres;
mod query;
mod sql_builder;
mod sqlite;

pub use connection_pool::ConnectionPool;
pub use dsl::{
    Column, ColumnInput, Delete, Expr, IntoValue, Select, SelectCols, SelectList, Update,
};
pub use migration::{
    AlterTable, BitVec, FloatVec, Int8Vec, Migration, SqlLikeType, SqlTag, SqlType, Table,
};
pub use query::{DecodeError, FromValue, QueryResult, Row, Value};

// Re-export proc macros for type-safe schema generation
pub use minisql_macros::migrations;
