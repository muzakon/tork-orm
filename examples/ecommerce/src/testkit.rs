//! Test helpers: apply the generated migrations to a fresh database and build
//! valid domain rows. Used by the production-readiness tests in `tests/`.

use time::OffsetDateTime;
use tork_orm::migration::FileMigrator;
use tork_orm::prelude::*;

use crate::enums::*;
use crate::models::*;

/// A placeholder for database-filled timestamp columns (`created_at`/`updated_at`).
pub const EPOCH: OffsetDateTime = OffsetDateTime::UNIX_EPOCH;

/// Connects to `url` and applies the committed SQL migrations, returning a ready
/// database. Use `":memory:"` for isolated unit tests or a file URL for
/// concurrency tests (a file allows more than one pooled connection).
pub async fn migrated(url: &str, pool: u32) -> Result<Database> {
    let db = Database::connect(url, pool).await?;
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/migrations");
    FileMigrator::new(db.clone(), dir).up().await?;
    Ok(db)
}

pub fn user(email: &str) -> User {
    User {
        id: 0,
        email: email.into(),
        phone: None,
        password_hash: "hash".into(),
        first_name: None,
        last_name: None,
        role: UserRole::Customer,
        status: UserStatus::Active,
        email_verified_at: None,
        last_login_at: None,
        created_at: EPOCH,
        updated_at: EPOCH,
        deleted_at: None,
    }
}

pub fn vendor(slug: &str) -> Vendor {
    Vendor {
        id: 0,
        name: slug.into(),
        slug: slug.into(),
        status: VendorStatus::Active,
        legal_name: None,
        tax_number: None,
        contact_email: None,
        contact_phone: None,
        created_at: EPOCH,
        updated_at: EPOCH,
        deleted_at: None,
    }
}

pub fn product(vendor_id: i64, slug: &str) -> Product {
    Product {
        id: 0,
        vendor_id,
        brand_id: None,
        title: slug.into(),
        slug: slug.into(),
        short_description: None,
        description: None,
        status: ProductStatus::Active,
        seo_title: None,
        seo_description: None,
        extra_data: None,
        created_at: EPOCH,
        updated_at: EPOCH,
        deleted_at: None,
    }
}

pub fn variant(product_id: i64, sku: &str, price_cents: i64) -> ProductVariant {
    ProductVariant {
        id: 0,
        product_id,
        sku: sku.into(),
        barcode: None,
        title: None,
        price_cents,
        compare_at_price_cents: None,
        cost_price_cents: None,
        currency: "USD".into(),
        weight_grams: None,
        dimensions: None,
        attributes: None,
        is_active: true,
        created_at: EPOCH,
        updated_at: EPOCH,
        deleted_at: None,
    }
}

pub fn location(vendor_id: i64) -> InventoryLocation {
    InventoryLocation {
        id: 0,
        vendor_id,
        name: "main".into(),
        country_code: "US".into(),
        city: "NYC".into(),
        address_line: None,
        is_active: true,
        created_at: EPOCH,
        updated_at: EPOCH,
    }
}

pub fn inventory(variant_id: i64, location_id: i64, on_hand: i32) -> InventoryItem {
    InventoryItem {
        id: 0,
        variant_id,
        location_id,
        quantity_on_hand: on_hand,
        quantity_reserved: 0,
        reorder_level: 0,
        version: 1,
        created_at: EPOCH,
        updated_at: EPOCH,
    }
}

pub fn order(user_id: i64, number: &str, grand_total_cents: i64) -> Order {
    Order {
        id: 0,
        user_id: Some(user_id),
        coupon_id: None,
        order_number: number.into(),
        status: OrderStatus::Pending,
        currency: "USD".into(),
        subtotal_cents: grand_total_cents,
        discount_total_cents: 0,
        tax_total_cents: 0,
        shipping_total_cents: 0,
        grand_total_cents,
        customer_email: "c@x.com".into(),
        customer_phone: None,
        placed_at: None,
        cancelled_at: None,
        created_at: EPOCH,
        updated_at: EPOCH,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn order_item(
    order_id: i64,
    vendor_id: i64,
    product_id: i64,
    variant_id: i64,
    quantity: i32,
    unit_price_cents: i64,
) -> OrderItem {
    OrderItem {
        id: 0,
        order_id,
        vendor_id,
        product_id,
        variant_id,
        sku: "sku".into(),
        product_title: "p".into(),
        variant_title: None,
        quantity,
        unit_price_cents,
        discount_total_cents: 0,
        tax_total_cents: 0,
        line_total_cents: unit_price_cents * quantity as i64,
        attributes_snapshot: None,
        created_at: EPOCH,
        updated_at: EPOCH,
    }
}

pub fn payment(order_id: i64, amount_cents: i64) -> Payment {
    Payment {
        id: 0,
        order_id,
        provider: PaymentProvider::Manual,
        method: PaymentMethod::Card,
        status: PaymentStatus::Paid,
        amount_cents,
        currency: "USD".into(),
        transaction_id: None,
        provider_reference: None,
        raw_response: None,
        paid_at: None,
        failed_at: None,
        created_at: EPOCH,
        updated_at: EPOCH,
    }
}

pub fn cart(user_id: i64) -> Cart {
    Cart {
        id: 0,
        user_id: Some(user_id),
        session_id: None,
        currency: "USD".into(),
        is_active: true,
        created_at: EPOCH,
        updated_at: EPOCH,
    }
}

pub fn cart_item(cart_id: i64, variant_id: i64, quantity: i32) -> CartItem {
    CartItem { id: 0, cart_id, variant_id, quantity, created_at: EPOCH, updated_at: EPOCH }
}

pub fn address(user_id: i64) -> Address {
    Address {
        id: 0,
        user_id,
        address_type: AddressType::Shipping,
        title: None,
        country_code: "US".into(),
        city: "NYC".into(),
        district: None,
        postal_code: None,
        line1: "1 Main St".into(),
        line2: None,
        recipient_name: "R".into(),
        recipient_phone: "555".into(),
        is_default: true,
        created_at: EPOCH,
        updated_at: EPOCH,
    }
}

pub fn review(user_id: i64, product_id: i64, rating: i32) -> Review {
    Review {
        id: 0,
        user_id,
        product_id,
        order_item_id: None,
        rating,
        title: None,
        comment: None,
        is_verified_purchase: false,
        is_published: true,
        created_at: EPOCH,
        updated_at: EPOCH,
        deleted_at: None,
    }
}

/// A minimal consistent dataset: one vendor, product, variant, location, and an
/// inventory item with `on_hand` units, plus one customer. Returns their ids.
pub struct Seed {
    pub user_id: i64,
    pub vendor_id: i64,
    pub product_id: i64,
    pub variant_id: i64,
    pub location_id: i64,
    pub inventory_id: i64,
}

pub async fn seed(db: &Database, on_hand: i32) -> Result<Seed> {
    let u = User::create(db, &user("buyer@x.com")).await?;
    let v = Vendor::create(db, &vendor("acme")).await?;
    let p = Product::create(db, &product(v.id, "widget")).await?;
    let var = ProductVariant::create(db, &variant(p.id, "ACME-1", 1_999)).await?;
    let loc = InventoryLocation::create(db, &location(v.id)).await?;
    let inv = InventoryItem::create(db, &inventory(var.id, loc.id, on_hand)).await?;
    Ok(Seed {
        user_id: u.id,
        vendor_id: v.id,
        product_id: p.id,
        variant_id: var.id,
        location_id: loc.id,
        inventory_id: inv.id,
    })
}
