use minisql::{ConnectionPool, Migration, migrations};

migrations! {
    auth_schema {
        table users {
            id: uuid::Uuid [PrimaryKey],
            email: String [Unique],
            password_hash: String,
            created_at: i64,
        }

        table server_sessions {
            id: uuid::Uuid [PrimaryKey],
            user_id: uuid::Uuid,
            token_id: uuid::Uuid [Unique],
            revoked_at: Option<i64>,
            created_at: i64,
            expires_at: i64,

            foreign_key(user_id -> users.id);
        }

        table llm_providers {
            id: uuid::Uuid [PrimaryKey],
            user_id: uuid::Uuid,
            provider_name: String,
            kind: i64,
            api_base_url: String,
            token_nonce: String,
            token_ciphertext: String,
            created_at: i64,
            updated_at: i64,

            foreign_key(user_id -> users.id);
        }

        table llm_models {
            id: uuid::Uuid [PrimaryKey],
            user_id: uuid::Uuid,
            provider_id: uuid::Uuid,
            model_slug: String,
            model_name: String,
            supports_thinking: bool,
            supports_tools: bool,
            supports_image_input: bool,
            created_at: i64,
            updated_at: i64,

            foreign_key(user_id -> users.id);
            foreign_key(provider_id -> llm_providers.id);
        }
    }
}
