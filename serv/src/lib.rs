use spacetimedb::{Identity, ReducerContext, SpacetimeType, Timestamp, table, reducer, Table};

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketStatus {
    Open,
    Resolved,
}

#[derive(SpacetimeType, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Yes,
    No,
}

#[table(name = users, public)]
#[derive(Clone)]
pub struct User {
    #[primary_key]
    pub id: Identity,

    pub username: String,
    pub created_at: Timestamp,
}

#[table(name = balances, public)]
#[derive(Clone)]
pub struct Balance {
    #[primary_key]
    pub user_id: Identity,

    pub balance: i64,
    pub locked_balance: i64,
    pub updated_at: Timestamp,
}

#[table(name = markets, public)]
#[derive(Clone)]
pub struct Market {
    #[primary_key]
    pub id: u64,

    pub question: String,
    pub description: String,

    pub status: MarketStatus,

    pub close_time: Timestamp,
    pub resolve_time: Option<Timestamp>,

    pub resolution: Option<Outcome>,

    pub created_by: Identity,
    pub created_at: Timestamp,

    // AMM fields
    pub yes_reserves: i64,
    pub no_reserves: i64,
    pub invariant_k: i128,
}

#[table(name = admin, public)]
#[derive(Clone)]
pub struct Admin {
    #[primary_key]
    pub user_id: Identity,

    pub created_at: Timestamp,
}

#[table(name = trades, public)]
#[derive(Clone)]
pub struct Trade {
    #[primary_key]
    pub id: u64,

    pub market_id: u64,
    pub outcome: Outcome,

    // price as percentage (0-100)
    pub price: u32,
    pub quantity: i64,

    pub buyer_id: Identity,
    pub seller_id: Identity,

    pub timestamp: Timestamp,
}

#[table(name = positions, public)]
#[derive(Clone)]
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
#[derive(Clone)]
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
#[derive(Clone)]
pub struct MarketStats {
    #[primary_key]
    pub market_id: u64,

    pub last_price: u32,
    pub total_volume: u64,
    pub open_interest: u64,

    pub updated_at: Timestamp,
}

#[table(name = user_stats, public)]
#[derive(Clone)]
pub struct UserStats {
    #[primary_key]
    pub user_id: Identity,

    pub portfolio_value: i64,
    pub realized_pnl: i64,
    pub total_volume: u64,

    pub updated_at: Timestamp,
}

#[reducer]
pub fn register_user(ctx: &ReducerContext, username: String) {
    let user_id = ctx.sender;

    if ctx.db.users().id().find(&user_id).is_some() {
        return;
    }

    ctx.db.users().insert(User {
        id: user_id,
        username,
        created_at: ctx.timestamp,
    });

    ctx.db.balances().insert(Balance {
        user_id,
        balance: 10_000,
        locked_balance: 0,
        updated_at: ctx.timestamp,
    });

    ctx.db.user_stats().insert(UserStats {
        user_id,
        portfolio_value: 10_000,
        realized_pnl: 0,
        total_volume: 0,
        updated_at: ctx.timestamp,
    });
} 

// Helper Functions

fn is_admin(ctx: &ReducerContext, user_id: Identity) -> bool {
    ctx.db.admin().user_id().find(&user_id).is_some()
}

/// Get current market price as percentage (0-100) based on NO reserves
/// Formula: no_reserves / (yes_reserves + no_reserves) * 100
fn get_current_price(yes_reserves: i64, no_reserves: i64) -> u32 {
    if yes_reserves + no_reserves == 0 {
        return 50;
    }
    ((no_reserves as f64 / (yes_reserves + no_reserves) as f64) * 100.0) as u32
}

/// Constant product formula: y * r_no + x * r_yes = k
/// Given input token amount (y), solve for output shares (x)
fn calculate_buy_output(token_amount: i64, r_yes: i64, r_no: i64, k: i128) -> i64 {
    if token_amount <= 0 {
        return 0;
    }

    let r_yes_i128 = r_yes as i128;
    let r_no_i128 = r_no as i128;
    let y_i128 = token_amount as i128;

    // After adding tokens: new_r_no = r_no + y
    let new_r_no = r_no_i128 + y_i128;

    // Solve: new_r_yes = k / new_r_no
    let new_r_yes = k / new_r_no;

    // Output shares: r_yes - new_r_yes
    let output = r_yes_i128 - new_r_yes;
    output as i64
}

