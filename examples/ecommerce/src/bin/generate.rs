//! App-embedded `migrate generate`.
//!
//! The standalone `tork-orm` CLI cannot see an application's Rust models, so
//! schema-diffing generation runs from inside the app, where the model types are
//! linked. This binary diffs every model against a database and writes a migration
//! that reconciles it (creating any missing table with its columns and indexes).
//!
//! ```text
//! DATABASE_URL=sqlite://app.db cargo run -p ecommerce --bin generate -- initial
//! ```

use std::path::Path;

use ecommerce::models::*;
use tork_orm::migration::generate::{generate, write_migration};
// Specific imports, not `prelude::*`: the prelude's `OrderItem` (an ORDER BY term)
// would otherwise collide with the `OrderItem` domain model under two glob imports.
use tork_orm::{Database, Model, TableSchema};

/// Every model, in foreign-key dependency order, for a readable generated file.
fn model_schemas() -> Vec<TableSchema> {
    vec![
        User::table_schema(),
        Address::table_schema(),
        Vendor::table_schema(),
        VendorMember::table_schema(),
        Brand::table_schema(),
        Category::table_schema(),
        Product::table_schema(),
        ProductCategory::table_schema(),
        ProductVariant::table_schema(),
        ProductImage::table_schema(),
        InventoryLocation::table_schema(),
        InventoryItem::table_schema(),
        InventoryMovement::table_schema(),
        Cart::table_schema(),
        CartItem::table_schema(),
        Coupon::table_schema(),
        Order::table_schema(),
        OrderItem::table_schema(),
        OrderAddress::table_schema(),
        CouponRedemption::table_schema(),
        Payment::table_schema(),
        Shipment::table_schema(),
        ShipmentItem::table_schema(),
        ReturnRequest::table_schema(),
        ReturnItem::table_schema(),
        Refund::table_schema(),
        Review::table_schema(),
        AuditLog::table_schema(),
    ]
}

#[tokio::main]
async fn main() -> tork_orm::Result<()> {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite::memory:".to_string());
    let dir = std::env::var("MIGRATIONS_DIR").unwrap_or_else(|_| "migrations".to_string());
    let name = std::env::args().nth(1).unwrap_or_else(|| "auto".to_string());

    let db = Database::connect(&url, 1).await?;
    let change = generate(&db, &model_schemas()).await?;

    if change.is_empty() {
        println!("Schema is up to date; nothing to generate.");
        return Ok(());
    }

    match write_migration(Path::new(&dir), &name, &change)? {
        Some(path) => println!("Wrote {}", path.display()),
        None => println!("No changes."),
    }
    Ok(())
}
