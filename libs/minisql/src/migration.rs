use uuid::Uuid;

use crate::{connection_pool::Backend, sql_builder::SQLError};

pub struct BitVec<const DIM: u32>(pub Vec<u8>);
pub struct FloatVec<const DIM: u32>(pub Vec<f32>);
pub struct Int8Vec<const DIM: u32>(pub Vec<i8>);

#[derive(Debug, Clone, Default, Hash)]
pub struct Migration {
    tables: Vec<Table>,
    raw_sql: Vec<String>,
}

#[derive(Debug, Clone, Hash)]
pub struct Table {
    pub(crate) name: String,
    pub(crate) columns: Vec<Command>,
    pub(crate) foreign_keys: Vec<ForeignKey>,
    pub(crate) enable_vectors: bool,
}

#[derive(Debug, Clone, Hash)]
pub enum Command {
    AddColumn {
        name: String,
        r#type: SqlType,
        primary_key: bool,
        unique: bool,
    },
}

#[derive(Debug, Clone, Hash)]
pub struct ForeignKey {
    pub fields: Vec<String>,
    pub table: String,
    pub foreign_fields: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SqlTag {
    PrimaryKey,
    Unique,
}

#[derive(Debug, Clone, Hash)]
pub enum SqlType {
    Integer,
    Real,
    Text,
    Blob,
    Uuid,
    Enum(Vec<String>),
    BitVector(u32),
    FloatVector(u32),
    Int8Vector(u32),
}

pub trait SqlLikeType {
    fn as_sql_type() -> SqlType;

    fn to_sql(&self) -> String {
        todo!()
    }
}

impl<T: SqlLikeType> SqlLikeType for Option<T> {
    fn as_sql_type() -> SqlType {
        T::as_sql_type()
    }
}

impl Migration {
    pub fn new() -> Self {
        Migration {
            ..Default::default()
        }
    }

    pub fn table(mut self, t: Table) -> Self {
        self.tables.push(t);
        self
    }

    pub fn raw<S: ToString>(mut self, sql: S) -> Self {
        self.raw_sql.push(sql.to_string());
        self
    }

    pub(crate) fn get_tables(&self) -> &[Table] {
        &self.tables
    }

    pub(crate) fn get_raw_queries(&self) -> &[String] {
        &self.raw_sql
    }
}

impl Table {
    pub fn from_name(name: &str) -> Table {
        Table {
            name: name.to_owned(),
            columns: vec![],
            foreign_keys: vec![],
            enable_vectors: false,
        }
    }

    pub fn add<T: SqlLikeType, S: ToString>(mut self, name: S, tags: &[SqlTag]) -> Self {
        let primary_key = tags.iter().find(|t| *t == &SqlTag::PrimaryKey).is_some();
        let unique = tags.iter().find(|t| *t == &SqlTag::Unique).is_some();

        self.columns.push(Command::AddColumn {
            name: name.to_string(),
            r#type: T::as_sql_type(),
            primary_key,
            unique,
        });

        self
    }

    pub fn foreign_key<S1: ToString, S2: ToString, S3: ToString>(
        mut self,
        fields: &[S1],
        table: S2,
        foreign_fields: &[S3],
    ) -> Self {
        let fields = fields.iter().map(|s| s.to_string()).collect();
        let foreign_fields = foreign_fields.iter().map(|s| s.to_string()).collect();

        self.foreign_keys.push(ForeignKey {
            fields,
            table: table.to_string(),
            foreign_fields,
        });

        self
    }

    pub fn enable_vectors(mut self) -> Self {
        self.enable_vectors = true;
        self
    }

    pub fn to_sql(&self, backend: &Backend) -> Result<String, SQLError> {
        let builder = backend.builder();
        builder.build_table(self)
    }
}

macro_rules! sql_type {
    ($t:ident, $e:expr) => {
        impl SqlLikeType for $t {
            fn as_sql_type() -> SqlType {
                $e
            }
        }
    };
}

sql_type!(i8, SqlType::Integer);
sql_type!(u8, SqlType::Integer);
sql_type!(i16, SqlType::Integer);
sql_type!(u16, SqlType::Integer);
sql_type!(i32, SqlType::Integer);
sql_type!(u32, SqlType::Integer);
sql_type!(i64, SqlType::Integer);
sql_type!(u64, SqlType::Integer);
sql_type!(f32, SqlType::Real);
sql_type!(f64, SqlType::Real);
sql_type!(bool, SqlType::Integer);
sql_type!(String, SqlType::Text);
sql_type!(Uuid, SqlType::Uuid);

impl<const DIM: u32> SqlLikeType for BitVec<DIM> {
    fn as_sql_type() -> SqlType {
        SqlType::BitVector(DIM)
    }
}

impl<const DIM: u32> SqlLikeType for FloatVec<DIM> {
    fn as_sql_type() -> SqlType {
        SqlType::FloatVector(DIM)
    }
}

impl<const DIM: u32> SqlLikeType for Int8Vec<DIM> {
    fn as_sql_type() -> SqlType {
        SqlType::Int8Vector(DIM)
    }
}
