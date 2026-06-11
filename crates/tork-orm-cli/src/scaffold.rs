//! Pure helpers for scaffolding a new migration file.

/// Generates a fresh 12-character hex revision id.
pub fn new_revision() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Converts a name to `snake_case`, keeping only ASCII alphanumerics.
pub fn snake_case(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// The file name for a migration.
pub fn file_name(revision: &str, snake: &str) -> String {
    format!("{revision}_{snake}.sql")
}

/// Builds the contents of a new migration file.
pub fn template(revision: &str, down_revision: Option<&str>, snake: &str) -> String {
    format!(
        "-- revision: {revision}\n\
         -- down_revision: {down}\n\
         -- name: {snake}\n\
         \n\
         -- migrate:up\n\
         -- Write the schema changes here, for example:\n\
         -- CREATE TABLE \"example\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT);\n\
         \n\
         -- migrate:down\n\
         -- Undo the changes, for example:\n\
         -- DROP TABLE \"example\";\n",
        down = down_revision.unwrap_or(""),
    )
}
