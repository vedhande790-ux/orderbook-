-- 0001: initial schema — what the exchange remembers
-- orders: one row per order ever placed (resting, filled, cancelled)
CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    side        TEXT NOT NULL CHECK (side IN ('buy', 'sell')),
    price       BIGINT, -- NULL = market order (mirrors PriceType::Market)
    qty         DOUBLE PRECISION NOT NULL,
    filled      DOUBLE PRECISION NOT NULL DEFAULT 0,
    status      TEXT NOT NULL, -- Unfilled | Partial | Filled | Cancelled
    user_id     BIGINT NOT NULL,
    timestamp   BIGINT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- trades: one row per match printed
CREATE TABLE trades (
    id            BIGINT PRIMARY KEY,
    price         BIGINT NOT NULL, -- execution price in cents
    qty           DOUBLE PRECISION NOT NULL,
    buy_order_id  BIGINT NOT NULL REFERENCES orders(id),
    sell_order_id BIGINT NOT NULL REFERENCES orders(id),
    timestamp     BIGINT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);