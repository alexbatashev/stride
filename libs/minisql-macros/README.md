# MinSQL Macros

This crate provides procedural macros for the MinSQL library to enable type-safe, declarative database schema definitions.

## Features

- **Type-safe migrations**: Define database schemas using Rust types
- **Declarative syntax**: Clean, readable migration definitions
- **Foreign key support**: Define relationships between tables
- **Vector extensions**: Enable vector search capabilities
- **Custom column names**: Handle reserved keywords with custom column mapping

## Usage

### Basic Migration

```rust
use minisql::{migrations, Migration};
use uuid::Uuid;

migrations! {
    initial_schema {
        table users {
            id: Uuid [PrimaryKey],
            name: String,
            email: String [Unique],
            created_at: String,
        }
        
        table posts {
            id: Uuid [PrimaryKey],
            user_id: Uuid,
            title: String,
            content: String,
            
            foreign_key(user_id -> users.id);
        }
    }
}

// This generates a `get_migrations()` function that returns Vec<Migration>
fn main() {
    let migrations = get_migrations();
    // Use migrations with your connection pool
}
```

### Advanced Features

#### Custom Column Names

When you need to use Rust keywords as column names:

```rust
migrations! {
    schema_v1 {
        table files {
            id: Uuid [PrimaryKey],
            file_type: FileType [column = "type"],  // Maps to "type" column in DB
            name: String,
        }
    }
}
```

#### Vector Extensions

Enable vector search capabilities:

```rust
use minisql::{FloatVec, migrations};

migrations! {
    vector_schema {
        table documents {
            id: Uuid [PrimaryKey],
            content: String,
            embedding: FloatVec<1536>,
            
            enable_vectors;
        }
    }
}
```

#### Multiple Foreign Keys

```rust
migrations! {
    complex_schema {
        table user_roles {
            user_id: Uuid,
            role_id: Uuid,
            assigned_by: Uuid,
            
            foreign_key(user_id -> users.id);
            foreign_key(role_id -> roles.id);
            foreign_key(assigned_by -> users.id);
        }
    }
}
```

#### Raw SQL

For complex migrations that need raw SQL:

```rust
migrations! {
    mixed_schema {
        table users {
            id: Uuid [PrimaryKey],
            name: String,
        }
        
        raw "CREATE INDEX idx_users_name ON users(name);";
        raw "CREATE VIEW active_users AS SELECT * FROM users WHERE active = true;";
    }
}
```

## Supported Types

The macro supports all types that implement `SqlLikeType`:

- **Primitive types**: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `bool`
- **String types**: `String`
- **UUID**: `uuid::Uuid`
- **Vector types**: `BitVec<N>`, `FloatVec<N>`, `Int8Vec<N>`
- **Custom enums**: Any enum that implements `SqlLikeType`

## SQL Tags

- `PrimaryKey` - Mark column as primary key
- `Unique` - Add unique constraint

## Multiple Migrations

You can define multiple migrations in a single macro call:

```rust
migrations! {
    initial_schema {
        table users {
            id: Uuid [PrimaryKey],
            name: String,
        }
    }
    
    add_posts {
        table posts {
            id: Uuid [PrimaryKey],
            user_id: Uuid,
            title: String,
            
            foreign_key(user_id -> users.id);
        }
    }
}
```

This generates a function that returns a `Vec<Migration>` with both migrations in order.

## Error Handling

The macro provides compile-time validation:
- Unknown SQL tags will cause compilation errors
- Invalid foreign key syntax will be caught at compile time
- Type mismatches are prevented by Rust's type system

## Integration

This macro integrates seamlessly with the MinSQL connection pool:

```rust
use minisql::ConnectionPool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = ConnectionPool::new("sqlite:memory:")?;
    let migrations = get_migrations();
    
    db.initialize_database(migrations).await?;
    
    Ok(())
}
```
