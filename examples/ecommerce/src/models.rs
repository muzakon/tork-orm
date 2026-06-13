//! The e-commerce domain models.
//!
//! Mapped from a SQLAlchemy schema. Conventions:
//! - Money is stored as `i64` minor units (cents); fields are suffixed `_cents`.
//! - `created_at`/`updated_at`/`deleted_at` are managed by the ORM lifecycle
//!   attributes; soft-deletable tables carry `deleted_at`.
//! - Enums are text-backed via `#[derive(DbEnum)]` (see `enums.rs`).
//! - Foreign-key actions (`on_delete`), CHECK constraints (`#[table(check = ...)]`),
//!   and composite-unique indexes are declared here, so the migration generated from
//!   these models is production-correct.

use serde_json::Value as Json;
use time::OffsetDateTime;
use tork_orm::prelude::*;

use crate::enums::*;

// ============================================================
// User / Address
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "users")]
pub struct User {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(varchar(length = 255), unique)]
    pub email: String,
    #[field(varchar(length = 32), unique)]
    pub phone: Option<String>,
    pub password_hash: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    #[field(db_enum)]
    pub role: UserRole,
    #[field(db_enum)]
    pub status: UserStatus,
    pub email_verified_at: Option<OffsetDateTime>,
    pub last_login_at: Option<OffsetDateTime>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
    #[field(deleted_at)]
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Model)]
#[table(name = "addresses")]
pub struct Address {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = User::id, on_delete = "cascade")]
    pub user_id: i64,
    #[field(db_enum)]
    pub address_type: AddressType,
    pub title: Option<String>,
    pub country_code: String,
    pub city: String,
    pub district: Option<String>,
    pub postal_code: Option<String>,
    pub line1: String,
    pub line2: Option<String>,
    pub recipient_name: String,
    pub recipient_phone: String,
    pub is_default: bool,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

// ============================================================
// Vendor / Brand / Category
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "vendors")]
pub struct Vendor {
    #[field(primary_key, auto)]
    pub id: i64,
    pub name: String,
    #[field(varchar(length = 180), unique)]
    pub slug: String,
    #[field(db_enum)]
    pub status: VendorStatus,
    pub legal_name: Option<String>,
    pub tax_number: Option<String>,
    pub contact_email: Option<String>,
    pub contact_phone: Option<String>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
    #[field(deleted_at)]
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Model)]
#[table(name = "vendor_members", indexes = [
    unique(name = "uq_vendor_member", fields = [vendor_id, user_id]),
])]
pub struct VendorMember {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Vendor::id, on_delete = "cascade")]
    pub vendor_id: i64,
    #[field(foreign_key = User::id, on_delete = "cascade")]
    pub user_id: i64,
    #[field(db_enum)]
    pub role: VendorMemberRole,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "brands")]
pub struct Brand {
    #[field(primary_key, auto)]
    pub id: i64,
    pub name: String,
    #[field(varchar(length = 180), unique)]
    pub slug: String,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "categories")]
pub struct Category {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Category::id, on_delete = "set_null")]
    pub parent_id: Option<i64>,
    pub name: String,
    #[field(varchar(length = 180), unique)]
    pub slug: String,
    pub description: Option<String>,
    pub sort_order: i32,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

// ============================================================
// Product / Variant / Image
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "products", indexes = [
    unique(name = "uq_products_vendor_slug", fields = [vendor_id, slug]),
])]
pub struct Product {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Vendor::id, on_delete = "cascade")]
    pub vendor_id: i64,
    #[field(foreign_key = Brand::id, on_delete = "set_null")]
    pub brand_id: Option<i64>,
    pub title: String,
    pub slug: String,
    pub short_description: Option<String>,
    pub description: Option<String>,
    #[field(db_enum)]
    pub status: ProductStatus,
    pub seo_title: Option<String>,
    pub seo_description: Option<String>,
    pub extra_data: Option<Json>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
    #[field(deleted_at)]
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Model)]
#[table(name = "product_categories", indexes = [
    unique(name = "uq_product_category", fields = [product_id, category_id]),
])]
pub struct ProductCategory {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Product::id, on_delete = "cascade")]
    pub product_id: i64,
    #[field(foreign_key = Category::id, on_delete = "cascade")]
    pub category_id: i64,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Model)]
