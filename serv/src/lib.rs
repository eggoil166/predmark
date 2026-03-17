use spacetimedb::{table, Identity, SpacetimeType, Timestamp};

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketStatus {
    Open,
    Closed,
    Resolved,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderStatus {
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Yes,
    No,
}

#[table(name = users, public)]
pub struct User {
    #[primary_key]
    pub id: Identity,

    pub username: String,
    pub created_at: Timestamp,
}

#[table(name = balances, public)]
pub struct Balance {
    #[primary_key]
    pub user_id: Identity,

    pub balance: i64,
    pub locked_balance: i64,
    pub updated_at: Timestamp,
}

#[table(name = events, public)]
pub struct Event {
    #[primary_key]
    pub id: u64,

    pub title: String,
    pub description: String,

    pub created_by: Identity,
    pub created_at: Timestamp,
}

#[table(name = markets, public)]
pub struct Market {
    #[primary_key]
    pub id: u64,

    pub event_id: u64,

    pub question: String,
    pub description: String,

    pub status: MarketStatus,

    pub close_time: Timestamp,
    pub resolve_time: Option<Timestamp>,

    pub resolution: Option<Outcome>,

    pub created_by: Identity,
    pub created_at: Timestamp,
}

#[table(name = orders, public)]
pub struct Order {
    #[primary_key]
    pub id: u64,

    pub user_id: Identity,
    pub market_id: u64,

    pub outcome: Outcome,
    pub side: OrderSide,

    // price = probability * 100
    pub price: u32,

    pub quantity: u64,
    pub filled: u64,

    pub status: OrderStatus,

    pub created_at: Timestamp,
}

#[table(name = trades, public)]
pub struct Trade {
    #[primary_key]
    pub id: u64,

    pub market_id: u64,
    pub outcome: Outcome,

    pub price: u32,
    pub quantity: u64,

    pub buyer_id: Identity,
    pub seller_id: Identity,

    pub buy_order_id: u64,
    pub sell_order_id: u64,

    pub timestamp: Timestamp,
}

#[table(name = orderbook_levels, public)]
pub struct OrderBookLevel {
    #[primary_key]
    pub id: u64,

    pub market_id: u64,

    pub outcome: Outcome,
    pub side: OrderSide,

    pub price: u32,
    pub total_quantity: u64,
}

#[table(name = positions, public)]
pub struct Position {
    #[primary_key]
    pub id: u64,

    pub user_id: Identity,
    pub market_id: u64,

    pub outcome: Outcome,

    pub shares: i64,
    pub average_price: u32,

    pub updated_at: Timestamp,
}

#[table(name = price_history, public)]
pub struct PricePoint {
    #[primary_key]
    pub id: u64,

    pub market_id: u64,
    pub outcome: Outcome,

    pub price: u32,
    pub volume: u64,

    pub timestamp: Timestamp,
}

#[table(name = market_stats, public)]
pub struct MarketStats {
    #[primary_key]
    pub market_id: u64,

    pub last_price: u32,
    pub total_volume: u64,
    pub open_interest: u64,

    pub updated_at: Timestamp,
}

#[table(name = user_stats, public)]
pub struct UserStats {
    #[primary_key]
    pub user_id: Identity,

    pub portfolio_value: i64,
    pub realized_pnl: i64,
    pub total_volume: u64,

    pub updated_at: Timestamp,
}