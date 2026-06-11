//! Terminal styling: a small ANSI palette gated on whether output is a terminal.
//!
//! Colors are raw ANSI escapes (no dependency). When stdout is not a terminal, or
//! `NO_COLOR` is set, or `--no-color` is passed, every helper returns plain text —
//! so output is clean in pipes and easy to assert on in tests.

use std::io::IsTerminal;

/// Symbols used across the output.
pub mod sym {
    /// Success marker.
    pub const CHECK: &str = "✔";
    /// Failure marker.
    pub const CROSS: &str = "✖";
    /// Applied-migration dot.
    pub const APPLIED: &str = "●";
    /// Pending-migration dot.
    pub const PENDING: &str = "○";
    /// Warning / checksum-changed marker.
    pub const WARN: &str = "⚠";
    /// List arrow.
    pub const ARROW: &str = "→";
}

/// Applies (or omits) ANSI colors.
#[derive(Clone, Copy)]
pub struct Style {
    color: bool,
}

impl Style {
    /// Detects whether to colorize: a terminal, no `NO_COLOR`, no `--no-color`.
    pub fn detect(no_color: bool) -> Self {
        let color = !no_color
            && std::env::var_os("NO_COLOR").is_none()
            && std::io::stdout().is_terminal();
        Self { color }
    }

    /// Wraps `text` in an ANSI code when colorizing.
    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\u{1b}[{code}m{text}\u{1b}[0m")
        } else {
            text.to_string()
        }
    }

    /// Green (success).
    pub fn green(&self, text: &str) -> String {
        self.paint("32", text)
    }

    /// Red (error).
    pub fn red(&self, text: &str) -> String {
        self.paint("31", text)
    }

    /// Yellow (warning / changed).
    pub fn yellow(&self, text: &str) -> String {
        self.paint("33", text)
    }

    /// Cyan (revisions / headers).
    pub fn cyan(&self, text: &str) -> String {
        self.paint("36", text)
    }

    /// Dim (secondary text).
    pub fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    /// Bold (emphasis).
    pub fn bold(&self, text: &str) -> String {
        self.paint("1", text)
    }
}
