# 5. Writes: Insert, Update, and Delete

Tork ORM supports inserting, updating, and deleting database records. These operations can be performed either on individual model instances or as bulk operations using a `QuerySet`.

---

## 1. Creating Records (Insert)

### A. Insert a Single Instance (`Model::create`)
To insert a new record, construct a model instance and call `<Model>::create`. 

- **Primary Keys:** If the model has an auto-incrementing primary key (e.g. `#[field(primary_key, auto)]`), you can pass any value (like `0`) in the input struct. The database-generated ID will be fetched (via `RETURNING`) and the returned model will contain the actual ID.

```rust
let new_user = User {
    id: 0, // Ignored because of #[field(primary_key, auto)]
    username: "alice".into(),
    email: "alice@example.com".into(),
    is_active: true,
};

// Inserts and returns the stored row containing the database-generated ID.
let stored_user = User::create(&db, &new_user).await?;
assert_eq!(stored_user.id, 1);
```

### B. Bulk Inserting Instances (`Model::bulk_create`)
To insert multiple records efficiently in a single query, use `<Model>::bulk_create`. It returns the number of inserted rows as a `usize`.

```rust
let batch = [
    User { id: 0, username: "bob".into(), email: "bob@x.com".into(), is_active: true },
    User { id: 0, username: "carol".into(), email: "carol@x.com".into(), is_active: true },
];

let inserted_count = User::bulk_create(&db, &batch).await?;
assert_eq!(inserted_count, 2);
```
*Note: If the input slice is empty, `bulk_create` returns `0` immediately without querying the database.*

---

## 2. Updating Records

### A. Updating a Single Instance (`save`)
If you have retrieved a model instance from the database and modified its fields, you can write the changes back using the `save` method. This updates all columns using the instance's primary key value in the `WHERE` clause.

It returns the number of affected rows (typically `1`).

```rust
// Fetch a user
let mut user = User::query()
    .filter(User::username.eq("alice"))
    .one(&db)
    .await?;

// Modify fields
user.email = "alice.new@example.com".into();
user.is_active = false;

// Save changes back to the database
let affected_rows = user.save(&db).await?;
assert_eq!(affected_rows, 1);
```

### B. Bulk Updates (`QuerySet::update`)
To update specific columns on multiple rows matching a query filter without fetching them first, use the `update` method on a `QuerySet`.

It takes a list of column assignments (e.g., `Column.set(value)`) and returns the number of updated rows.

```rust
// Deactivate all users whose usernames do not equal 'bob'
let deactivated_count = User::query()
    .filter(User::username.ne("bob"))
    .update(&db, [
        User::is_active.set(false),
    ])
    .await?;

println!("Deactivated {} users", deactivated_count);
```

---

## 3. Deleting Records (`QuerySet::delete`)

Deletions in Tork ORM are executed via the `QuerySet` builder. This allows deleting specific rows matching a filter, or emptying a table entirely.

It returns the number of deleted rows as a `usize`.

```rust
// Delete a specific user
let deleted = User::query()
    .filter(User::username.eq("bob"))
    .delete(&db)
    .await?;
assert_eq!(deleted, 1);

// Delete all users in the table
let total_deleted = User::query().delete(&db).await?;
println!("Deleted {} remaining users", total_deleted);
```