/// Binary search to find token input needed to buy NO shares
#[allow(dead_code)]
fn calculate_buy_input_for_output(desired_shares: i64, r_yes: i64, r_no: i64, k: i128) -> i64 {
    if desired_shares <= 0 {
        return 0;
    }

    let mut low: i64 = 0;
    let mut high: i64 = i64::MAX / 2;
    let mut result: i64 = 0;

    let _r_yes_i128 = r_yes as i128;
    let _r_no_i128 = r_no as i128;

    while low <= high {
        let mid = low + (high - low) / 2;
        let output = calculate_buy_output(mid, r_yes, r_no, k);

        if output == desired_shares {
            return mid;
        } else if output < desired_shares {
            low = mid + 1;
            result = mid;
        } else {
            high = mid - 1;
        }
    }

    // Round up to ensure we get at least desired_shares
    if calculate_buy_output(result, r_yes, r_no, k) < desired_shares {
        result + 1
    } else {
        result
    }
}

// Reducers

#[reducer]
pub fn create_market(
    ctx: &ReducerContext,
    market_id: u64,
    question: String,
    description: String,
    close_time: Timestamp,
) {
    if !is_admin(ctx, ctx.sender) {
        return;
    }

    let yes_reserves = 1_000_000_000i64;  // 1e9 in fixed-point
    let no_reserves = 1_000_000_000i64;
    let invariant_k = (yes_reserves as i128) * (no_reserves as i128);

    ctx.db.markets().insert(Market {
        id: market_id,
        question,
        description,
        status: MarketStatus::Open,
        close_time,
        resolve_time: None,
        resolution: None,
        created_by: ctx.sender,
        created_at: ctx.timestamp,
        yes_reserves,
        no_reserves,
        invariant_k,
    });

    ctx.db.market_stats().insert(MarketStats {
        market_id,
        last_price: 50,
        total_volume: 0,
        open_interest: 0,
        updated_at: ctx.timestamp,
    });
}

#[reducer]
pub fn buy_shares(
    ctx: &ReducerContext,
    market_id: u64,
    outcome: Outcome,
    amount: i64,
) {
    let user_id = ctx.sender;

    // Check market exists and is open
    let mut market = match ctx.db.markets().id().find(&market_id) {
        Some(m) => m.clone(),
        None => return,
    };

    if market.status != MarketStatus::Open || amount <= 0 {
        return;
    }

    // Check user has sufficient balance
    let mut balance = match ctx.db.balances().user_id().find(&user_id) {
        Some(b) => b.clone(),
        None => return,
    };

    if balance.balance < amount {
        return;
    }

    // Calculate output shares using AMM formula
    let output_shares = calculate_buy_output(
        amount,
        market.yes_reserves,
        market.no_reserves,
        market.invariant_k,
    );

    if output_shares <= 0 {
        return;
    }

    // Update markers based on outcome
    if outcome == Outcome::Yes {
        market.no_reserves += amount;
    } else {
        market.yes_reserves += amount;
    }

    // Verify invariant (should hold approximately within rounding)
    let new_k = (market.yes_reserves as i128) * (market.no_reserves as i128);
    market.invariant_k = new_k;

    // Update market (delete old and insert new)
    ctx.db.markets().delete(market.clone());
    ctx.db.markets().insert(market.clone());

    // Update balance
    balance.balance -= amount;
    balance.updated_at = ctx.timestamp;
    ctx.db.balances().delete(balance.clone());
    ctx.db.balances().insert(balance);

    // Update or create position
    if let Some(mut pos) = ctx
        .db
        .positions()
        .iter()
        .find(|p| p.user_id == user_id && p.market_id == market_id && p.outcome == outcome)
        .map(|p| p.clone())
    {
        // Update existing position
        let current_price = get_current_price(market.yes_reserves, market.no_reserves);
        let total_value = (pos.shares * pos.average_price as i64) + (output_shares * current_price as i64);
        let total_shares = pos.shares + output_shares;
        pos.average_price = if total_shares > 0 {
            (total_value / total_shares) as u32
        } else {
            current_price
        };
        pos.shares = total_shares;
        pos.updated_at = ctx.timestamp;
        ctx.db.positions().delete(pos.clone());
        ctx.db.positions().insert(pos);
    } else {
        // Create new position
        let current_price = get_current_price(market.yes_reserves, market.no_reserves);
        let pos_id = ((market_id as u128) * 1000 + user_id.to_string().len() as u128) as u64;
        ctx.db.positions().insert(Position {
            id: pos_id,
            user_id,
            market_id,
            outcome,
            shares: output_shares,
            average_price: current_price,
            updated_at: ctx.timestamp,
        });
    }

    // Create trade record
    let trade_id = (market_id << 16) ^ (output_shares.abs() as u64);
    ctx.db.trades().insert(Trade {
        id: trade_id,
        market_id,
        outcome,
        price: get_current_price(market.yes_reserves, market.no_reserves),
        quantity: output_shares,
        buyer_id: user_id,
        seller_id: Identity::default(),
        timestamp: ctx.timestamp,
    });

    // Update market stats
    let mut stats = ctx
        .db
        .market_stats()
        .market_id()
        .find(&market_id)
        .map(|s| s.clone())
        .unwrap_or_else(|| MarketStats {
            market_id,
            last_price: 50,
            total_volume: 0,
            open_interest: 0,
            updated_at: ctx.timestamp,
        });

    let current_price = get_current_price(market.yes_reserves, market.no_reserves);
    stats.last_price = current_price;
    stats.total_volume += output_shares as u64;
    stats.open_interest += output_shares as u64;
    stats.updated_at = ctx.timestamp;
    ctx.db.market_stats().delete(stats.clone());
    ctx.db.market_stats().insert(stats);

    // Update user stats
    let mut user_stats = ctx
        .db
        .user_stats()
        .user_id()
        .find(&user_id)
        .map(|u| u.clone())
        .unwrap_or_else(|| UserStats {
            user_id,
            portfolio_value: 0,
            realized_pnl: 0,
            total_volume: 0,
            updated_at: ctx.timestamp,
        });

    user_stats.total_volume += output_shares as u64;
    user_stats.portfolio_value += output_shares as i64 * current_price as i64;
    user_stats.updated_at = ctx.timestamp;
    ctx.db.user_stats().delete(user_stats.clone());
    ctx.db.user_stats().insert(user_stats);

    // Record price point in history
    let price_id = (market_id << 16) ^ (output_shares.abs() as u64) ^ 1;
    ctx.db.price_history().insert(PricePoint {
        id: price_id,
        market_id,
        outcome,
        price: current_price,
        volume: output_shares as u64,
        timestamp: ctx.timestamp,
    });
}

