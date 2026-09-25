#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceType {
    Limit(i64),
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Unfilled,
    Partial,
    Filled,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: u64,
    pub user_id: u64,
    pub side: Side,
    pub price: PriceType,
    pub qty: f64,
    pub filled: f64,
    pub status: Status,
    pub timestamp: u64,
}

impl Order {
    pub fn remaining(&self) -> f64 {
        self.qty - self.filled
    }
}