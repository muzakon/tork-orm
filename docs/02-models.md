# 2. Defining Models

In Tork ORM, database tables are represented by Rust structs that implement the `Model` trait. Instead of writing this trait implementation manually, you use the `#[derive(Model)]` macro.

## Model Declaration

To define a model, annotate a struct with `#[derive(Model)]` and specify the table name using the `#[table(name = "...")]` helper attribute:

```rust
use tork_orm::prelude::*;

#[derive(Debug, Clone, Model)]
#[table(name = "users")]
pub struct User {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(varchar(length = 50), unique)]
    pub username: String,
    pub email: String,
    pub is_active: bool,
    pub nickname: Option<String>,
}
```

## Field Attributes

Each field in your struct corresponds to a column in the database table. By default, fields are mapped based on their Rust type (e.g., `i64` becomes `BigInt`, `String` becomes `Text`, `bool` becomes `Integer` or `Boolean`).

You can customize this mapping using the `#[field(...)]` attribute:

| Attribute | Description |
|---|---|
| `primary_key` | Marks the field as the table's primary key column. |
| `auto` | Marks the column as auto-incrementing / database-generated. It will be excluded from INSERT statements, and its populated value will be read back from the database. |
| `varchar(length = N)` | Maps the field to a SQL `VARCHAR(N)` type instead of the default `TEXT`. |
| `unique` | Generates a unique index on this single column. |
| `index` | Generates a standard index on this single column. |
| `foreign_key = ReferencedModel::field` | Defines a foreign key constraint pointing to the specified model's field. By default, this also generates a non-unique index on this column. |
| `index = false` | When combined with `foreign_key`, opts out of the automatic index generation on the foreign key column. |

### Foreign Keys Example

```rust
#[derive(Debug, Clone, Model)]
#[table(name = "posts")]
pub struct Post {
    #[field(primary_key, auto)]
    pub id: i64,
    // Defines a foreign key to User::id and automatically indexes it
    #[field(foreign_key = User::id)]
    pub user_id: i64,
    // Defines a foreign key but disables automatic indexing
    #[field(foreign_key = User::id, index = false)]
    pub creator_id: i64,
    pub title: String,
}
```

## Custom Field Types (`BindValue` and `FromValue`)

If you want to use custom types (like enums or complex structs) as model fields, you must implement the `BindValue` and `FromValue` traits. These traits tell Tork ORM how to serialize the type to a database-compatible `Value` and deserialize it back.

### Custom Enum Example

Here is how to map a `PostStatus` enum to a text column:

```rust
use tork_orm::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub enum PostStatus {
    Draft,
    Published,
}

// Convert Rust enum to Database Value
impl BindValue for PostStatus {
    fn to_value(&self) -> Value {
        match self {
            PostStatus::Draft => Value::Text("draft".into()),
            PostStatus::Published => Value::Text("published".into()),
        }
    }
}

// Convert Database Value back to Rust enum
impl FromValue for PostStatus {
    fn from_value(value: Value) -> Result<Self> {
        match value {
            Value::Text(text) if text == "published" => Ok(PostStatus::Published),
            _ => Ok(PostStatus::Draft), // Default or fallback
        }
    }
}

// Now the enum can be used directly in models:
#[derive(Debug, Clone, Model)]
#[table(name = "articles")]
pub struct Article {
    #[field(primary_key, auto)]
    pub id: i64,
    pub title: String,
    pub status: PostStatus, // Mapped automatically
}
```

## The Model Trait API

Deriving `Model` generates several helpful metadata constants and methods that can be accessed programmatically:

- `<ModelName>::TABLE`: The table name as a `&'static str`.
- `<ModelName>::PRIMARY_KEY`: The primary key column name as a `&'static str`.
- `<ModelName>::COLUMNS`: A slice of `ColumnDef` structs describing the table's structure.
- `<ModelName>::indexes()`: Returns a `Vec<IndexDef>` containing all indexes declared on the model.
- `<ModelName>::table_schema()`: Returns a `TableDef` for DDL queries.
- `instance.primary_key_value()`: Returns the database value of the instance's primary key.
- `instance.insert_values()`: Returns a `Vec<(&'static str, Value)>` representing the columns and values to insert, skipping auto columns.
