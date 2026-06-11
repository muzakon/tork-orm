//! Rendering each command's output: crisp, colored, one summary line.

use std::io::{self, Write};

use tork_orm::migration::{Applied, FileStatus};
use tork_orm::OrmError;

use crate::style::{sym, Style};

/// The direction an action ran, for headers and summaries.
#[derive(Clone, Copy)]
pub enum Action {
    /// Applying migrations.
    Up,
    /// Reverting migrations.
    Down,
}

impl Action {
    fn header(self) -> &'static str {
        match self {
            Action::Up => "Applying migrations",
            Action::Down => "Reverting migrations",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Action::Up => "Applied",
            Action::Down => "Reverted",
        }
    }

    fn idle(self) -> &'static str {
        match self {
            Action::Up => "Already up to date",
            Action::Down => "Nothing to revert",
        }
    }
}

/// Prints the result of an `up`/`down`: one line per migration, then a summary.
pub fn migrations_done(style: &Style, action: Action, results: &[Applied]) {
    if results.is_empty() {
        println!("\n  {}\n", style.dim(action.idle()));
        return;
    }

    println!("\n  {}", style.bold(action.header()));
    let total: u128 = results.iter().map(|r| r.elapsed.as_millis()).sum();
    for result in results {
        println!(
            "  {} {}  {}  {}",
            style.green(sym::CHECK),
            style.cyan(&result.revision),
            pad(&result.name, 20),
            style.dim(&format!("{}ms", result.elapsed.as_millis())),
        );
    }
    println!(
        "\n  {} {} migration{} in {}ms\n",
        action.summary(),
        results.len(),
        plural(results.len()),
        total,
    );
}

/// Prints the migration status table with a one-line summary.
pub fn status(style: &Style, dir: &str, statuses: &[FileStatus]) {
    println!(
        "\n  {}  {}  {}\n",
        style.bold("Migrations"),
        style.dim("·"),
        style.dim(dir),
    );

    let (mut applied, mut changed, mut pending) = (0_usize, 0_usize, 0_usize);
    for entry in statuses {
        let (symbol, note) = match entry.checksum_matches {
            Some(true) => {
                applied += 1;
                (style.green(sym::APPLIED), String::new())
            }
            Some(false) => {
                changed += 1;
                (style.yellow(sym::WARN), style.yellow("changed since applied"))
            }
            None => {
                pending += 1;
                (style.dim(sym::PENDING), String::new())
            }
        };
        let suffix = if note.is_empty() {
            String::new()
        } else {
            format!("  {note}")
        };
        println!(
            "  {}  {}  {}{}",
            symbol,
            style.cyan(&entry.revision),
            pad(&entry.name, 20),
            suffix,
        );
    }

    let mut parts = vec![format!("{applied} applied")];
    if changed > 0 {
        parts.push(format!("{changed} changed"));
    }
    parts.push(format!("{pending} pending"));
    println!("\n  {}\n", style.dim(&parts.join(" · ")));
}

/// Prints the list a destructive `down` would revert, for confirmation.
pub fn revert_preview(style: &Style, items: &[(String, String)]) {
    println!(
        "\n  This will revert {} migration{}:",
        items.len(),
        plural(items.len()),
    );
    for (revision, name) in items {
        println!(
            "    {} {}  {}",
            style.dim(sym::ARROW),
            style.cyan(revision),
            style.dim(name),
        );
    }
}

/// Asks a yes/no question, returning `true` only on an explicit yes.
pub fn confirm(prompt: &str) -> io::Result<bool> {
    print!("  {prompt} [y/N] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

/// Prints a one-line informational message.
pub fn info(style: &Style, message: &str) {
    println!("\n  {}\n", style.dim(message));
}

/// Prints an error block to stderr.
pub fn error(style: &Style, error: &OrmError) {
    eprintln!(
        "\n  {}  {}\n",
        style.red(&format!("{} Error", sym::CROSS)),
        error.message(),
    );
}

/// Left-pads `text` to `width` columns (names are ASCII).
fn pad(text: &str, width: usize) -> String {
    if text.len() >= width {
        text.to_string()
    } else {
        format!("{text}{}", " ".repeat(width - text.len()))
    }
}

/// Returns the plural suffix for a count.
fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}
