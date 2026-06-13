//! A complex multi-vendor e-commerce schema modeled with the Tork ORM.
//!
//! This crate exists as a realistic, production-shaped example: ~28 tables with
//! enums, JSON columns, self-referential categories, composite-unique constraints,
//! foreign-key actions, CHECK constraints, soft-delete, optimistic locking, and
//! money stored as integer minor units (cents). The `tests/` directory exercises
//! the production concerns: migration up/down, constraints, transactions, row
//! locking, parameter-binding safety, relations, and aggregates.

pub mod enums;
pub mod models;
pub mod testkit;
