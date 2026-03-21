use spacetimedb::{Identity, ReducerContext, Timestamp, reducer, Table};
mod enums;
use crate::enums::*;
mod tables;
use crate::tables::*;

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
/// Formula: (no_reserves * 100) / (yes_reserves + no_reserves)
/// Uses fixed-point integer arithmetic for determinism
/// Note: Used for reference pricing only. Actual trade prices determined by CPMM curve.
fn get_current_price(yes_reserves: i64, no_reserves: i64) -> u32 {
    if yes_reserves + no_reserves == 0 {
        return 50;
    }
    let total = (yes_reserves as i128) + (no_reserves as i128);
    let numerator = (no_reserves as i128) * 100i128;
    (numerator / total) as u32
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

    if calculate_buy_output(result, r_yes, r_no, k) < desired_shares {
        result + 1
    } else {
        result
    }
}

#[reducer]
pub fn create_market(
    ctx: &ReducerContext,
    question: String,
    description: String,
    close_time: Timestamp,
) {
    if !is_admin(ctx, ctx.sender) {
        return;
    }

    let yes_reserves = 1_000_000i64;
    let no_reserves = 1_000_000i64;
    let invariant_k = (yes_reserves as i128) * (no_reserves as i128);
    let collateral = (yes_reserves + no_reserves) * 100i64;

    let market = ctx.db.markets().insert(Market {
        id: 0,
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
        collateral,
    });

    ctx.db.market_stats().insert(MarketStats {
        market_id: market.id,
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

    let mut market = match ctx.db.markets().id().find(&market_id) {
        Some(m) => m.clone(),
        None => return,
    };

    if market.status != MarketStatus::Open || amount <= 0 {
        return;
    }

    let mut balance = match ctx.db.balances().user_id().find(&user_id) {
        Some(b) => b.clone(),
        None => return,
    };

    if balance.balance < amount {
        return;
    }
    let output_shares = calculate_buy_output(
        amount,
        market.yes_reserves,
        market.no_reserves,
        market.invariant_k,
    );

    if output_shares <= 0 {
        return;
    }
    if outcome == Outcome::Yes {
        market.no_reserves += amount;
        market.yes_reserves -= output_shares;
    } else {
        market.yes_reserves += amount;
        market.no_reserves -= output_shares;
    }

    // Verify reserves remain positive
    if market.yes_reserves < 0 || market.no_reserves < 0 {
        return;  // Trade would make reserves negative; abort
    }
    market.invariant_k = (market.yes_reserves as i128) * (market.no_reserves as i128);
    market.collateral += amount;
    ctx.db.markets().delete(market.clone());
    ctx.db.markets().insert(market.clone());
    balance.balance -= amount;
    balance.updated_at = ctx.timestamp;
    ctx.db.balances().delete(balance.clone());
    ctx.db.balances().insert(balance);

    let position = ctx
        .db
        .positions()
        .user_id()
        .filter(&user_id)
        .find(|p| p.market_id == market_id && p.outcome == outcome);

    let trade_price_paid = if output_shares > 0 {
        ((amount as i128 * 100i128) / (output_shares as i128)) as u32
    } else {
        0
    };

    if let Some(mut pos) = position {  
        let total_value = (pos.shares * pos.average_price as i64) + (output_shares * trade_price_paid as i64);
        let total_shares = pos.shares + output_shares;
        pos.average_price = if total_shares > 0 {
            (total_value / total_shares) as u32
        } else {
            trade_price_paid
        };
        pos.shares = total_shares;
        pos.updated_at = ctx.timestamp;
        ctx.db.positions().delete(pos.clone());
        ctx.db.positions().insert(pos);
    } else {
        ctx.db.positions().insert(Position {
            id: 0,
            user_id,
            market_id,
            outcome,
            shares: output_shares,
            average_price: trade_price_paid,
            updated_at: ctx.timestamp,
        });
    }

    let trade_price = get_current_price(market.yes_reserves, market.no_reserves);
    ctx.db.trades().insert(Trade {
        id: 0,
        market_id,
        outcome,
        price: trade_price,
        quantity: output_shares,
        buyer_id: user_id,
        seller_id: Identity::default(),
        timestamp: ctx.timestamp,
    });

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

    let current_price = get_current_price(market.yes_reserves, market.no_reserves);
    user_stats.total_volume += output_shares as u64;
    user_stats.portfolio_value += output_shares as i64 * current_price as i64;
    user_stats.updated_at = ctx.timestamp;
    ctx.db.user_stats().delete(user_stats.clone());
    ctx.db.user_stats().insert(user_stats);

    ctx.db.price_history().insert(PricePoint {
        id: 0,
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

    let mut market = match ctx.db.markets().id().find(&market_id) {
        Some(m) => m.clone(),
        None => return,
    };

    if market.status != MarketStatus::Open || shares <= 0 {
        return;
    }

    let position = ctx
        .db
        .positions()
        .user_id()
        .filter(&user_id)
        .find(|p| p.market_id == market_id && p.outcome == outcome);

    let position = match position {
        Some(p) => p,
        None => return,
    };

    if position.shares < shares {
        return;
    }

    // CRITICAL: Use CPMM formula to calculate token output with proper slippage
    // Selling works as the inverse: remove shares from outcome, add to complementary outcome
    // This ensures price impact - selling large amounts gives worse prices
    let token_output = if outcome == Outcome::Yes {
        // Selling YES: reduces YES reserves, adds to NO
        // Use CPMM: (yes - shares) * (no + output) = k
        // Solving: output = k / (yes - shares) - no
        let new_yes_i128 = (market.yes_reserves - shares) as i128;
        if new_yes_i128 <= 0 {
            return;  // Would drain the pool
        }
        
        let k = market.invariant_k;
        let new_no_i128 = k / new_yes_i128;
        let output = (new_no_i128 - (market.no_reserves as i128)) as i64;
        
        if output <= 0 {
            return;  // No tokens received
        }
        output
    } else {
        // Selling NO: reduces NO reserves, adds to YES
        let new_no_i128 = (market.no_reserves - shares) as i128;
        if new_no_i128 <= 0 {
            return;
        }
        
        let k = market.invariant_k;
        let new_yes_i128 = k / new_no_i128;
        let output = (new_yes_i128 - (market.yes_reserves as i128)) as i64;
        
        if output <= 0 {
            return;
        }
        output
    };

    if outcome == Outcome::Yes {
        market.yes_reserves -= shares;
        market.no_reserves += token_output;
    } else {
        market.no_reserves -= shares;
        market.yes_reserves += token_output;
    }

    if market.yes_reserves < 0 || market.no_reserves < 0 {
        return;
    }
    market.invariant_k = (market.yes_reserves as i128) * (market.no_reserves as i128);
    market.collateral -= token_output;
    if market.collateral < 0 {
        market.collateral = 0;
    }

    ctx.db.markets().delete(market.clone());
    ctx.db.markets().insert(market.clone());
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

    let mut updated_position = position.clone();
    let position_avg_price = position.average_price;
    updated_position.shares -= shares;
    updated_position.updated_at = ctx.timestamp;

    if updated_position.shares > 0 {
        ctx.db.positions().delete(updated_position.clone());
        ctx.db.positions().insert(updated_position);
    } else {
        ctx.db.positions().delete(position);
    }

    let trade_price = get_current_price(market.yes_reserves, market.no_reserves);
    ctx.db.trades().insert(Trade {
        id: 0,
        market_id,
        outcome,
        price: trade_price,
        quantity: shares,
        buyer_id: Identity::default(),
        seller_id: user_id,
        timestamp: ctx.timestamp,
    });
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

    ctx.db.price_history().insert(PricePoint {
        id: 0,
        market_id,
        outcome,
        price: current_price,
        volume: shares as u64,
        timestamp: ctx.timestamp,
    });
}

#[reducer]
pub fn mark_market_resolved(
    ctx: &ReducerContext,
    market_id: u64,
    outcome: Outcome,
) {
    if !is_admin(ctx, ctx.sender) {
        return;
    }
    let mut market = match ctx.db.markets().id().find(&market_id) {
        Some(m) => m.clone(),
        None => return,
    };

    if market.status != MarketStatus::Open {
        return;
    }

    market.status = MarketStatus::Resolved;
    market.resolution = Some(outcome);
    market.resolve_time = Some(ctx.timestamp);
    ctx.db.markets().delete(market.clone());
    ctx.db.markets().insert(market);

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

#[reducer]
pub fn claim_payout(
    ctx: &ReducerContext,
    market_id: u64,
) {
    let user_id = ctx.sender;

    let market = match ctx.db.markets().id().find(&market_id) {
        Some(m) => m.clone(),
        None => return,
    };

    if market.status != MarketStatus::Resolved {
        return;
    }

    let outcome = match market.resolution {
        Some(o) => o,
        None => return,
    };

    let position_query: Vec<_> = ctx
        .db
        .positions()
        .user_id()
        .filter(&user_id)
        .filter(|p| p.market_id == market_id)
        .collect();

    let position = match position_query.first() {
        Some(p) => p.clone(),
        None => return,
    };

    let payout = if position.outcome == outcome {
        position.shares * 100
    } else {
        0
    };

    if payout > 0 {
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

        balance.balance += payout;
        balance.updated_at = ctx.timestamp;
        ctx.db.balances().delete(balance.clone());
        ctx.db.balances().insert(balance);
    }

    ctx.db.positions().delete(position);
}