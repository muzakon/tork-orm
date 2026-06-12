//! Pure helpers for scaffolding a new migration file.

use tork_orm_config::RevisionStyle;

/// Generates a fresh 12-character hex revision id.
pub fn new_revision() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

/// Returns the revision id for a new migration given the configured style.
///
/// `Hash` produces a fresh 12-character id; `Sequence` produces a zero-padded
/// number one greater than the count of existing migrations.
pub fn revision_id(style: RevisionStyle, existing_count: usize) -> String {
    match style {
        RevisionStyle::Hash => new_revision(),
        RevisionStyle::Sequence => format!("{:04}", existing_count + 1),
    }
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

/// Truncates a slug to at most `max` characters, never leaving a trailing `_`.
pub fn truncate_slug(slug: &str, max: usize) -> String {
    if slug.len() <= max {
        return slug.to_string();
    }
    slug[..max].trim_end_matches('_').to_string()
}

/// The date/time components a file-name template can interpolate, each already
/// zero-padded to its conventional width.
pub struct DateParts {
    /// Four-digit year.
    pub year: String,
    /// Two-digit month.
    pub month: String,
    /// Two-digit day.
    pub day: String,
    /// Two-digit hour (24h).
    pub hour: String,
    /// Two-digit minute.
    pub minute: String,
    /// Two-digit second.
    pub second: String,
}

/// Renders a migration file name from a template and its parts, appending `.sql`.
///
/// Supported tokens: `{rev}`, `{slug}`, `{year}`, `{month}`, `{day}`, `{hour}`,
/// `{minute}`, `{second}`. Unknown tokens are left untouched.
pub fn render_file_name(template: &str, revision: &str, slug: &str, date: &DateParts) -> String {
    let mut name = template
        .replace("{rev}", revision)
        .replace("{slug}", slug)
        .replace("{year}", &date.year)
        .replace("{month}", &date.month)
        .replace("{day}", &date.day)
        .replace("{hour}", &date.hour)
        .replace("{minute}", &date.minute)
        .replace("{second}", &date.second);
    name.push_str(".sql");
    name
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_date() -> DateParts {
        DateParts {
            year: "2026".into(),
            month: "06".into(),
            day: "12".into(),
            hour: "14".into(),
            minute: "30".into(),
            second: "05".into(),
        }
    }

    #[test]
    fn default_template_reproduces_current_names() {
        let name = render_file_name("{rev}_{slug}", "a1b2c3d4e5f6", "add_users", &sample_date());
        assert_eq!(name, "a1b2c3d4e5f6_add_users.sql");
    }

    #[test]
    fn date_tokens_are_interpolated() {
        let name = render_file_name(
            "{year}{month}{day}_{hour}{minute}_{rev}_{slug}",
            "abcdef",
            "add_orders",
            &sample_date(),
        );
        assert_eq!(name, "20260612_1430_abcdef_add_orders.sql");
    }

    #[test]
    fn sequence_revision_is_zero_padded_next_number() {
        assert_eq!(revision_id(RevisionStyle::Sequence, 0), "0001");
        assert_eq!(revision_id(RevisionStyle::Sequence, 41), "0042");
    }

    #[test]
    fn hash_revision_is_twelve_hex_chars() {
        let rev = revision_id(RevisionStyle::Hash, 0);
        assert_eq!(rev.len(), 12);
        assert!(rev.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn slug_is_truncated_without_trailing_underscore() {
        assert_eq!(truncate_slug("add_users", 40), "add_users");
        // Cutting "add_a_table" at 5 would yield "add_a"; cutting at 4 yields "add"
        // after trimming the trailing underscore.
        assert_eq!(truncate_slug("add_a_table", 4), "add");
        assert_eq!(truncate_slug("add_a_table", 5), "add_a");
    }
}
