use sqlx::{PgPool, Postgres, Row, Transaction};

use crate::domain::{Order, OrderBook, PriceType, Side, Status, Trade};

fn side_str(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn price_opt(price: PriceType) -> Option<i64> {
    match price {
        PriceType::Limit(p) => Some(p),
        PriceType::Market => None,
    }
}

fn status_str(order: &Order) -> String {
    format!("{:?}", order.status)
}

pub async fn insert_order(
    tx: &mut Transaction<'_, Postgres>,
    order: &Order,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO orders (id, side, price, qty, filled, status, user_id, timestamp)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(order.id as i64)
    .bind(side_str(order.side))
    .bind(price_opt(order.price))
    .bind(order.qty)
    .bind(order.filled)
    .bind(status_str(order))
    .bind(order.user_id as i64)
    .bind(order.timestamp as i64)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn update_order(
    tx: &mut Transaction<'_, Postgres>,
    order: &Order,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE orders SET filled = $1, status = $2 WHERE id = $3")
        .bind(order.filled)
        .bind(status_str(order))
        .bind(order.id as i64)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn insert_trade(
    tx: &mut Transaction<'_, Postgres>,
    trade: &Trade,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trades (id, price, qty, buy_order_id, sell_order_id, timestamp)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(trade.id as i64)
    .bind(trade.price)
    .bind(trade.qty)
    .bind(trade.buy_order_id as i64)
    .bind(trade.sell_order_id as i64)
    .bind(trade.timestamp as i64)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn parse_side(s: &str) -> Side {
    match s {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => panic!("unknown side {s}"),
    }
}

fn parse_status(s: &str) -> Status {
    match s {
        "Unfilled" => Status::Unfilled,
        "Partial" => Status::Partial,
        "Filled" => Status::Filled,
        "Cancelled" => Status::Cancelled,
        _ => panic!("unknown status {s}"),
    }
}

pub async fn get_idempotent_response(
    tx: &mut Transaction<'_, Postgres>,
    key: &str,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let row = sqlx::query("SELECT response FROM idempotency_keys WHERE key = $1")
        .bind(key)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(row.map(|r| {
        let v: serde_json::Value = r.get("response");
        v
    }))
}

pub async fn store_idempotent_response(
    tx: &mut Transaction<'_, Postgres>,
    key: &str,
    response: &serde_json::Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO idempotency_keys (key, response) VALUES ($1, $2)")
        .bind(key)
        .bind(response)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn load_resting_book(pool: &PgPool) -> Result<(OrderBook, u64, u64), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, side, price, qty, filled, status, user_id, timestamp
         FROM orders WHERE status IN ('Unfilled', 'Partial') ORDER BY id",
    )
    .fetch_all(pool)
    .await?;

    let mut book = OrderBook::new();
    let mut max_order_id: Option<i64> = None;

    for row in rows {
        let id: i64 = row.get("id");
        let side_s: String = row.get("side");
        let price: Option<i64> = row.get("price");
        let qty: f64 = row.get("qty");
        let filled: f64 = row.get("filled");
        let status_s: String = row.get("status");
        let user_id: i64 = row.get("user_id");
        let timestamp: i64 = row.get("timestamp");

        let order = Order {
            id: id as u64,
            user_id: user_id as u64,
            side: parse_side(&side_s),
            price: match price {
                Some(p) => PriceType::Limit(p),
                None => PriceType::Market,
            },
            qty,
            filled,
            status: parse_status(&status_s),
            timestamp: timestamp as u64,
        };

        max_order_id = Some(max_order_id.map_or(id, |m| m.max(id)));

        if let PriceType::Limit(p) = order.price {
            let side_map = match order.side {
                Side::Buy => &mut book.bids,
                Side::Sell => &mut book.asks,
            };
            side_map.entry(p).or_default().push_back(order);
        }
    }

    let max_order: Option<i64> =
        sqlx::query_scalar("SELECT MAX(id) FROM orders")
            .fetch_one(pool)
            .await?;
    let max_trade: Option<i64> =
        sqlx::query_scalar("SELECT MAX(id) FROM trades")
            .fetch_one(pool)
            .await?;

    let next_order_id = max_order.map(|v| (v + 1) as u64).unwrap_or(0);
    let next_trade_id = max_trade.map(|v| (v + 1) as u64).unwrap_or(0);

    // Ensure next ids are at least past any resting id seen
    let next_order_id = max_order_id
        .map(|m| ((m + 1) as u64).max(next_order_id))
        .unwrap_or(next_order_id);

    Ok((book, next_order_id, next_trade_id))
}

pub async fn list_trades(pool: &PgPool, limit: i64) -> Result<Vec<Trade>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, price, qty, buy_order_id, sell_order_id, timestamp
         FROM trades ORDER BY id DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| Trade {
            id: r.get::<i64, _>("id") as u64,
            price: r.get("price"),
            qty: r.get("qty"),
            buy_order_id: r.get::<i64, _>("buy_order_id") as u64,
            sell_order_id: r.get::<i64, _>("sell_order_id") as u64,
            timestamp: r.get::<i64, _>("timestamp") as u64,
        })
        .collect())
}