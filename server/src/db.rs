use minisql::migrations;

use uuid::Uuid;

migrations! {
    schema {
        table users {
            id: Uuid [PrimaryKey],
            username: String,
            password_hash: String,
        }
    }
}