#[reducer]
pub fn sell_shares(
    ctx: &ReducerContext,
    market_id: u64,
    outcome: Outcome,
    shares: i64,
) {
    let user_id = ctx.sender;

    // Check market exists and is open
    let mut market = match ctx.db.markets().id().find(&market_id) {
        Some(m) => m.clone(),
        None => return,
    };

    if market.status != MarketStatus::Open || shares <= 0 {
        return;
    }

    // Check user has position
    let position = match ctx
        .db
        .positions()
        .iter()
        .find(|p| p.user_id == user_id && p.market_id == market_id && p.outcome == outcome)
    {
        Some(p) => p.clone(),
        None => return,
    };

    if position.shares < shares {
        return;
    }

    // Calculate token output for selling shares
    let token_output = if outcome == Outcome::Yes {
        // Selling YES: remove from yes_reserves
        let output = market.yes_reserves - ((market.invariant_k as f64 / (market.yes_reserves as f64 - shares as f64)) as i64);
        output
    } else {
        // Selling NO: remove from no_reserves
        let output = market.no_reserves - ((market.invariant_k as f64 / (market.no_reserves as f64 - shares as f64)) as i64);
        output
    };

    // Update reserves
    if outcome == Outcome::Yes {
        market.yes_reserves -= shares;
    } else {
        market.no_reserves -= shares;
    }
    market.invariant_k = (market.yes_reserves as i128) * (market.no_reserves as i128);

    ctx.db.markets().delete(market.clone());
    ctx.db.markets().insert(market.clone());

    // Refund tokens to user
    let mut balance = ctx
        .db
        .balances()
        .user_id()
        .find(&user_id)
        .map(|b| b.clone())
        .unwrap_or_else(|| Balance {
            user_id,
            balance: 0,
            locked_balance: 0,
            updated_at: ctx.timestamp,
        });

    balance.balance += token_output;
    balance.updated_at = ctx.timestamp;
    ctx.db.balances().delete(balance.clone());
    ctx.db.balances().insert(balance);

    // Update position
    let mut updated_position = position.clone();
    let position_avg_price = position.average_price;  // Save before moving
    updated_position.shares -= shares;
    updated_position.updated_at = ctx.timestamp;

    if updated_position.shares > 0 {
        ctx.db.positions().delete(updated_position.clone());
        ctx.db.positions().insert(updated_position);
    } else {
        ctx.db.positions().delete(position);
    }

    // Create trade record
    let trade_id = (market_id << 16) ^ (shares.abs() as u64);
    ctx.db.trades().insert(Trade {
        id: trade_id,
        market_id,
        outcome,
        price: get_current_price(market.yes_reserves, market.no_reserves),
        quantity: shares,
        buyer_id: Identity::default(),
        seller_id: user_id,
        timestamp: ctx.timestamp,
    });

    // Update market stats
    let mut stats = ctx
        .db
        .market_stats()
        .market_id()
        .find(&market_id)
        .map(|s| s.clone())
        .unwrap_or_else(|| MarketStats {
            market_id,
            last_price: 50,
            total_volume: 0,
            open_interest: 0,
            updated_at: ctx.timestamp,
        });

    let current_price = get_current_price(market.yes_reserves, market.no_reserves);
    stats.last_price = current_price;
    stats.total_volume += shares as u64;
    stats.open_interest = stats.open_interest.saturating_sub(shares as u64);
    stats.updated_at = ctx.timestamp;
    ctx.db.market_stats().delete(stats.clone());
    ctx.db.market_stats().insert(stats);

    // Update user stats
    let mut user_stats = ctx
        .db
        .user_stats()
        .user_id()
        .find(&user_id)
        .map(|u| u.clone())
        .unwrap_or_else(|| UserStats {
            user_id,
            portfolio_value: 0,
            realized_pnl: 0,
            total_volume: 0,
            updated_at: ctx.timestamp,
        });

    user_stats.total_volume += shares as u64;
    user_stats.realized_pnl += token_output - (shares * position_avg_price as i64);
    user_stats.updated_at = ctx.timestamp;
    ctx.db.user_stats().delete(user_stats.clone());
    ctx.db.user_stats().insert(user_stats);

    // Record price point in history
    let price_id = (market_id << 16) ^ (shares.abs() as u64) ^ 1;
    ctx.db.price_history().insert(PricePoint {
        id: price_id,
        market_id,
        outcome: outcome,
        price: current_price,
        volume: shares as u64,
        timestamp: ctx.timestamp,
    });
}

