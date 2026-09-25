use simulated_orderbook::domain::{PriceType, Side};
use simulated_orderbook::exchange::Exchange;

fn main() {
    let mut ex = Exchange::new();

    let sell = ex.place_order(Side::Sell, PriceType::Limit(10000), 5.0);
    println!("sell placed: id={}, status={:?}", sell.order.id, sell.order.status);

    let buy = ex.place_order(Side::Buy, PriceType::Market, 3.0);
    println!("buy  result: id={}, status={:?}, remaining={}",
        buy.order.id, buy.order.status, buy.order.remaining());
    println!("trades: {:#?}", buy.trades);
}