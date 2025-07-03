-- Trade direction ENUM
CREATE TYPE trade_direction as ENUM (
    'buy',
    'sell'
);

-- Trade table
CREATE TABLE IF NOT EXISTS trade (
    id BIGSERIAL PRIMARY KEY,
    signature VARCHAR(90) NOT NULL, 
    wallet VARCHAR(46) NOT NULL,
    token VARCHAR(46) NOT NULL,
    direction trade_direction NOT NULL,
    volume_token BIGINT NOT NULL,
    volume_usd float8 NOT NULL,
    volume_sol float8 NOT NULL,
    inserted_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

-- Copy execution table
CREATE TABLE IF NOT EXISTS copy_execution (
    trade_id BIGINT NOT NULL REFERENCES trade(id), 
    origin_trade_id BIGINT NOT NULL REFERENCES trade(id) 
);

CREATE OR REPLACE FUNCTION copy_execution_invariant()
RETURNS TRIGGER AS $$
BEGIN
    -- Check if tokens match between the two trades
    IF (SELECT token FROM trade WHERE id = NEW.trade_id) != 
       (SELECT token FROM trade WHERE id = NEW.origin_trade_id) THEN
        RAISE EXCEPTION 'Tokens do not match between trade_id and origin_trade_id';
    END IF;

    -- Check if directions match between the two trades
    IF (SELECT direction FROM trade WHERE id = NEW.trade_id) != 
       (SELECT direction FROM trade WHERE id = NEW.origin_trade_id) THEN
        RAISE EXCEPTION 'Directions do not match between trade_id and origin_trade_id';
    END IF;

    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER check_tokens_match
BEFORE INSERT OR UPDATE ON copy_execution
FOR EACH ROW
EXECUTE FUNCTION copy_execution_invariant();

CREATE TABLE IF NOT EXISTS transaction_cache (
    signature VARCHAR(90) PRIMARY KEY NOT NULL,
    signer VARCHAR(46) NOT NULL,
    transaction BYTEA NOT NULL
);

CREATE TABLE IF NOT EXISTS token_cache (
    address VARCHAR(46) PRIMARY KEY,
    decimals SMALLINT NOT NULL,
    name TEXT,
    logo_uri TEXT,
    symbol TEXT,
    program_id VARCHAR(46),
    tags TEXT[],
    freeze_authority VARCHAR(46),
    mint_authority VARCHAR(46)
);

-- todo: Cache 5min historical price(for SOL, WIF and popular tokens, maybe all tokens)
-- todo: Cache token information and update database regularly(needed especially for decimals tbh)