#[table(name = "product_variants", check = "price_cents >= 0")]
pub struct ProductVariant {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Product::id, on_delete = "cascade")]
    pub product_id: i64,
    #[field(varchar(length = 100), unique)]
    pub sku: String,
    #[field(varchar(length = 100), unique)]
    pub barcode: Option<String>,
    pub title: Option<String>,
    pub price_cents: i64,
    pub compare_at_price_cents: Option<i64>,
    pub cost_price_cents: Option<i64>,
    #[field(varchar(length = 3))]
    pub currency: String,
    pub weight_grams: Option<i32>,
    pub dimensions: Option<Json>,
    pub attributes: Option<Json>,
    pub is_active: bool,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
    #[field(deleted_at)]
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Model)]
#[table(name = "product_images")]
pub struct ProductImage {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Product::id, on_delete = "cascade")]
    pub product_id: i64,
    #[field(foreign_key = ProductVariant::id, on_delete = "set_null")]
    pub variant_id: Option<i64>,
    pub url: String,
    pub alt_text: Option<String>,
    pub sort_order: i32,
    pub is_primary: bool,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

// ============================================================
// Inventory
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "inventory_locations")]
pub struct InventoryLocation {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Vendor::id, on_delete = "cascade")]
    pub vendor_id: i64,
    pub name: String,
    pub country_code: String,
    pub city: String,
    pub address_line: Option<String>,
    pub is_active: bool,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "inventory_items",
    check = "quantity_on_hand >= 0",
    check = "quantity_reserved >= 0",
    indexes = [unique(name = "uq_inventory_variant_location", fields = [variant_id, location_id])],
)]
pub struct InventoryItem {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = ProductVariant::id, on_delete = "cascade")]
    pub variant_id: i64,
    #[field(foreign_key = InventoryLocation::id, on_delete = "cascade")]
    pub location_id: i64,
    pub quantity_on_hand: i32,
    pub quantity_reserved: i32,
    pub reorder_level: i32,
    /// Optimistic-lock guard against concurrent stock updates.
    #[field(version)]
    pub version: i64,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "inventory_movements", check = "quantity <> 0")]
pub struct InventoryMovement {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = InventoryItem::id, on_delete = "cascade")]
    pub inventory_item_id: i64,
    #[field(db_enum)]
    pub movement_type: InventoryMovementType,
    pub quantity: i32,
    pub reason: Option<String>,
    pub reference_type: Option<String>,
    pub reference_id: Option<i64>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

// ============================================================
// Cart
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "carts")]
pub struct Cart {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = User::id, on_delete = "cascade")]
    pub user_id: Option<i64>,
    pub session_id: Option<String>,
    #[field(varchar(length = 3))]
    pub currency: String,
    pub is_active: bool,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "cart_items",
    check = "quantity > 0",
    indexes = [unique(name = "uq_cart_variant", fields = [cart_id, variant_id])],
)]
pub struct CartItem {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Cart::id, on_delete = "cascade")]
    pub cart_id: i64,
    #[field(foreign_key = ProductVariant::id, on_delete = "restrict")]
    pub variant_id: i64,
    pub quantity: i32,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

// ============================================================
// Coupon
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "coupons", check = "discount_value >= 0")]
pub struct Coupon {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Vendor::id, on_delete = "cascade")]
    pub vendor_id: Option<i64>,
    #[field(varchar(length = 64), unique)]
    pub code: String,
    #[field(db_enum)]
    pub status: CouponStatus,
    #[field(db_enum)]
    pub discount_type: DiscountType,
    /// For `FixedAmount`, minor units (cents). For `Percentage`, basis points
    /// (1000 = 10.00%).
    pub discount_value: i64,
    pub max_discount_amount_cents: Option<i64>,
    pub min_order_amount_cents: Option<i64>,
    pub usage_limit: Option<i32>,
    pub usage_count: i32,
    pub per_user_limit: Option<i32>,
    pub starts_at: Option<OffsetDateTime>,
    pub ends_at: Option<OffsetDateTime>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

