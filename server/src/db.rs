#![allow(non_upper_case_globals)]

use minisql::migrations;

use uuid::Uuid;

migrations! {
    schema {
        table users {
            id: Uuid [PrimaryKey],
            username: String [Unique],
            password_hash: String,
        }

        table sessions {
            id: Uuid [PrimaryKey],
            user_id: Uuid,
            expires_at: i64,

            foreign_key(user_id -> users.id);
        }
    }
}
