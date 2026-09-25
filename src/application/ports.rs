use async_trait::async_trait;

use crate::domain::{Order, OrderBook, Trade};

#[async_trait]
pub trait OrderRepository: Send + Sync {
    async fn save_placement(
        &self,
        order: &Order,
        trades: &[Trade],
        touched: &[Order],
    ) -> Result<(), sqlx::Error>;

    async fn update_cancelled(&self, order: &Order) -> Result<(), sqlx::Error>;

    async fn load_resting_book(&self) -> Result<(OrderBook, u64, u64), sqlx::Error>;

    async fn list_trades(&self, limit: i64) -> Result<Vec<Trade>, sqlx::Error>;
}

#[async_trait]
pub trait IdempotencyStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, sqlx::Error>;
    async fn store(&self, key: &str, response: &serde_json::Value) -> Result<(), sqlx::Error>;
}