// ============================================================
// Order
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "orders",
    check = "subtotal_cents >= 0",
    check = "grand_total_cents >= 0",
)]
pub struct Order {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = User::id, on_delete = "set_null")]
    pub user_id: Option<i64>,
    #[field(foreign_key = Coupon::id, on_delete = "set_null")]
    pub coupon_id: Option<i64>,
    #[field(varchar(length = 64), unique)]
    pub order_number: String,
    #[field(db_enum)]
    pub status: OrderStatus,
    #[field(varchar(length = 3))]
    pub currency: String,
    pub subtotal_cents: i64,
    pub discount_total_cents: i64,
    pub tax_total_cents: i64,
    pub shipping_total_cents: i64,
    pub grand_total_cents: i64,
    pub customer_email: String,
    pub customer_phone: Option<String>,
    pub placed_at: Option<OffsetDateTime>,
    pub cancelled_at: Option<OffsetDateTime>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "order_items",
    check = "quantity > 0",
    check = "unit_price_cents >= 0",
)]
pub struct OrderItem {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Order::id, on_delete = "cascade")]
    pub order_id: i64,
    #[field(foreign_key = Vendor::id, on_delete = "restrict")]
    pub vendor_id: i64,
    #[field(foreign_key = Product::id, on_delete = "restrict")]
    pub product_id: i64,
    #[field(foreign_key = ProductVariant::id, on_delete = "restrict")]
    pub variant_id: i64,
    pub sku: String,
    pub product_title: String,
    pub variant_title: Option<String>,
    pub quantity: i32,
    pub unit_price_cents: i64,
    pub discount_total_cents: i64,
    pub tax_total_cents: i64,
    pub line_total_cents: i64,
    pub attributes_snapshot: Option<Json>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "order_addresses", indexes = [
    unique(name = "uq_order_address_type", fields = [order_id, address_type]),
])]
pub struct OrderAddress {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Order::id, on_delete = "cascade")]
    pub order_id: i64,
    #[field(db_enum)]
    pub address_type: AddressType,
    pub recipient_name: String,
    pub recipient_phone: String,
    pub country_code: String,
    pub city: String,
    pub district: Option<String>,
    pub postal_code: Option<String>,
    pub line1: String,
    pub line2: Option<String>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "coupon_redemptions", indexes = [
    unique(name = "uq_coupon_order_redemption", fields = [coupon_id, order_id]),
])]
pub struct CouponRedemption {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Coupon::id, on_delete = "cascade")]
    pub coupon_id: i64,
    #[field(foreign_key = Order::id, on_delete = "cascade")]
    pub order_id: i64,
    #[field(foreign_key = User::id, on_delete = "set_null")]
    pub user_id: Option<i64>,
    pub discount_amount_cents: i64,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

// ============================================================
// Payment / Shipment
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "payments", check = "amount_cents >= 0")]
pub struct Payment {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Order::id, on_delete = "cascade")]
    pub order_id: i64,
    #[field(db_enum)]
    pub provider: PaymentProvider,
    #[field(db_enum)]
    pub method: PaymentMethod,
    #[field(db_enum)]
    pub status: PaymentStatus,
    pub amount_cents: i64,
    #[field(varchar(length = 3))]
    pub currency: String,
    pub transaction_id: Option<String>,
    pub provider_reference: Option<String>,
    pub raw_response: Option<Json>,
    pub paid_at: Option<OffsetDateTime>,
    pub failed_at: Option<OffsetDateTime>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "shipments")]
pub struct Shipment {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Order::id, on_delete = "cascade")]
    pub order_id: i64,
    #[field(foreign_key = Vendor::id, on_delete = "restrict")]
    pub vendor_id: i64,
    #[field(db_enum)]
    pub status: ShipmentStatus,
    pub carrier: Option<String>,
    pub tracking_number: Option<String>,
    pub tracking_url: Option<String>,
    pub shipped_at: Option<OffsetDateTime>,
    pub delivered_at: Option<OffsetDateTime>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "shipment_items",
    check = "quantity > 0",
    indexes = [unique(name = "uq_shipment_order_item", fields = [shipment_id, order_item_id])],
)]
pub struct ShipmentItem {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Shipment::id, on_delete = "cascade")]
    pub shipment_id: i64,
    #[field(foreign_key = OrderItem::id, on_delete = "cascade")]
    pub order_item_id: i64,
    pub quantity: i32,
}

