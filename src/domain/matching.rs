use super::{Order, OrderBook, PriceType, Side, Status, Trade};

pub struct MatchResult {
    pub trades: Vec<Trade>,
    pub order: Order,
    pub touched_orders: Vec<Order>,
}

fn price_ok(side: Side, book_price: i64, order_price: PriceType) -> bool {
    match order_price {
        PriceType::Market => true,
        PriceType::Limit(p) => match side {
            Side::Buy => book_price <= p,
            Side::Sell => book_price >= p,
        },
    }
}

fn fill_status(remaining: f64) -> Status {
    if remaining <= 0.0 {
        Status::Filled
    } else {
        Status::Partial
    }
}

pub fn match_order(book: &mut OrderBook, incoming: Order) -> MatchResult {
    let mut order = incoming;
    let mut trades = Vec::new();
    let mut touched_orders = Vec::new();

    {
        let against = match order.side {
            Side::Buy => &mut book.asks,
            Side::Sell => &mut book.bids,
        };

        while order.remaining() > 0.0 {
            let best_price = against
                    .iter()
                    .find(|(price, queue)| {
                        !queue.is_empty() && price_ok(order.side, **price, order.price)
                    })
                    .map(|(price, _)| *price);
            let Some(price) = best_price else {
                break;
            };
            let queue = against.get_mut(&price).expect("price level present");

            let filled_front = {
                let front = queue.front_mut().expect("queue not empty");
                let fill = order.remaining().min(front.remaining());

                order.filled += fill;
                front.filled += fill;

                order.status = fill_status(order.remaining());
                front.status = fill_status(front.remaining());

                trades.push(Trade {
                    id: 0,
                    buy_order_id: if order.side == Side::Buy { order.id } else { front.id },
                    sell_order_id: if order.side == Side::Sell { order.id } else { front.id },
                    price,
                    qty: fill,
                    timestamp: order.timestamp.max(front.timestamp),
                });

                touched_orders.push(front.clone());

                front.remaining() <= 0.0
            };

            if filled_front {
                queue.pop_front();
                if queue.is_empty() {
                    against.remove(&price);
                }
            }
        }
    }

    if order.remaining() > 0.0 {
        match order.price {
            PriceType::Limit(p) => {
                let resting_side = match order.side {
                    Side::Buy => &mut book.bids,
                    Side::Sell => &mut book.asks,
                };
                resting_side.entry(p).or_default().push_back(order.clone());
            }
            PriceType::Market => {
                order.status = Status::Cancelled;
            }
        }
    }

    MatchResult { trades, order, touched_orders }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(id: u64, side: Side, price: PriceType, qty: f64) -> Order {
        Order {
            id,
            user_id: id,
            side,
            price,
            qty,
            filled: 0.0,
            status: Status::Unfilled,
            timestamp: id,
        }
    }

    #[test]
    fn market_buy_fills_cheapest_ask_first() {
        let mut book = OrderBook::new();

        match_order(&mut book, order(1, Side::Sell, PriceType::Limit(11000), 3.0));
        match_order(&mut book, order(2, Side::Sell, PriceType::Limit(10500), 2.0));

        let buy = order(3, Side::Buy, PriceType::Market, 3.0);
        let result = match_order(&mut book, buy);

        assert_eq!(result.trades.len(), 2);
        assert_eq!((result.trades[0].price, result.trades[0].qty), (10500, 2.0));
        assert_eq!((result.trades[1].price, result.trades[1].qty), (11000, 1.0));
    }

    #[test]
    fn limit_buy_refuses_prices_above_its_ceiling() {
        let mut book = OrderBook::new();

        match_order(&mut book, order(1, Side::Sell, PriceType::Limit(10500), 2.0));

        let buy = order(2, Side::Buy, PriceType::Limit(10000), 3.0);
        let result = match_order(&mut book, buy);

        assert!(result.trades.is_empty());
        assert_eq!(result.order.remaining(), 3.0);
        assert!(book.bids.contains_key(&10000));
        assert!(book.asks.contains_key(&10500));
    }

    #[test]
    fn partial_fill_marks_order_partial_and_rests_remainder() {
        let mut book = OrderBook::new();

        match_order(&mut book, order(1, Side::Sell, PriceType::Limit(10000), 2.0));

        let buy = order(2, Side::Buy, PriceType::Limit(11000), 5.0);
        let result = match_order(&mut book, buy);

        assert_eq!(result.trades.len(), 1);
        assert_eq!((result.trades[0].price, result.trades[0].qty), (10000, 2.0));
        assert_eq!(result.order.status, Status::Partial);
        assert_eq!(result.order.remaining(), 3.0);
        assert!(book.bids.contains_key(&11000));
    }

    #[test]
    fn sell_order_drains_bids_and_respects_floor() {
        let mut book = OrderBook::new();

        match_order(&mut book, order(1, Side::Buy, PriceType::Limit(10000), 2.0));

        let sell_too_high = order(2, Side::Sell, PriceType::Limit(11000), 5.0);
        let high_result = match_order(&mut book, sell_too_high);

        assert!(high_result.trades.is_empty());
        assert_eq!(high_result.order.remaining(), 5.0);
        assert!(book.asks.contains_key(&11000));

        let sell = order(3, Side::Sell, PriceType::Limit(10000), 2.0);
        let result = match_order(&mut book, sell);

        assert_eq!(result.trades.len(), 1);
        assert_eq!((result.trades[0].price, result.trades[0].qty), (10000, 2.0));
        assert!(book.bids.is_empty());
    }

    #[test]
    fn earlier_order_at_same_price_fills_first() {
        let mut book = OrderBook::new();

        match_order(&mut book, order(1, Side::Sell, PriceType::Limit(10000), 2.0));
        match_order(&mut book, order(2, Side::Sell, PriceType::Limit(10000), 3.0));

        let buy = order(3, Side::Buy, PriceType::Market, 3.0);
        let result = match_order(&mut book, buy);

        assert_eq!(result.trades.len(), 2);
        assert_eq!(result.trades[0].sell_order_id, 1);
        assert_eq!(result.trades[1].sell_order_id, 2);
        assert_eq!(result.trades[0].qty, 2.0);
        assert_eq!(result.trades[1].qty, 1.0);
        assert_eq!(book.asks[&10000].front().unwrap().id, 2);
    }
}