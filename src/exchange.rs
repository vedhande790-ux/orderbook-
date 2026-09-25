use crate::domain::{match_order, MatchResult, Order, OrderBook, PriceType, Side, Status};

pub struct Exchange {
    book: OrderBook,
    next_order_id: u64,
    next_trade_id: u64,
}

impl Exchange {
    pub fn new() -> Self {
        Exchange {
            book: OrderBook::new(),
            next_order_id: 0,
            next_trade_id: 0,
        }
    }

    pub fn from_book(book: OrderBook, next_order_id: u64, next_trade_id: u64) -> Self {
        Exchange {
            book,
            next_order_id,
            next_trade_id,
        }
    }

    pub fn place_order(&mut self, side: Side, price: PriceType, qty: f64) -> MatchResult {
        let order = Order {
            id: self.next_order_id,
            user_id: 0,
            side,
            price,
            qty,
            filled: 0.0,
            status: Status::Unfilled,
            timestamp: self.next_order_id,
        };
        self.next_order_id += 1;

        let mut result = match_order(&mut self.book, order);

        for trade in &mut result.trades {
            trade.id = self.next_trade_id;
            self.next_trade_id += 1;
        }

        result
    }

    pub fn cancel_order(&mut self, order_id: u64) -> Option<Order> {
        self.book.cancel(order_id)
    }

    pub fn best_bid(&self) -> Option<i64> {
        self.book.bids.last_key_value().map(|(&price, _)| price)
    }

    pub fn best_ask(&self) -> Option<i64> {
        self.book.asks.first_key_value().map(|(&price, _)| price)
    }

    pub fn book(&self) -> &OrderBook {
        &self.book
    }
}

impl Default for Exchange {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn place_order_assigns_order_and_trade_ids() {
        let mut ex = Exchange::new();

        let sell = ex.place_order(Side::Sell, PriceType::Limit(10000), 2.0);
        assert_eq!(sell.order.id, 0);
        assert_eq!(sell.order.status, Status::Unfilled);

        let buy = ex.place_order(Side::Buy, PriceType::Market, 2.0);
        assert_eq!(buy.order.id, 1);
        assert_eq!(buy.trades.len(), 1);
        assert_eq!(buy.trades[0].id, 0);
        assert_eq!(buy.trades[0].price, 10000);
        assert_eq!(buy.trades[0].qty, 2.0);
        assert_eq!(buy.order.status, Status::Filled);
    }

    #[test]
    fn cancel_order_by_id() {
        let mut ex = Exchange::new();
        let resting = ex.place_order(Side::Buy, PriceType::Limit(10000), 5.0);

        let cancelled = ex.cancel_order(resting.order.id).expect("should exist");
        assert_eq!(cancelled.id, resting.order.id);
        assert_eq!(cancelled.status, Status::Cancelled);
        assert!(ex.cancel_order(resting.order.id).is_none());
    }

    #[test]
    fn top_of_book_reports_best_bid_and_ask() {
        let mut ex = Exchange::new();

        assert_eq!(ex.best_bid(), None);
        assert_eq!(ex.best_ask(), None);

        ex.place_order(Side::Sell, PriceType::Limit(11000), 2.0);
        assert_eq!(ex.best_ask(), Some(11000));
        assert_eq!(ex.best_bid(), None);

        ex.place_order(Side::Buy, PriceType::Limit(9900), 3.0);
        ex.place_order(Side::Buy, PriceType::Limit(10050), 1.0);
        assert_eq!(ex.best_bid(), Some(10050));
        assert_eq!(ex.best_ask(), Some(11000));

        ex.place_order(Side::Sell, PriceType::Limit(10900), 2.0);
        assert_eq!(ex.best_ask(), Some(10900));

        ex.place_order(Side::Buy, PriceType::Limit(11500), 2.0);
        assert_eq!(ex.best_ask(), Some(11000));
        assert_eq!(ex.best_bid(), Some(10050));
    }
}