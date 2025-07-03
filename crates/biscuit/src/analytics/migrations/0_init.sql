CREATE TABLE IF NOT EXISTS transactions (
    signature VARCHAR(90) NOT NULL,
    json_raw TEXT NOT NULL,
    
)

-- Create schema
CREATE SCHEMA IF NOT EXISTS analytics;

-- Trade direction ENUM
CREATE TYPE analytics.trade_direction as ENUM (
    'buy',
    'sell'
);

-- Token 
CREATE TABLE IF NOT EXISTS analytics.token_info (
    address VARCHAR(46) PRIMARY KEY,
    decimals SMALLINT NOT NULL,
    name TEXT,
    logo_uri TEXT,
    symbol TEXT
);

-- From birdeye API: `get_trades_for_token`
CREATE TABLE IF NOT EXISTS analytics.early_birds (
    token VARCHAR(46) PRIMARY KEY,
    wallets VARCHAR(46)[]
);

CREATE TABLE IF NOT EXISTS analytics.trade (
    id BIGSERIAL PRIMARY KEY,
    signer VARCHAR(46) NOT NULL,
    signature VARCHAR(90) NOT NULL,
    slot BIGINT NOT NULL,
    block_time BIGINT NOT NULL,
    token VARCHAR(46) NOT NULL,
    other_token VARCHAR(46) NOT NULL,
    direction analytics.trade_direction NOT NULL,
    token_amount float8 NOT NULL,
    other_token_amount float8 NOT NULL,
    token_volume_usd float8 NOT NULL,
    other_token_volume_usd float8 NOT NULL
);
