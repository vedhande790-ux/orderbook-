#[derive(Debug, Clone)]
pub struct Trade {
    pub id: u64,
    pub buy_order_id: u64,
    pub sell_order_id: u64,
    pub price: i64,
    pub qty: f64,
    pub timestamp: u64,
}