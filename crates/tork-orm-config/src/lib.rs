//! Reads the `[package.metadata.tork]` table from a crate's `Cargo.toml`.
//!
//! This is the single, project-level source of truth for the target SQL dialect and
//! the migration tooling settings. It is intentionally tiny and dependency-light so
//! both the procedural macros (at compile time) and the CLI (at run time) can share
//! one parser.
//!
//! Connection settings (host, port, pool size, credentials) deliberately do **not**
//! live here — those stay in application code and the environment.
//!
//! # Example table
//!
//! ```toml
//! [package.metadata.tork]
//! dialect = "postgres"
//!
//! [package.metadata.tork.migrations]
//! dir                  = "migrations"
//! file_template        = "{rev}_{slug}"
//! revision_style       = "hash"
//! truncate_slug_length = 40
//! ```
#![forbid(unsafe_code)]

use std::path::Path;

/// The target SQL dialect declared for a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConfigDialect {
    /// SQLite (the default when no dialect is declared).
    #[default]
    Sqlite,
    /// PostgreSQL.
    Postgres,
}

impl ConfigDialect {
    /// Parses a dialect name, accepting the common spellings.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "sqlite" => Some(Self::Sqlite),
            "postgres" | "postgresql" => Some(Self::Postgres),
            _ => None,
        }
    }

    /// Returns the canonical name of this dialect.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::Postgres => "postgres",
        }
    }

    /// Whether the dialect supports choosing an index method (`USING`).
    ///
    /// These capability flags mirror the runtime `Dialect` trait in the core crate;
    /// they are duplicated here (rather than depended upon) so the macros stay free of
    /// the heavy core dependency. Keep the two in sync.
    pub fn supports_index_method(self) -> bool {
        matches!(self, Self::Postgres)
    }

    /// Whether the dialect supports covering index columns (`INCLUDE`).
    pub fn supports_index_include(self) -> bool {
        matches!(self, Self::Postgres)
    }

    /// Whether the dialect supports per-column index operator classes.
    pub fn supports_index_opclass(self) -> bool {
        matches!(self, Self::Postgres)
    }

    /// Whether the dialect has a native JSON column type.
    pub fn supports_json(self) -> bool {
        matches!(self, Self::Postgres)
    }

    /// Whether the dialect has a native UUID column type.
    pub fn supports_uuid(self) -> bool {
        matches!(self, Self::Postgres)
    }

    /// Whether the dialect has native array column types.
    pub fn supports_array(self) -> bool {
        matches!(self, Self::Postgres)
    }

    /// Returns the human-readable name of the first index feature in use that this
    /// dialect does not support, or `None` when every used feature is supported.
    ///
    /// Drives the compile-time validation in the `Model` derive.
    pub fn unsupported_index_feature(
        self,
        has_method: bool,
        has_include: bool,
        has_opclass: bool,
    ) -> Option<&'static str> {
        if has_method && !self.supports_index_method() {
            return Some("index method (USING)");
        }
        if has_include && !self.supports_index_include() {
            return Some("covering index columns (INCLUDE)");
        }
        if has_opclass && !self.supports_index_opclass() {
            return Some("index operator classes");
        }
        None
    }
}

/// How revision identifiers in migration file names are generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RevisionStyle {
    /// A random short hash (the default).
    #[default]
    Hash,
    /// A zero-padded, incrementing sequence number.
    Sequence,
}

impl RevisionStyle {
    /// Parses a revision style name.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "hash" => Some(Self::Hash),
            "sequence" => Some(Self::Sequence),
            _ => None,
        }
    }
}

/// Migration tooling settings.
#[derive(Debug, Clone)]
pub struct Migrations {
    /// The directory migration files live in.
    pub dir: String,
    /// The file-name template. Tokens: `{rev}`, `{slug}`, `{year}`, `{month}`,
    /// `{day}`, `{hour}`, `{minute}`, `{second}`.
    pub file_template: String,
    /// How `{rev}` is generated.
    pub revision_style: RevisionStyle,
    /// The maximum length of the `{slug}` token.
    pub truncate_slug_length: usize,
    /// The timezone for date tokens; `None` means UTC.
    pub timezone: Option<String>,
}

impl Default for Migrations {
    fn default() -> Self {
        Self {
            dir: "migrations".to_string(),
            file_template: "{rev}_{slug}".to_string(),
            revision_style: RevisionStyle::Hash,
            truncate_slug_length: 40,
            timezone: None,
        }
    }
}

/// The parsed `[package.metadata.tork]` configuration.
#[derive(Debug, Clone, Default)]
pub struct TorkConfig {
    /// The target SQL dialect, or `None` when none was declared.
    ///
    /// Compile-time validation only fires when a dialect is explicitly declared;
    /// an absent dialect keeps the model dialect-neutral (validated later at render
    /// time), preserving the prior behavior.
    pub dialect: Option<ConfigDialect>,
    /// Migration tooling settings.
    pub migrations: Migrations,
}

impl TorkConfig {
    /// The declared dialect, or SQLite when none was declared.
    pub fn dialect_or_default(&self) -> ConfigDialect {
        self.dialect.unwrap_or_default()
    }
}

