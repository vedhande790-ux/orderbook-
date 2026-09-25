use std::collections::{BTreeMap, VecDeque};

use super::{Order, Status};

#[derive(Debug, Clone)]
pub struct OrderBook {
    pub bids: BTreeMap<i64, VecDeque<Order>>,
    pub asks: BTreeMap<i64, VecDeque<Order>>,
}

impl OrderBook {
    pub fn new() -> Self {
        OrderBook {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    pub fn cancel(&mut self, order_id: u64) -> Option<Order> {
        for map in [&mut self.bids, &mut self.asks] {
            let hit = map.iter().find_map(|(&price, queue)| {
                queue
                    .iter()
                    .position(|o| o.id == order_id)
                    .map(|pos| (price, pos))
            });
            if let Some((price, pos)) = hit {
                let (mut order, queue_empty) = {
                    let queue = map.get_mut(&price).unwrap();
                    let order = queue.remove(pos).unwrap();
                    (order, queue.is_empty())
                };
                order.status = Status::Cancelled;
                if queue_empty {
                    map.remove(&price);
                }
                return Some(order);
            }
        }
        None
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}