// ============================================================
// Return / Refund
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "return_requests")]
pub struct ReturnRequest {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Order::id, on_delete = "cascade")]
    pub order_id: i64,
    #[field(foreign_key = User::id, on_delete = "set_null")]
    pub user_id: Option<i64>,
    #[field(db_enum)]
    pub status: ReturnStatus,
    pub reason: Option<String>,
    #[field(created_at)]
    pub requested_at: OffsetDateTime,
    pub resolved_at: Option<OffsetDateTime>,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Model)]
#[table(name = "return_items",
    check = "quantity > 0",
    indexes = [unique(name = "uq_return_order_item", fields = [return_request_id, order_item_id])],
)]
pub struct ReturnItem {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = ReturnRequest::id, on_delete = "cascade")]
    pub return_request_id: i64,
    #[field(foreign_key = OrderItem::id, on_delete = "cascade")]
    pub order_item_id: i64,
    pub quantity: i32,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Model)]
#[table(name = "refunds", check = "amount_cents >= 0")]
pub struct Refund {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = Order::id, on_delete = "cascade")]
    pub order_id: i64,
    #[field(foreign_key = Payment::id, on_delete = "set_null")]
    pub payment_id: Option<i64>,
    pub amount_cents: i64,
    #[field(varchar(length = 3))]
    pub currency: String,
    pub reason: Option<String>,
    pub provider_reference: Option<String>,
    pub refunded_at: Option<OffsetDateTime>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
}

// ============================================================
// Reviews / Audit
// ============================================================

#[derive(Debug, Clone, Model)]
#[table(name = "reviews",
    check = "rating >= 1 AND rating <= 5",
    indexes = [unique(name = "uq_user_product_order_review", fields = [user_id, product_id, order_item_id])],
)]
pub struct Review {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = User::id, on_delete = "cascade")]
    pub user_id: i64,
    #[field(foreign_key = Product::id, on_delete = "cascade")]
    pub product_id: i64,
    #[field(foreign_key = OrderItem::id, on_delete = "set_null")]
    pub order_item_id: Option<i64>,
    pub rating: i32,
    pub title: Option<String>,
    pub comment: Option<String>,
    pub is_verified_purchase: bool,
    pub is_published: bool,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
    #[field(updated_at)]
    pub updated_at: OffsetDateTime,
    #[field(deleted_at)]
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Model)]
#[table(name = "audit_logs")]
pub struct AuditLog {
    #[field(primary_key, auto)]
    pub id: i64,
    #[field(foreign_key = User::id, on_delete = "set_null")]
    pub actor_user_id: Option<i64>,
    pub entity_type: String,
    pub entity_id: Option<i64>,
    pub action: String,
    pub before_data: Option<Json>,
    pub after_data: Option<Json>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    #[field(created_at)]
    pub created_at: OffsetDateTime,
}

// ============================================================
// Relations (a representative set used by the tests)
// ============================================================

#[relations]
impl User {
    #[has_many(Order, foreign_key = Order::user_id)]
    pub fn orders() {}
    #[has_many(Address, foreign_key = Address::user_id)]
    pub fn addresses() {}
    #[has_many(Review, foreign_key = Review::user_id)]
    pub fn reviews() {}
}

#[relations]
impl Order {
    #[belongs_to(User, foreign_key = Order::user_id)]
    pub fn customer() {}
    #[has_many(OrderItem, foreign_key = OrderItem::order_id)]
    pub fn items() {}
    #[has_many(Payment, foreign_key = Payment::order_id)]
    pub fn payments() {}
    #[has_many(Shipment, foreign_key = Shipment::order_id)]
    pub fn shipments() {}
}

#[relations]
impl Vendor {
    #[has_many(Product, foreign_key = Product::vendor_id)]
    pub fn products() {}
}

#[relations]
impl Product {
    #[belongs_to(Vendor, foreign_key = Product::vendor_id)]
    pub fn vendor() {}
    #[has_many(ProductVariant, foreign_key = ProductVariant::product_id)]
    pub fn variants() {}
    #[has_many(Review, foreign_key = Review::product_id)]
    pub fn reviews() {}
}

#[relations]
impl ProductVariant {
    #[belongs_to(Product, foreign_key = ProductVariant::product_id)]
    pub fn product() {}
    #[has_many(InventoryItem, foreign_key = InventoryItem::variant_id)]
    pub fn inventory_items() {}
}
