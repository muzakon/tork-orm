# 8. Migrations, Schema Generation, and the CLI

Tork ORM supports two migration strategies: **programmatic migrations** written in Rust, and **SQL-based migrations** managed via the CLI. It also includes an automated schema diffing generator.

---

## 1. Programmatic Migrations (`#[migration]`)

Programmatic migrations are written in Rust using the `#[migration]` attribute macro and the `SchemaManager` DDL builder.

### Defining a Migration
A migration struct must implement functions for its revision, name, up direction (applying changes), and down direction (reverting changes).

```rust
use tork_orm::migration::{migration, Column, ForeignKey, ForeignKeyAction, SchemaManager};
use tork_orm::Result;

pub struct CreateUsers;

#[migration]
impl CreateUsers {
    // Unique revision string (typically a timestamp or hash)
    fn revision() -> &'static str {
        "20260611_000001"
    }

    // Human-readable name
    fn name() -> &'static str {
        "create_users"
    }

    // Statements to apply the migration
    async fn up(schema: &mut SchemaManager<'_>) -> Result<()> {
        schema
            .create_table("users")
            .column(Column::new("id").bigint().primary_key().auto_increment())
            .column(Column::new("username").varchar(50).not_null())
            .execute()
            .await?;
        Ok(())
    }

    // Statements to roll back the migration
    async fn down(schema: &mut SchemaManager<'_>) -> Result<()> {
        schema.drop_table("users").execute().await?;
        Ok(())
    }
}
```

### Running Programmatic Migrations
You group migrations in a `MigrationSet` and use a `Migrator` to run them. Migrations are executed inside database transactions; if a migration fails, the entire transaction rolls back.

```rust
use tork_orm::migration::{boxed, MigrationSet, Migrator};

async fn run_migrations(db: &Database) -> Result<()> {
    // Collect migrations
    let set = MigrationSet::new(vec![
        boxed(CreateUsers),
        boxed(CreatePosts),
    ]);

    // Apply all pending migrations
    let applied = Migrator::new(db, set).up().await?;
    println!("Applied {} migrations", applied);
    Ok(())
}
```

---

## 2. SQL-Based CLI Migrations (`tork-orm`)

If you prefer plain SQL files, Tork ORM provides a standalone binary (`tork-orm`) that tracks applied migrations inside a database table (`_tork_migrations`).

### Migration SQL Format
SQL migrations are written in files inside a `migrations/` directory. Each file contains headers identifying the revision, down-revision link, and boundaries for up/down SQL:

```sql
-- revision: 1975ea83b712
-- down_revision: a3f9c1d4e8b2
-- migrate:up
CREATE TABLE "users" (
    "id" INTEGER PRIMARY KEY AUTOINCREMENT,
    "username" TEXT NOT NULL
);

-- migrate:down
DROP TABLE "users";
```

### CLI Commands
Export your connection string and run commands via the terminal:

```bash
export DATABASE_URL=sqlite://app.db

# 1. Initialize the migrations directory
tork-orm migrate init

# 2. Scaffold a new migration file
tork-orm migrate create add_posts

# 3. Apply all pending migrations
tork-orm migrate up

# 4. Show migration status (applied vs pending)
tork-orm migrate status

# 5. Revert the last migration (rolls back one step)
tork-orm migrate down

# 6. Re-apply the last migration (down then up)
tork-orm migrate redo
```

---

## 3. Automated Schema Generation (`migrate generate`)

Tork ORM can automatically generate migration code by diffing your Rust model declarations against the schema of a live database.

### How it Works

The generator introspects every table tracked by a registered model, computes the difference between the model's declared schema and the live database, and emits the SQL statements needed to close that gap.

For **missing tables** the generator creates the full table and all its indexes in one pass.

For **existing tables** the generator performs a column and index diff in a fixed order that satisfies SQLite's constraints:

1. **Drop stale indexes** — removed before any column is dropped, since SQLite rejects `DROP COLUMN` on a column that an index covers.
2. **Drop removed columns** — columns present in the database but absent from the model get an `ALTER TABLE ... DROP COLUMN` statement. Primary key columns cannot be dropped automatically; a `-- NOTE:` comment is emitted instead.
3. **Add new columns** — columns present in the model but absent from the database get an `ALTER TABLE ... ADD COLUMN` statement. Because `NOT NULL` without a default fails on non-empty tables, newly required columns are added as nullable with a `-- NOTE:` comment; fill the values and add the constraint via a manual table rebuild.
4. **Note type or nullability mismatches** — SQLite has no `ALTER COLUMN`; if a column's type or nullability changed, the generator emits a `-- NOTE:` comment explaining that a table rebuild is required.
5. **Create new indexes** — added after any new columns are in place.

`SchemaChange::is_empty()` returns `true` when the diff contains only informational comments and no executable SQL, so a diff that only notes mismatches does not produce a spurious migration file.

### Basic Usage

```rust
use tork_orm::migration::generate::{generate, write_migration};

async fn generate_migration_file(db: &Database) -> Result<()> {
    let target_schemas = [
        User::table_schema(),
        Post::table_schema(),
    ];

    let change = generate(db, &target_schemas).await?;

    if change.is_empty() {
        println!("Database schema matches the models.");
        return Ok(());
    }

    let migration_dir = std::path::Path::new("./migrations");
    if let Some(path) = write_migration(migration_dir, "auto_sync", &change)? {
        println!("Generated: {:?}", path);
    }
    Ok(())
}
```

### Example: Adding a Column

Suppose the `posts` table exists in the database but the model gains a new `view_count` field:

```rust
#[derive(Model)]
#[table(name = "posts")]
struct Post {
    #[field(primary_key, auto)]
    id: i64,
    title: String,
    view_count: i64,  // new field
}
```

`generate` produces:

```sql
-- migrate:up
ALTER TABLE "posts" ADD COLUMN "view_count" BIGINT;

-- migrate:down
ALTER TABLE "posts" DROP COLUMN "view_count";
```

If `view_count` is not an `Option` (i.e., `NOT NULL`), a note is prepended:

```sql
-- migrate:up
-- NOTE: column "view_count" added as nullable; NOT NULL requires a default value for existing rows
ALTER TABLE "posts" ADD COLUMN "view_count" BIGINT;
```

### Example: Removing a Column

Remove a field from the model and run `generate` again:

```sql
-- migrate:up
ALTER TABLE "posts" DROP COLUMN "obsolete_field";

-- migrate:down
ALTER TABLE "posts" ADD COLUMN "obsolete_field" TEXT;
```

The down migration restores the column schema as nullable. Data in the column before the drop cannot be recovered.

### Example: Type Change (NOTE only)

SQLite cannot alter a column's type in place. When the model's declared type differs from the live schema, the diff emits an informational note and no executable statement:

```sql
-- NOTE: column "score" definition changed (model: BIGINT not null, database: INTEGER not null);
--       rebuild the table to apply the change
```

No migration file is written in this case because `is_empty()` is `true` when only notes are present.

### Using the Registry

If all models are registered via `#[derive(Model)]` (which happens automatically when the `migrations` feature is on), you can use `generate_from_registry` instead of listing schemas manually:

```rust
use tork_orm::migration::generate::generate_from_registry;

let change = generate_from_registry(db).await?;
```

The registry collects every model linked into the binary, so all tables are diffed in one call. This is the form used by `orm_api/src/bin/generate.rs`.
