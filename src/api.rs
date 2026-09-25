use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::domain::{MatchResult, OrderBook, PriceType, Side};
use crate::exchange::Exchange;

pub type SharedExchange = Arc<Mutex<Exchange>>;

#[derive(Clone)]
pub struct AppState {
    pub exchange: SharedExchange,
    pub db: PgPool,
    pub demo_running: Arc<AtomicBool>,
}

#[derive(Serialize)]
pub struct Level {
    pub price: i64,
    pub qty: f64,
    pub orders: usize,
}

#[derive(Serialize)]
pub struct BookView {
    pub bids: Vec<Level>,
    pub asks: Vec<Level>,
}

fn levels_for(book: &OrderBook, side: Side) -> Vec<Level> {
    let map = match side {
        Side::Buy => &book.bids,
        Side::Sell => &book.asks,
    };
    map.iter()
        .map(|(&price, queue)| Level {
            price,
            qty: queue.iter().map(|o| o.remaining()).sum(),
            orders: queue.len(),
        })
        .collect()
}

pub async fn get_book(State(app): State<AppState>) -> Json<BookView> {
    let guard = app.exchange.lock().unwrap();
    let book = guard.book();
    let view = BookView {
        bids: levels_for(book, Side::Buy),
        asks: levels_for(book, Side::Sell),
    };
    Json(view)
}

#[derive(Deserialize)]
pub struct OrderRequest {
    pub side: String,
    pub price: Option<i64>,
    pub qty: f64,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

pub use crate::application::service::{OrderResponse, TradeResponse};

pub enum ApiError {
    BadRequest(String),
    NotFound(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let (status, msg) = match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, msg).into_response()
    }
}

fn map_service_error(e: crate::application::service::ServiceError) -> ApiError {
    match e {
        crate::application::service::ServiceError::InvalidSide(s) => {
            ApiError::BadRequest(format!("invalid side: {s}"))
        }
        crate::application::service::ServiceError::Internal(s) => ApiError::Internal(s),
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

pub async fn post_order(
    State(app): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<OrderRequest>,
) -> Result<Json<OrderResponse>, ApiError> {
    let idem_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or(req.idempotency_key.clone());
    let resp = crate::application::service::place_order_service(
        &app.exchange,
        &app.db,
        &req.side,
        req.price,
        req.qty,
        idem_key,
    )
    .await
    .map_err(map_service_error)?;
    Ok(Json(resp))
}

#[derive(Deserialize)]
pub struct TradesQuery {
    pub limit: Option<i64>,
}

pub async fn get_trades(
    State(app): State<AppState>,
    Query(q): Query<TradesQuery>,
) -> Result<Json<Vec<TradeResponse>>, ApiError> {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    let trades = crate::db::list_trades(&app.db, limit)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(
        trades
            .into_iter()
            .map(|t| TradeResponse {
                price: t.price,
                qty: t.qty,
                buy_order_id: t.buy_order_id,
                sell_order_id: t.sell_order_id,
            })
            .collect(),
    ))
}

pub async fn demo_start(State(app): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    if app.demo_running.swap(true, Ordering::SeqCst) {
        return Err(ApiError::BadRequest("demo already running".to_string()));
    }
    let app_clone = app.clone();
    tokio::spawn(async move {
        let mut rng: u64 = 42;
        while app_clone.demo_running.load(Ordering::SeqCst) {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let side = if rng % 2 == 0 { Side::Buy } else { Side::Sell };
            let price = 9700 + (rng % 700) as i64; // 9700..10400 around mid 10000
            let qty = 0.5 + ((rng % 4) as f64) * 0.5; // 0.5..2.0
            let result = app_clone
                .exchange
                .lock()
                .unwrap()
                .place_order(side, PriceType::Limit(price), qty);
            let response = result_to_response(crate::domain::MatchResult {
                trades: result.trades.clone(),
                order: result.order.clone(),
                touched_orders: result.touched_orders.clone(),
            });
            let response_json = match serde_json::to_value(&response) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let mut tx = match app_clone.db.begin().await {
                Ok(tx) => tx,
                Err(_) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                    continue;
                }
            };
            let _ = crate::db::insert_order(&mut tx, &result.order).await;
            for t in &result.trades {
                let _ = crate::db::insert_trade(&mut tx, t).await;
            }
            for touched in &result.touched_orders {
                let _ = crate::db::update_order(&mut tx, touched).await;
            }
            let _ = tx.commit().await;
            let _ = response_json;
            tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
        }
    });
    Ok(Json(json!({"status":"demo started"})))
}

pub async fn demo_stop(State(app): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    if !app.demo_running.swap(false, Ordering::SeqCst) {
        return Err(ApiError::BadRequest("demo not running".to_string()));
    }
    Ok(Json(json!({"status":"demo stopped"})))
}

pub async fn cancel_order(
    State(app): State<AppState>,
    Path(order_id): Path<u64>,
) -> Result<Json<OrderResponse>, ApiError> {
    let cancelled = app.exchange.lock().unwrap().cancel_order(order_id);
    match cancelled {
        Some(order) => {
            let mut tx = app
                .db
                .begin()
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            crate::db::update_order(&mut tx, &order)
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            tx.commit()
                .await
                .map_err(|e| ApiError::Internal(e.to_string()))?;
            Ok(Json(result_to_response(MatchResult {
                trades: Vec::new(),
                order,
                touched_orders: Vec::new(),
            })))
        }
        None => Err(ApiError::NotFound(format!("order {order_id} not found"))),
    }
}