impl TorkConfig {
    /// Loads the configuration from the `Cargo.toml` in `manifest_dir`.
    ///
    /// A missing file, a missing `[package.metadata.tork]` table, or any missing field
    /// falls back to defaults, so a project with no configuration behaves exactly as
    /// before (SQLite, default migration naming).
    pub fn load(manifest_dir: &Path) -> Self {
        let path = manifest_dir.join("Cargo.toml");
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_cargo_toml_str(&text),
            Err(_) => Self::default(),
        }
    }

    /// Parses the configuration from the contents of a `Cargo.toml`.
    pub fn from_cargo_toml_str(text: &str) -> Self {
        let mut config = Self::default();
        let Ok(table) = text.parse::<toml::Table>() else {
            return config;
        };
        let Some(tork) = table
            .get("package")
            .and_then(toml::Value::as_table)
            .and_then(|p| p.get("metadata"))
            .and_then(toml::Value::as_table)
            .and_then(|m| m.get("tork"))
            .and_then(toml::Value::as_table)
        else {
            return config;
        };

        if let Some(dialect) = tork.get("dialect").and_then(toml::Value::as_str) {
            if let Some(parsed) = ConfigDialect::parse(dialect) {
                config.dialect = Some(parsed);
            }
        }

        if let Some(migrations) = tork.get("migrations").and_then(toml::Value::as_table) {
            let m = &mut config.migrations;
            if let Some(dir) = migrations.get("dir").and_then(toml::Value::as_str) {
                m.dir = dir.to_string();
            }
            if let Some(tpl) = migrations.get("file_template").and_then(toml::Value::as_str) {
                m.file_template = tpl.to_string();
            }
            if let Some(style) = migrations
                .get("revision_style")
                .and_then(toml::Value::as_str)
                .and_then(RevisionStyle::parse)
            {
                m.revision_style = style;
            }
            if let Some(len) = migrations
                .get("truncate_slug_length")
                .and_then(toml::Value::as_integer)
            {
                if len > 0 {
                    m.truncate_slug_length = len as usize;
                }
            }
            if let Some(tz) = migrations.get("timezone").and_then(toml::Value::as_str) {
                m.timezone = Some(tz.to_string());
            }
        }

        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_or_missing_table_yields_defaults() {
        let config = TorkConfig::from_cargo_toml_str("[package]\nname = \"x\"\n");
        assert_eq!(config.dialect, None);
        assert_eq!(config.dialect_or_default(), ConfigDialect::Sqlite);
        assert_eq!(config.migrations.dir, "migrations");
        assert_eq!(config.migrations.file_template, "{rev}_{slug}");
        assert_eq!(config.migrations.revision_style, RevisionStyle::Hash);
        assert_eq!(config.migrations.truncate_slug_length, 40);
        assert_eq!(config.migrations.timezone, None);
    }

    #[test]
    fn reads_dialect_and_migration_settings() {
        let text = r#"
            [package]
            name = "demo"

            [package.metadata.tork]
            dialect = "postgresql"

            [package.metadata.tork.migrations]
            dir = "db/changes"
            file_template = "{year}{month}{day}_{rev}_{slug}"
            revision_style = "sequence"
            truncate_slug_length = 20
            timezone = "Europe/Istanbul"
        "#;
        let config = TorkConfig::from_cargo_toml_str(text);
        assert_eq!(config.dialect, Some(ConfigDialect::Postgres));
        assert_eq!(config.migrations.dir, "db/changes");
        assert_eq!(config.migrations.file_template, "{year}{month}{day}_{rev}_{slug}");
        assert_eq!(config.migrations.revision_style, RevisionStyle::Sequence);
        assert_eq!(config.migrations.truncate_slug_length, 20);
        assert_eq!(config.migrations.timezone.as_deref(), Some("Europe/Istanbul"));
    }

    #[test]
    fn dialect_capabilities_match_expectations() {
        assert!(!ConfigDialect::Sqlite.supports_index_opclass());
        assert!(ConfigDialect::Postgres.supports_index_opclass());
        assert!(!ConfigDialect::Sqlite.supports_index_include());
        assert!(ConfigDialect::Postgres.supports_index_method());
    }

    #[test]
    fn unknown_dialect_is_treated_as_undeclared() {
        let text = "[package.metadata.tork]\ndialect = \"oracle\"\n";
        assert_eq!(TorkConfig::from_cargo_toml_str(text).dialect, None);
    }

    #[test]
    fn unsupported_index_feature_detection() {
        // SQLite supports none of the three; it reports the first one in use.
        assert_eq!(
            ConfigDialect::Sqlite.unsupported_index_feature(true, false, false),
            Some("index method (USING)")
        );
        assert_eq!(
            ConfigDialect::Sqlite.unsupported_index_feature(false, true, false),
            Some("covering index columns (INCLUDE)")
        );
        assert_eq!(
            ConfigDialect::Sqlite.unsupported_index_feature(false, false, true),
            Some("index operator classes")
        );
        assert_eq!(
            ConfigDialect::Sqlite.unsupported_index_feature(false, false, false),
            None
        );
        // PostgreSQL supports all three.
        assert_eq!(
            ConfigDialect::Postgres.unsupported_index_feature(true, true, true),
            None
        );
    }
}
