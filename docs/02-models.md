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

### Database Enums (`#[derive(DbEnum)]`)

Implementing `BindValue`/`FromValue` by hand (above) works, but for the common case of an enum stored as text you can derive `DbEnum` instead. It generates the conversions, records the allowed values, and gives the column a real database enum type.

```rust
use tork_orm::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, DbEnum)]
pub enum Status {
    Active,                       // stored as 'active'
    Inactive,                     // stored as 'inactive'
    #[db_enum(rename = "on_hold")]
    OnHold,                       // stored as 'on_hold'
}
```

By default each variant is stored as its `snake_case` name. Override one variant with `#[db_enum(rename = "...")]`, or the whole enum with `#[db_enum(rename_all = "...")]` (`snake_case`, `SCREAMING_SNAKE_CASE`, `kebab-case`, `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`). Set the type name with `#[db_enum(name = "...")]` (it defaults to the `snake_case` of the enum).

Mark the model field with `#[field(db_enum)]` to use it:

```rust
#[derive(Debug, Clone, Model)]
#[table(name = "accounts")]
pub struct Account {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(db_enum)]
    pub status: Status,
    #[field(db_enum)]
    pub tier: Option<Status>,   // nullable
}
```

Enums work on every backend. The column renders as a native `ENUM('active', 'inactive', 'on_hold')` on MySQL and as a text column with a `CHECK (status IN ('active', 'inactive', 'on_hold'))` constraint on PostgreSQL and SQLite, so an out-of-range value is rejected by the database everywhere. Filtering uses the enum value directly:

```rust
let active = Account::query()
    .filter(Account::status.eq(Status::Active))
    .all(&db)
    .await?;
```

## Lifecycle Columns

Four field attributes turn a column into a managed lifecycle column. They are ordinary columns you can still read and write; the ORM maintains them for you.

| Attribute | Field type | Behavior |
| --- | --- | --- |
| `#[field(created_at)]` | `OffsetDateTime` | Set to the database time on insert; never changed afterwards. |
| `#[field(updated_at)]` | `OffsetDateTime` | Set on insert and refreshed to the database time on every `save()`. |
| `#[field(version)]` | integer (`i32`/`i64`) | Optimistic-lock counter, checked and incremented by `save()`. |
| `#[field(deleted_at)]` | `Option<OffsetDateTime>` | Soft-delete marker; `delete()` stamps it instead of removing the row. |

```rust
use time::OffsetDateTime;

#[derive(Debug, Clone, Model)]
#[table(name = "documents")]
pub struct Document {
    #[field(primary_key, auto)]
    pub id: i64,
    pub body: String,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
    #[field(version)]
    pub version: i64,
    #[field(deleted_at)]
    pub deleted_at: Option<OffsetDateTime>,
}
```

The `created_at` and `updated_at` columns rely on a database default, so declare them `DEFAULT CURRENT_TIMESTAMP` in your migration. The write-time behavior is covered in [Writes](05-writes.md); the soft-delete query scope is covered in [Querying](04-queries.md).

## The Model Trait API

Deriving `Model` generates several helpful metadata constants and methods that can be accessed programmatically:

- `<ModelName>::TABLE`: The table name as a `&'static str`.
- `<ModelName>::PRIMARY_KEY`: The primary key column name as a `&'static str`.
- `<ModelName>::COLUMNS`: A slice of `ColumnDef` structs describing the table's structure.
- `<ModelName>::indexes()`: Returns a `Vec<IndexDef>` containing all indexes declared on the model.
- `<ModelName>::table_schema()`: Returns a `TableDef` for DDL queries.
- `instance.primary_key_value()`: Returns the database value of the instance's primary key.
- `instance.insert_values()`: Returns a `Vec<(&'static str, Value)>` representing the columns and values to insert, skipping auto columns.
