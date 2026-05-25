#![allow(non_upper_case_globals)]

use minisql::{DecodeError, FromValue, IntoValue, SqlLikeType, Value, migrations};

use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub enum Role {
    System,
    Agent,
    User,
    Tool,
}

#[derive(Clone, Copy, Debug)]
pub enum ObjectKind {
    Directory,
    File,
}

migrations! {
    schema {
        table users {
            id: Uuid [PrimaryKey],
            username: String [Unique],
            password_hash: String,
            personality: Option<String>,
        }

        table sessions {
            id: Uuid [PrimaryKey],
            user_id: Uuid,
            expires_at: i64,

            foreign_key(user_id -> users.id);
        }

        table projects {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            title: String,

            foreign_key(owner -> users.id);
        }

        table threads {
            id: Uuid [PrimaryKey],
            owner: Uuid,
            title: String,
            project_id: Option<Uuid>,

            foreign_key(owner -> users.id);
            foreign_key(project_id -> projects.id);
        }

        table messages {
            id: Uuid [PrimaryKey],
            parent_thread: Uuid,
            seq: u64,
            role: Role,
            content: String,
            thinking: Option<String>,
            tool_calls: Option<String>,
            tool_call_id: Option<String>,

            foreign_key(parent_thread -> threads.id);
        }

        table skills {
            id: Uuid [PrimaryKey],
            name: String [Unique],
            title: String,
            description: String,
            content: String,
            owner: Option<Uuid>,

            foreign_key(owner -> users.id);
        }

        table vfs_workspaces {
            id: Uuid [PrimaryKey],
            parent_thread: Option<Uuid>,
            parent_project: Option<Uuid>,
        }

        table vfs_nodes {
            id: Uuid [PrimaryKey],
            name: String,
            kind: ObjectKind,
            parent_node: Option<Uuid>,
            parent_workspace: Option<Uuid>,
            owner: Uuid,
            created_at: i64,
            mime_type: Option<String>,

            foreign_key(parent_node -> vfs_nodes.id);
            foreign_key(parent_workspace -> vfs_workspaces.id);
            foreign_key(owner -> users.id);
        }

        table vfs_objects {
            id: Uuid [PrimaryKey],
            version: i64,
            location: String,
            created_at: i64,
            node: Uuid,
            size: i64,

            foreign_key(node -> vfs_nodes.id);
        }
    }
}

impl FromValue for Role {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
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
    fn from(val: Role) -> Value {
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

impl SqlLikeType for ObjectKind {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Text
    }
}

impl FromValue for ObjectKind {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Text(s) if s == "dir" => Ok(ObjectKind::Directory),
            Value::Text(s) if s == "file" => Ok(ObjectKind::File),
            _ => Err(DecodeError("Invalid object kind".to_string())),
        }
    }
}

impl From<ObjectKind> for Value {
    fn from(val: ObjectKind) -> Value {
        match val {
            ObjectKind::Directory => Value::Text("dir".to_string()),
            ObjectKind::File => Value::Text("file".to_string()),
        }
    }
}

impl IntoValue for ObjectKind {
    fn into_value(self) -> Value {
        self.into()
    }
}
