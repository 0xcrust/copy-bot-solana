use solana_program_runtime::compute_budget::ComputeBudget;
use solana_program_runtime::compute_budget_processor::{
    process_compute_budget_instructions, DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
    MAX_COMPUTE_UNIT_LIMIT,
};
use solana_program_runtime::prioritization_fee::{PrioritizationFeeDetails, PrioritizationFeeType};
use solana_sdk::borsh1::try_from_slice_unchecked;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::instruction::CompiledInstruction;
use solana_sdk::message::VersionedMessage;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::VersionedTransaction;

#[derive(Copy, Debug, Default, Clone)]
pub struct PriorityDetails {
    pub priority_fee: u64,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
}

impl PriorityDetails {
    pub fn new(priority_fee: u64, compute_unit_limit: u32, compute_unit_price: u64) -> Self {
        PriorityDetails {
            priority_fee,
            compute_unit_limit,
            compute_unit_price,
        }
    }

    pub fn from_cu_price(cu_price_microlamports: u64, cu_limit: Option<u32>) -> Self {
        let cu_limit = cu_limit.unwrap_or(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT);
        let fee = PrioritizationFeeType::ComputeUnitPrice(cu_price_microlamports);
        let details = PrioritizationFeeDetails::new(fee, cu_limit as u64);
        PriorityDetails {
            priority_fee: details.get_fee(),
            compute_unit_limit: cu_limit,
            compute_unit_price: cu_price_microlamports,
        }
    }

    pub fn from_versioned_transaction(transaction: &VersionedTransaction) -> Self {
        if let Err(e) = transaction.sanitize() {
            return PriorityDetails {
                priority_fee: 0,
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                compute_unit_price: 0,
            };
        }
        priority_details_from_instructions(
            transaction.message.instructions(),
            transaction.message.static_account_keys(),
        )
    }

    pub fn from_versioned_message(message: &VersionedMessage) -> Self {
        if let Err(e) = message.sanitize() {
            return PriorityDetails {
                priority_fee: 0,
                compute_unit_limit: DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT,
                compute_unit_price: 0,
            };
        }
        priority_details_from_instructions(message.instructions(), message.static_account_keys())
    }

    pub fn from_instructions(
        instructions: &[CompiledInstruction],
        static_account_keys: &[Pubkey],
    ) -> Self {
        priority_details_from_instructions(instructions, static_account_keys)
    }

    pub fn from_iterator<'a>(
        instructions: impl Iterator<Item = (&'a Pubkey, &'a CompiledInstruction)>,
    ) -> Self {
        let compute_unit_limit = DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT;
        let compute_budget = ComputeBudget::new(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64);

        let compute_limits = process_compute_budget_instructions(instructions);
        match compute_limits {
            Ok(compute_limits) => {
                let fee =
                    PrioritizationFeeType::ComputeUnitPrice(compute_limits.compute_unit_price);
                let details =
                    PrioritizationFeeDetails::new(fee, compute_limits.compute_unit_limit as u64);
                PriorityDetails {
                    priority_fee: details.get_fee(),
                    compute_unit_limit: compute_limits.compute_unit_limit,
                    compute_unit_price: compute_limits.compute_unit_price,
                }
            }
            Err(e) => PriorityDetails {
                priority_fee: 0,
                compute_unit_limit,
                compute_unit_price: 0,
            },
        }
    }
}

fn priority_details_from_instructions(
    instructions: &[CompiledInstruction],
    static_account_keys: &[Pubkey],
) -> PriorityDetails {
    let mut compute_unit_limit = DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT;
    let compute_budget = ComputeBudget::new(DEFAULT_INSTRUCTION_COMPUTE_UNIT_LIMIT as u64);

    let instructions = instructions.iter().map(|ix| {
        if let Ok(ComputeBudgetInstruction::SetComputeUnitLimit(limit)) =
            try_from_slice_unchecked(&ix.data)
        {
            compute_unit_limit = limit.min(MAX_COMPUTE_UNIT_LIMIT);
        }
        (
            static_account_keys
                .get(usize::from(ix.program_id_index))
                .expect("program id index is sanitized"),
            ix,
        )
    });
    let compute_limits = process_compute_budget_instructions(instructions);
    match compute_limits {
        Ok(compute_limits) => {
            let fee = PrioritizationFeeType::ComputeUnitPrice(compute_limits.compute_unit_price);
            let details =
                PrioritizationFeeDetails::new(fee, compute_limits.compute_unit_limit as u64);
            PriorityDetails {
                priority_fee: details.get_fee(),
                compute_unit_limit: compute_limits.compute_unit_limit,
                compute_unit_price: compute_limits.compute_unit_price,
            }
        }
        Err(e) => PriorityDetails {
            priority_fee: 0,
            compute_unit_limit,
            compute_unit_price: 0,
        },
    }
}

