use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::domain::{MatchResult, PriceType, Side};
use crate::exchange::Exchange;

#[derive(Debug)]
pub enum ServiceError {
    InvalidSide(String),
    Internal(String),
}

pub type SharedExchange = Arc<Mutex<Exchange>>;

#[derive(Serialize, Deserialize)]
pub struct OrderResponse {
    pub order_id: u64,
    pub status: String,
    pub remaining: f64,
    pub trades: Vec<TradeResponse>,
}

#[derive(Serialize, Deserialize)]
pub struct TradeResponse {
    pub price: i64,
    pub qty: f64,
    pub buy_order_id: u64,
    pub sell_order_id: u64,
}

pub fn parse_side(raw: &str) -> Result<Side, ServiceError> {
    match raw {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        other => Err(ServiceError::InvalidSide(other.to_string())),
    }
}

fn result_to_response(result: MatchResult) -> OrderResponse {
    OrderResponse {
        order_id: result.order.id,
        status: format!("{:?}", result.order.status),
        remaining: result.order.remaining(),
        trades: result
            .trades
            .into_iter()
            .map(|t| TradeResponse {
                price: t.price,
                qty: t.qty,
                buy_order_id: t.buy_order_id,
                sell_order_id: t.sell_order_id,
            })
            .collect(),
    }
}

pub async fn place_order_service(
    exchange: &SharedExchange,
    db: &PgPool,
    side_str: &str,
    price: Option<i64>,
    qty: f64,
    idem_key: Option<String>,
) -> Result<OrderResponse, ServiceError> {
    if let Some(ref key) = idem_key {
        let mut tx = db.begin().await.map_err(|e| ServiceError::Internal(e.to_string()))?;
        if let Some(cached) = crate::db::get_idempotent_response(&mut tx, key)
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?
        {
            tx.commit().await.map_err(|e| ServiceError::Internal(e.to_string()))?;
            let resp: OrderResponse = serde_json::from_value(cached)
                .map_err(|e| ServiceError::Internal(e.to_string()))?;
            return Ok(resp);
        }
        tx.commit().await.map_err(|e| ServiceError::Internal(e.to_string()))?;
    }

    let side = parse_side(side_str)?;
    let domain_price = match price {
        Some(p) => PriceType::Limit(p),
        None => PriceType::Market,
    };

    let result = exchange.lock().unwrap().place_order(side, domain_price, qty);
    let response = result_to_response(MatchResult {
        trades: result.trades.clone(),
        order: result.order.clone(),
        touched_orders: result.touched_orders.clone(),
    });
    let response_json = serde_json::to_value(&response).map_err(|e| ServiceError::Internal(e.to_string()))?;

    let mut tx = db.begin().await.map_err(|e| ServiceError::Internal(e.to_string()))?;
    if let Some(ref key) = idem_key {
        if let Some(cached) = crate::db::get_idempotent_response(&mut tx, key)
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?
        {
            tx.commit().await.map_err(|e| ServiceError::Internal(e.to_string()))?;
            let resp: OrderResponse = serde_json::from_value(cached)
                .map_err(|e| ServiceError::Internal(e.to_string()))?;
            return Ok(resp);
        }
    }
    crate::db::insert_order(&mut tx, &result.order).await.map_err(|e| ServiceError::Internal(e.to_string()))?;
    for trade in &result.trades {
        crate::db::insert_trade(&mut tx, trade).await.map_err(|e| ServiceError::Internal(e.to_string()))?;
    }
    for touched in &result.touched_orders {
        crate::db::update_order(&mut tx, touched).await.map_err(|e| ServiceError::Internal(e.to_string()))?;
    }
    if let Some(ref key) = idem_key {
        crate::db::store_idempotent_response(&mut tx, key, &response_json)
            .await
            .map_err(|e| {
                if e.to_string().contains("duplicate key") {
                    ServiceError::Internal("concurrent duplicate idempotency key".to_string())
                } else {
                    ServiceError::Internal(e.to_string())
                }
            })?;
    }
    tx.commit().await.map_err(|e| ServiceError::Internal(e.to_string()))?;
    Ok(response)
}