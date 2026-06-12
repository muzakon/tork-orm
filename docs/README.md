# Tork ORM Guide

Tork ORM is an asynchronous, Tortoise-style Object-Relational Mapper (ORM) for Rust, built to integrate natively with the Tork web framework. It provides type-safe, compile-time checked column-based queries, robust relation preloading to prevent N+1 query issues, programmatic and SQL-based migration tools, and a CLI for automated schema generation.

This guide walks you through the ORM from the ground up. Each chapter is filled with detailed, step-by-step examples.

## Contents

1. [Introduction](01-introduction.md)
2. [Defining Models](02-models.md)
3. [Indexing A to Z](03-indexing.md)
4. [Querying and QuerySets](04-queries.md)
5. [Writes: Insert, Update, and Delete](05-writes.md)
6. [Relations and Preloading](06-relations-and-preloading.md)
7. [Aggregates, Grouping, and Projections](07-aggregates-and-projections.md)
8. [Migrations, Schema Generation, and the CLI](08-migrations-and-cli.md)
9. [Tork Framework Integration](09-framework-integration.md)
10. [Database Transactions](10-transactions.md)
11. [Scalar Functions](11-functions.md)

## Key Features

- **Type-safe Querying:** Database columns are mapped to typed handles. Comparing columns to mismatched types is caught at compile time.
- **Tortoise-inspired API:** Ergonomic query chaining (`filter`, `order_by`, `limit`, etc.) with async executors.
- **N+1 Safe Eager Loading:** Preload related models onto their parents in bulk, with exactly one additional query per relationship.
- **A-to-Z Indexing:** Rich support for single-column, unique, compound, descending, partial, functional, and operator-class indexes.
- **Declarative Migrations:** Write migrations programmatically using a DDL builder, or generate SQL files via automated schema diffing.
- **Zero-compilation CLI:** Scaffolds and runs SQL-based migrations with simple database state tracking.