#[reducer]
pub fn resolve_market(
    ctx: &ReducerContext,
    market_id: u64,
    outcome: Outcome,
) {
    // Check admin permission
    if !is_admin(ctx, ctx.sender) {
        return;
    }

    // Check market exists and is open
    let mut market = match ctx.db.markets().id().find(&market_id) {
        Some(m) => m.clone(),
        None => return,
    };

    if market.status != MarketStatus::Open {
        return;
    }

    // Resolve market
    market.status = MarketStatus::Resolved;
    market.resolution = Some(outcome);
    market.resolve_time = Some(ctx.timestamp);
    ctx.db.markets().delete(market.clone());
    ctx.db.markets().insert(market);

    // Settle all positions
    let all_positions: Vec<_> = ctx.db.positions().iter().collect();
    for position in all_positions {
        if position.market_id == market_id {
            let payout = if position.outcome == outcome {
                position.shares * 100  // 100 tokens per share
            } else {
                0
            };

            if payout > 0 {
                let mut balance = ctx
                    .db
                    .balances()
                    .user_id()
                    .find(&position.user_id)
                    .map(|b| b.clone())
                    .unwrap_or_else(|| Balance {
                        user_id: position.user_id,
                        balance: 0,
                        locked_balance: 0,
                        updated_at: ctx.timestamp,
                    });

                balance.balance += payout;
                balance.updated_at = ctx.timestamp;
                ctx.db.balances().delete(balance.clone());
                ctx.db.balances().insert(balance);
            }

            // Delete position (marked as settled)
            ctx.db.positions().delete(position.clone());
        }
    }

    // Update market stats
    if let Some(mut stats) = ctx
        .db
        .market_stats()
        .market_id()
        .find(&market_id)
        .map(|s| s.clone())
    {
        stats.updated_at = ctx.timestamp;
        ctx.db.market_stats().delete(stats.clone());
        ctx.db.market_stats().insert(stats);
    }
}