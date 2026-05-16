#![allow(non_upper_case_globals)]

use minisql::{DecodeError, FromValue, IntoValue, SqlLikeType, Value, migrations};
use uuid::Uuid;

#[derive(Debug)]
pub enum Role {
    System,
    Agent,
    User,
    Tool,
}

migrations! {
    schema {
        table threads {
            id: Uuid [PrimaryKey],
            base_dir: String,
            working_dir: String,
        }

        table messages {
            id: Uuid [PrimaryKey],
            parent_thread: Uuid,
            seq: u64,
            role: Role,
            content: String,
            thinking: Option<String>,

            foreign_key(parent_thread -> threads.id);
        }
    }
}

impl FromValue for Role {
    fn from_value(v: &minisql::Value) -> Result<Self, minisql::DecodeError> {
        match v {
            Value::Text(s) if s == "system" => Ok(Role::System),
            Value::Text(s) if s == "agent" => Ok(Role::Agent),
            Value::Text(s) if s == "user" => Ok(Role::User),
            Value::Text(s) if s == "tool" => Ok(Role::Tool),
            _ => Err(DecodeError("Invalid role".to_string())),
        }
    }
}

impl SqlLikeType for Role {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl From<Role> for Value {
    fn from(val: Role) -> Self {
        match val {
            Role::System => Value::Text("system".to_string()),
            Role::Agent => Value::Text("agent".to_string()),
            Role::User => Value::Text("user".to_string()),
            Role::Tool => Value::Text("tool".to_string()),
        }
    }
}

impl IntoValue for Role {
    fn into_value(self) -> Value {
        self.into()
    }
}