#[cfg(test)]
mod test {
    use super::PriorityDetails;
    use std::str::FromStr;

    use solana_client::nonblocking::rpc_client::RpcClient;
    use solana_client::rpc_config::RpcTransactionConfig;
    use solana_sdk::commitment_config::CommitmentConfig;
    use solana_sdk::signature::Signature;
    use solana_transaction_status::UiTransactionEncoding;

    #[derive(Debug)]
    struct TransactionDetails {
        fee: u64, // in lamports
        compute_unit_limit: u32,
        compute_unit_price: u64, // in MicroLamports
    }
    impl TransactionDetails {
        const fn new(fee: u64, compute_unit_limit: u32, compute_unit_price: u64) -> Self {
            TransactionDetails {
                fee,
                compute_unit_limit,
                compute_unit_price,
            }
        }
    }

    const FIXTURES: [(&str, TransactionDetails); 6] = [
        ("2pstHqTYrrJnBfvj5pmd1jZLaNo11Dm1sEdYUVWPEPd7tJx5T98YaeJoFcvK2Q3DXUzti6unMBccE9zw8yB6NQsd", TransactionDetails::new(105000, 1400000, 71428)),
        ("5MVAmz9EA6xzpsJiMo9dTy1HTx33Rmq3796gdUK2uACokBu8dmQzrVp1HKMZuS8UL4JDiNmGFBXCxHuU46A1wxC1", TransactionDetails::new(5039, 3631, 10500)),
        ("234tKjmeRnw6cNTskBC4CiLUg8AAEdkDCr88mQatkPwp5LA4NSsuCCFFqh4mTUUUyzKqvqKrHxFm3kr3vsEp5pyE", TransactionDetails::new(33000, 1400000, 20000)),
        ("3UuztBgJFZjVghYtDUKzUhw7s61umxdzXx4d6iVGMKfMNGjRNRBUe3Kr3qg9G2ND6mSHbrzhe2pvZrmpPBRiszr5", TransactionDetails::new(33003, 1400000, 20002)),
        ("32GVtufP4Lw3osHwK3wqru3NfUTjdPr2EohHfBrmcMVKfRFHRJrMheGmxRZ6poP6QofF8QXj4sdLT1oQVYFJGMUq", TransactionDetails::new(5018, 1651, 10500)),
        ("3YMo98JnYHEhTJnrecGujGeYCgvuDt3Qwjy9GBhYZqWxxC4SGneSaRcAtsBq9b4udV9gLQ4h1xbzeBLfhXqwjvAa", TransactionDetails::new(5039, 3631, 10500)),
    ];
    const BASE_FEE_LAMPORTS: u64 = 5000;
    #[tokio::test]
    async fn test_compute_priority_details() {
        dotenv::dotenv().unwrap();
        let url = std::env::var("TEST_RPC_URL").unwrap();
        let client = RpcClient::new(url);

        for (signature, details) in FIXTURES {
            let transaction = client
                .get_transaction_with_config(
                    &Signature::from_str(signature).unwrap(),
                    RpcTransactionConfig {
                        encoding: Some(UiTransactionEncoding::Base64),
                        commitment: Some(CommitmentConfig::finalized()),
                        max_supported_transaction_version: Some(0),
                    },
                )
                .await
                .unwrap();

            let transaction = transaction.transaction.transaction.decode().unwrap();
            let priority_details = PriorityDetails::from_versioned_transaction(&transaction);
            let expected_priority_fee = details.fee - BASE_FEE_LAMPORTS;
            assert_eq!(
                priority_details.compute_unit_limit,
                details.compute_unit_limit
            );
            assert_eq!(
                priority_details.compute_unit_price,
                details.compute_unit_price
            );
            assert_eq!(priority_details.priority_fee, expected_priority_fee);
        }
    }
}
