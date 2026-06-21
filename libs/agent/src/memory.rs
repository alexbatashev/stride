//! Memory palace schema: a per-user, spatially organized long-term memory
//! inspired by MemPalace. Wings hold rooms, rooms hold drawers (verbatim
//! originals) indexed by closets (compressed summary cards carrying the vector
//! embedding). Doors link rooms so a memory can be reached from many angles.
//!
//! The schema lives in the agent crate as a composable namespaced fragment so
//! any host can deploy it; the cloud applies it alongside its own schema with
//! `db.migrator().apply(...).apply(memory::schema()).run()`.

#![allow(non_upper_case_globals)]

use minisql::{DecodeError, FromValue, IntoValue, SqlLikeType, Value, migrations};
use uuid::Uuid;

migrations! {
    namespace memory_palace {
        v1 {
            table memory_wings {
                id: Uuid [PrimaryKey],
                owner: Uuid,
                name: String,
                description: String,
                created_at: i64,

                foreign_key(owner -> users.id);
            }

            table memory_rooms {
                id: Uuid [PrimaryKey],
                owner: Uuid,
                wing: Uuid,
                name: String,
                description: String,
                created_at: i64,

                foreign_key(owner -> users.id);
                foreign_key(wing -> memory_wings.id);
            }

            table memory_halls {
                id: Uuid [PrimaryKey],
                owner: Uuid,
                wing: Uuid,
                name: String,
                description: String,
                created_at: i64,

                foreign_key(owner -> users.id);
                foreign_key(wing -> memory_wings.id);
            }

            table memory_drawers {
                id: Uuid [PrimaryKey],
                owner: Uuid,
                room: Uuid,
                title: String,
                content: String,
                source: Option<String>,
                created_at: i64,

                foreign_key(owner -> users.id);
                foreign_key(room -> memory_rooms.id);
            }

            table memory_closets {
                id: Uuid [PrimaryKey],
                owner: Uuid,
                room: Uuid,
                drawer: Uuid,
                hall: Option<Uuid>,
                summary: String,
                keywords: String,
                embedding: Option<Embedding>,
                created_at: i64,

                foreign_key(owner -> users.id);
                foreign_key(room -> memory_rooms.id);
                foreign_key(drawer -> memory_drawers.id);
                foreign_key(hall -> memory_halls.id);
            }

            table memory_doors {
                id: Uuid [PrimaryKey],
                owner: Uuid,
                from_room: Uuid,
                to_room: Uuid,
                relation: String,
                created_at: i64,

                foreign_key(owner -> users.id);
                foreign_key(from_room -> memory_rooms.id);
                foreign_key(to_room -> memory_rooms.id);
            }

            raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_wings_owner_name ON memory_wings(owner, name)";
            raw "CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_rooms_wing_name ON memory_rooms(wing, name)";
            raw "CREATE INDEX IF NOT EXISTS idx_memory_closets_owner ON memory_closets(owner)";
            raw "CREATE INDEX IF NOT EXISTS idx_memory_drawers_room ON memory_drawers(room)";
        }
    }
}

/// A vector embedding stored as a raw little-endian `f32` blob. Kept as an
/// opaque byte column so the schema is independent of the embedding model's
/// dimension; cosine distance is computed by sqlite-vec at query time.
#[derive(Clone, Debug)]
pub struct Embedding(pub Vec<u8>);

impl Embedding {
    /// Pack a float vector into the little-endian byte layout sqlite-vec reads.
    pub fn from_floats(values: &[f32]) -> Self {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        Embedding(bytes)
    }
}

impl SqlLikeType for Embedding {
    fn as_sql_type() -> minisql::SqlType {
        minisql::SqlType::Blob
    }
}

impl FromValue for Embedding {
    fn from_value(v: &Value) -> Result<Self, DecodeError> {
        match v {
            Value::Blob(b) => Ok(Embedding(b.clone())),
            other => Err(DecodeError(format!(
                "expected BLOB for Embedding, got {other}"
            ))),
        }
    }
}

impl From<Embedding> for Value {
    fn from(val: Embedding) -> Value {
        Value::Blob(val.0)
    }
}

impl IntoValue for Embedding {
    fn into_value(self) -> Value {
        Value::Blob(self.0)
    }
}
