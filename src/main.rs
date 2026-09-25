use std::sync::{
    atomic::AtomicBool,
    Arc, Mutex,
};

use axum::response::Html;
use axum::routing::{delete, get, post};
use axum::Router;
use sqlx::PgPool;

use simulated_orderbook::api::{cancel_order, demo_start, demo_stop, get_book, get_trades, post_order, AppState};
use simulated_orderbook::domain::{Order, PriceType, Side, Status};
use simulated_orderbook::exchange::Exchange;

async fn index() -> Html<&'static str> {
    Html(include_str!("../order-book-terminal.html"))
}

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| {
        eprintln!(
            "DATABASE_URL is not set. Example:\n  \
             $env:DATABASE_URL='postgres://postgres:<password>@127.0.0.1:5432/simulated_orderbook'"
        );
        std::process::exit(1);
    });

    let db = PgPool::connect(&database_url).await.unwrap();
    sqlx::migrate!("./migrations").run(&db).await.unwrap();
    println!("database connected + migrations applied");

    let (mut book, mut next_order_id, next_trade_id) =
        simulated_orderbook::db::load_resting_book(&db).await.unwrap();
    println!(
        "recovered book: {} bid levels, {} ask levels, next_order_id={}, next_trade_id={}",
        book.bids.len(),
        book.asks.len(),
        next_order_id,
        next_trade_id
    );

    if book.bids.is_empty() && book.asks.is_empty() {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders")
            .fetch_one(&db)
            .await
            .unwrap();
        if count == 0 {
            println!("seeding book with mock orders...");
            let seed = vec![
                (Side::Buy, 9500),
                (Side::Buy, 9600),
                (Side::Buy, 9700),
                (Side::Buy, 9800),
                (Side::Buy, 9900),
                (Side::Sell, 10100),
                (Side::Sell, 10200),
                (Side::Sell, 10300),
                (Side::Sell, 10400),
                (Side::Sell, 10500),
            ];
            let mut tx = db.begin().await.unwrap();
            for (side, price) in seed {
                let order = Order {
                    id: next_order_id,
                    user_id: 0,
                    side,
                    price: PriceType::Limit(price),
                    qty: 1.5 + (price % 3) as f64 * 0.5,
                    filled: 0.0,
                    status: Status::Unfilled,
                    timestamp: next_order_id,
                };
                sqlx::query(
                    "INSERT INTO orders (id, side, price, qty, filled, status, user_id, timestamp)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                )
                .bind(order.id as i64)
                .bind(match side {
                    Side::Buy => "buy",
                    Side::Sell => "sell",
                })
                .bind(price)
                .bind(order.qty)
                .bind(order.filled)
                .bind(format!("{:?}", order.status))
                .bind(order.user_id as i64)
                .bind(order.timestamp as i64)
                .execute(&mut *tx)
                .await
                .unwrap();
                let map = match side {
                    Side::Buy => &mut book.bids,
                    Side::Sell => &mut book.asks,
                };
                map.entry(price).or_default().push_back(order);
                next_order_id += 1;
            }
            tx.commit().await.unwrap();
            println!("seeded {} orders, next_order_id={}", 10, next_order_id);
        }
    }

    let state = AppState {
        exchange: Arc::new(Mutex::new(Exchange::from_book(
            book, next_order_id, next_trade_id,
        ))),
        db,
        demo_running: Arc::new(AtomicBool::new(false)),
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/book", get(get_book))
        .route("/api/trades", get(get_trades))
        .route("/api/orders", post(post_order))
        .route("/api/orders/{id}", delete(cancel_order))
        .route("/api/demo/start", post(demo_start))
        .route("/api/demo/stop", post(demo_stop))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("listening on http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}