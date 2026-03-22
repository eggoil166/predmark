use spacetimedb::{Identity, Timestamp, table};
use crate::enums::*;

#[table(accessor = users, public)]
#[derive(Clone)]
pub struct User {
    #[primary_key]
    pub id: Identity,

    pub username: String,
    pub created_at: Timestamp,
}

#[table(accessor = balances, public)]
#[derive(Clone)]
pub struct Balance {
    #[primary_key]
    pub user_id: Identity,

    pub balance: i64,
    pub locked_balance: i64,
    pub updated_at: Timestamp,
}

#[table(accessor = markets, public)]
#[derive(Clone)]
pub struct Market {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    pub question: String,
    pub description: String,

    pub status: MarketStatus,

    pub close_time: Timestamp,
    pub resolve_time: Option<Timestamp>,

    pub resolution: Option<Outcome>,

    pub created_by: Identity,
    pub created_at: Timestamp,

    // AMM fields - represent the house's inventory of shares
    pub yes_reserves: i64,
    pub no_reserves: i64,
    pub invariant_k: i128,
    
    // CRITICAL: Collateral is the total tokens locked to pay out shares at resolution
    // Invariant: collateral must be sufficient to pay all potential outcomes
    pub collateral: i64,
}

#[table(accessor = admin, public)]
#[derive(Clone)]
pub struct Admin {
    #[primary_key]
    pub user_id: Identity,

    pub created_at: Timestamp,
}

#[table(accessor = trades, public)]
#[derive(Clone)]
pub struct Trade {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    #[index(btree)]
    pub market_id: u64,
    pub outcome: Outcome,

    // price as percentage (0-100)
    pub price: u32,
    pub quantity: i64,

    pub buyer_id: Identity,
    pub seller_id: Identity,

    pub timestamp: Timestamp,
}

#[table(accessor = positions, public)]
#[derive(Clone)]
pub struct Position {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    #[index(btree)]
    pub user_id: Identity,
    #[index(btree)]
    pub market_id: u64,

    pub outcome: Outcome,

    pub shares: i64,
    pub average_price: u32,

    pub updated_at: Timestamp,
}

#[table(accessor = price_history, public)]
#[derive(Clone)]
pub struct PricePoint {
    #[primary_key]
    #[auto_inc]
    pub id: u64,

    #[index(btree)]
    pub market_id: u64,
    pub outcome: Outcome,

    pub price: u32,
    pub volume: u64,

    pub timestamp: Timestamp,
}

#[table(accessor = market_stats, public)]
#[derive(Clone)]
pub struct MarketStats {
    #[primary_key]
    pub market_id: u64,

    pub last_price: u32,
    pub total_volume: u64,
    pub open_interest: u64,

    pub updated_at: Timestamp,
}

#[table(accessor = user_stats, public)]
#[derive(Clone)]
pub struct UserStats {
    #[primary_key]
    pub user_id: Identity,

    pub portfolio_value: i64,
    pub realized_pnl: i64,
    pub total_volume: u64,

    pub updated_at: Timestamp,
}