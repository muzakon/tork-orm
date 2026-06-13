//! Domain enumerations, stored as text via `#[derive(DbEnum)]`. The default
//! `snake_case` rendering makes each variant store as its lowercase name, matching
//! the values an equivalent SQLAlchemy `Enum(native_enum=False)` would use.

use tork_orm::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum UserRole {
    Customer,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum UserStatus {
    Active,
    Passive,
    Banned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum VendorStatus {
    Pending,
    Active,
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum VendorMemberRole {
    Owner,
    Manager,
    Staff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum AddressType {
    Shipping,
    Billing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum ProductStatus {
    Draft,
    Active,
    Archived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum OrderStatus {
    Pending,
    Confirmed,
    Processing,
    Shipped,
    Delivered,
    Cancelled,
    Refunded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum PaymentStatus {
    Pending,
    Authorized,
    Paid,
    Failed,
    Cancelled,
    Refunded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum PaymentProvider {
    Stripe,
    Iyzico,
    Paypal,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum PaymentMethod {
    Card,
    BankTransfer,
    CashOnDelivery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum ShipmentStatus {
    Pending,
    Preparing,
    Shipped,
    Delivered,
    Failed,
    Returned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum DiscountType {
    Percentage,
    FixedAmount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum CouponStatus {
    Active,
    Passive,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum InventoryMovementType {
    Purchase,
    Sale,
    Return,
    Adjustment,
    Reservation,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
pub enum ReturnStatus {
    Requested,
    Approved,
    Rejected,
    Received,
    Refunded,
}
