//! A stable checksum over a migration's rendered DDL.
//!
//! Applying a migration records a checksum of the SQL it renders. On a later run
//! the checksum is recomputed and compared, so an already-applied migration whose
//! definition changed can be detected. The hash is computed over the rendered SQL
//! (the semantic artifact), not the source file.
//!
//! FNV-1a is used: a small, stable, dependency-free hash. This is change
//! detection, not tamper resistance, so a non-cryptographic hash is appropriate.

/// FNV-1a 64-bit offset basis.
const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Computes the FNV-1a 64-bit hash of `bytes`.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Returns a stable hex checksum of the rendered statements.
///
/// # Examples
///
/// ```
/// use tork_orm_core::migration::checksum::checksum_of;
///
/// let a = checksum_of(&["CREATE TABLE x (id INTEGER)".to_string()]);
/// let b = checksum_of(&["CREATE TABLE x (id INTEGER)".to_string()]);
/// assert_eq!(a, b);
/// assert_eq!(a.len(), 16);
/// ```
pub fn checksum_of(statements: &[String]) -> String {
    format!("{:016x}", fnv1a64(statements.join("\n").as_bytes()))
}
