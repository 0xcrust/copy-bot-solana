use crate::parser::generated::pump::accounts::BondingCurve;

#[derive(Debug)]
pub struct BondingCurveComplete;
impl std::fmt::Display for BondingCurveComplete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bonding curve complete")
    }
}
impl std::error::Error for BondingCurveComplete {}

impl BondingCurve {
    pub fn get_buy_price(&self, amount: u64) -> Result<u64, BondingCurveComplete> {
        if self.complete {
            return Err(BondingCurveComplete);
        }

        if amount == 0 {
            return Ok(0);
        }

        // Calculate the product of virtual reserves
        let n = self.virtual_sol_reserves as u128 * self.virtual_token_reserves as u128;
        // Calculate the new virtual sol reserves after the purchase
        let i = (self.virtual_sol_reserves + amount) as u128;
        // Calculate the new virtual token reserves after the purchase
        let r = (n / i) as u64 + 1;
        // Calculate the amount of tokens to be purchased
        let s = self.virtual_token_reserves - r;

        // Return the minimum of the calculated tokens and real token reserves
        Ok(std::cmp::min(self.real_token_reserves, s))
    }

    pub fn get_sell_price(&self, amount: u64, fee_bps: u64) -> Result<u64, BondingCurveComplete> {
        if self.complete {
            return Err(BondingCurveComplete);
        }

        if amount == 0 {
            return Ok(0);
        }

        // Calculate the proportional amount of virtual sol reserves to be received
        let n = ((amount as u128 * self.virtual_sol_reserves as u128)
            / (self.virtual_token_reserves as u128 + amount as u128)) as u64;
        // Calculate the fee amount in the same units
        let a = (n * fee_bps) / 10_000;

        // Return the net amount after deducting the fee
        Ok(n - a)
    }

    pub fn get_market_cap_sol(&self) -> u64 {
        if self.virtual_token_reserves == 0 {
            return 0;
        }
        ((self.token_total_supply as u128 * self.virtual_sol_reserves as u128)
            / self.virtual_token_reserves as u128) as u64
    }

    pub fn get_buy_out_price(&self, amount: u64, fee_bps: u64) -> u64 {
        let sol_tokens = std::cmp::min(amount, self.real_sol_reserves) as u128;
        let total_sell_value = (((sol_tokens * self.virtual_sol_reserves as u128)
            / (self.virtual_token_reserves as u128 - sol_tokens))
            + 1) as u64;
        let fee = total_sell_value * fee_bps / 10_000;
        total_sell_value + fee
    }

    pub fn final_market_cap_sol(&self, fee_bps: u64) -> u64 {
        let total_sell_value = self.get_buy_out_price(self.real_token_reserves, fee_bps);
        let total_virtual_value = self.virtual_sol_reserves + total_sell_value;
        let total_virtual_tokens = self.virtual_token_reserves - self.real_token_reserves;

        if total_virtual_tokens == 0 {
            return 0;
        }

        ((self.token_total_supply as u128 * total_virtual_value as u128)
            / total_virtual_tokens as u128) as u64
    }
}
