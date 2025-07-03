// use super::config::CopyConfig;
// use super::history::{get_token_prices, HistoricalTrades, TradeDirection};
// use crate::constants::mints::{SOL, USDC, USDT};
// use crate::constants::programs::{
//     JUPITER_AGGREGATOR_V6_PROGRAM_ID, ORCA_WHIRLPOOLS_PROGRAM_ID,
//     RAYDIUM_CONCENTRATED_LIQUIDITY_PROGRAM_ID, RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
// };
// use crate::core::traits::swap::v1::{ParsedSwap, SwapParser, SwapTokens};
// use crate::core::traits::txn_extensions::TransactionMods;
// use crate::core::traits::{PollRequest, StreamInterface};
// use crate::core::types::wire_transaction::ConfirmTransactionResponse;
// use crate::core::JoinHandleResult;
// use crate::parser::v1::V1Parser;
// use crate::swap::execution::{
//     SwapExecutionMode, SwapExecutor, SwapInput, SwapQuote, TransactionVariant,
// };
// use crate::swap::jupiter::executor::{ExecutorQuoteResponse, JupiterExecutor};
// use crate::swap::jupiter::price::PriceFeed as JupiterPriceFeed;
// use crate::utils::fees::PriorityDetails;
// use crate::wire::helius_rpc::{HeliusPrioFeesHandle, PriorityLevel};
// use crate::wire::rpc::poll_transactions::RpcTransactionsHandle;
// use crate::wire::send_txn::service::SendTransactionService;

// use std::sync::atomic::{AtomicUsize, Ordering};
// use std::sync::Arc;

// use anyhow::{anyhow, Context};
// use dashmap::DashMap;
// use futures::StreamExt;
// use log::{debug, error, info, warn};
// use solana_client::nonblocking::rpc_client::RpcClient;
// use solana_sdk::commitment_config::CommitmentConfig;
// use solana_sdk::signature::{Keypair, Signature, Signer};
// use solana_sdk::transaction::VersionedTransaction;
// use solana_sdk::{pubkey, pubkey::Pubkey};
// use tokio::sync::mpsc::{channel, unbounded_channel, Receiver};
// use tokio_stream::wrappers::UnboundedReceiverStream;

// const SWAP_PROGRAMS: [Pubkey; 4] = [
//     JUPITER_AGGREGATOR_V6_PROGRAM_ID,
//     RAYDIUM_CONCENTRATED_LIQUIDITY_PROGRAM_ID,
//     RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM_ID,
//     ORCA_WHIRLPOOLS_PROGRAM_ID,
// ];
// pub const STATIC_TOKENS: [Pubkey; 3] = [SOL, USDC, USDT];

// const PRIOFEE_ACCOUNTS: [Pubkey; 1] = [pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4")];

// #[derive(Clone, Debug)]
// pub struct CopyDetails<Q> {
//     origin_signature: Signature,
//     origin_signer: Pubkey,
//     origin_swap: ParsedSwap,
//     origin_swap_tokens: SwapTokens,
//     swap_exec_retries: u8,
//     send_txn_retries: u8,
//     confirmed_error_retries: u8,
//     retry_strategy: RetryStrategy,
//     next_priority_level: PriorityLevel,
//     quote_output: Option<Q>,
// }

// #[derive(Clone, Debug)]
// pub enum RetryStrategy {
//     BuildTransaction,
//     SendRaw(VersionedTransaction),
// }

// pub fn start_copy_service(
//     rpc_client: Arc<RpcClient>,
//     keypair: Arc<Keypair>,
//     copy_accounts: Vec<PollRequest>,
//     txns_handle: RpcTransactionsHandle,
//     txn_service: SendTransactionService,
//     mut new_accounts_receiver: Receiver<PollRequest>,
//     commitment: CommitmentConfig,
//     config: CopyConfig,
//     helius_priofees_handle: &HeliusPrioFeesHandle,
// ) -> anyhow::Result<Vec<JoinHandleResult<anyhow::Error>>> {
//     let start = std::time::Instant::now();
//     let CopyConfig {
//         trade_token,
//         trade_token_decimals,
//         input_amount,
//         slippage_bps,
//         moonbag_bps,
//         // max_pending_queue_length,
//         max_retry_queue_length,
//         retries_per_swap,
//         send_txn_retries,
//         confirmed_error_retries,
//         quote_api_url,
//         price_feed_url,
//         price_feed_refresh_frequency_ms,
//         log_interval_seconds,
//     } = config;

//     let (price_feed_task, price_feed) =
//         JupiterPriceFeed::start(price_feed_url, price_feed_refresh_frequency_ms);
//     txns_handle.watch_accounts(copy_accounts)?;
//     let (parsed_swap_tx, parsed_swap_rx) = unbounded_channel();
//     let (send_transactions_handle, confirmed_txns_receiver) = txn_service.subscribe();
//     let pending_transactions = Arc::new(DashMap::new());
//     let helius_priofees = helius_priofees_handle.subscribe();
//     helius_priofees.update_accounts(
//         PRIOFEE_ACCOUNTS
//             .into_iter()
//             .map(|p| p.to_string())
//             .collect(),
//     );

//     let received_tracked_txns = Arc::new(AtomicUsize::new(0));
//     let unique_processed_txns = Arc::new(AtomicUsize::new(0));
//     let sent_copy_txns = Arc::new(AtomicUsize::new(0));
//     let confirmed_copy_txns = Arc::new(AtomicUsize::new(0));
//     let unconfirmed_copy_txns = Arc::new(AtomicUsize::new(0));
//     let historical_trades = HistoricalTrades::default();

//     let main_task = tokio::spawn({
//         let rpc_client = Arc::clone(&rpc_client);
//         let handle = txns_handle;
//         let received_tracked_txns = Arc::clone(&received_tracked_txns);
//         let parsed_swap_tx = parsed_swap_tx.clone();
//         async move {
//             let mut txns_stream = handle.subscribe();

//             loop {
//                 tokio::select! {
//                     Some(x) = new_accounts_receiver.recv() => {
//                         if let Err(e) = handle.watch_accounts(vec![x]) {
//                             error!("Failed to send watch account command");
//                         }
//                     }
//                     Some(tx) = txns_stream.next() => {
//                         let transaction = match tx {
//                             Err(e) => {
//                                 error!("Error from txn stream: {}", e);
//                                 continue;
//                             },
//                             Ok(tx) => tx
//                         };
//                         // Make sure we only process txns the copy service should be bothered with
//                         // Currently we do this by guaranteeing that this service doesn't share a txn stream
//                         // In the future we should do this by:
//                         //   * Adding filter combinators to the resulting stream
//                         //   * Letting the sending part of the txn stream recognize filters
//                         // Currently we rely on the fact that our txn stream is unique to this service
//                         received_tracked_txns.fetch_add(1, Ordering::Relaxed);
//                         // log::info!("[Copy] Detected tx {}", transaction.signature);

//                         // Skip failed transactions
//                         if let Some(ref err) = transaction.err {
//                             warn!("Skipping failed tx {}. error={}", transaction.signature, transaction.err.unwrap());
//                             continue;
//                         }

//                         let keys = transaction.account_keys();
//                         let mut instruction = transaction.message.instructions().iter().find_map(|i| {
//                             let program = keys.get(i.program_id_index as usize).unwrap();
//                             if SWAP_PROGRAMS.contains(program) {
//                                 Some(i)
//                             } else {
//                                 None
//                             }
//                         });
//                         if instruction.is_none() {
//                             if let Some(inner_instructions) = transaction.inner_instructions.as_ref() {
//                                 instruction = inner_instructions
//                                     .into_iter()
//                                     .find_map(|(instruction_idx, instructions)| {
//                                         instructions.into_iter().find_map(|ix| {
//                                             let program_id = keys.get(ix.program_id_index as usize).unwrap();
//                                             SWAP_PROGRAMS
//                                                 .into_iter()
//                                                 .find(|p| p == program_id)
//                                                 .map(|_| ix)
//                                         })
//                                     });
//                             }
//                         }

//                         if instruction.is_none() {
//                             warn!("No swap ix for transaction {}. Skipping..", transaction.signature);
//                             continue;
//                         }

//                         let parsed = match V1Parser::parse_instruction(instruction.unwrap(), &keys) {
//                             Err(e) => {
//                                 error!("Error parsing transaction with signature {}: {}", transaction.signature, e);
//                                 continue
//                             },
//                             Ok(None) => {
//                                 error!("Couldn't parse transaction");
//                                 continue
//                             }
//                             Ok(Some(parsed)) => parsed
//                         };
//                         let swap_details = parsed.get_swap_details();
//                         if swap_details.is_none() {
//                             info!("Skipping tx {} because it is not a swap transaction", transaction.signature);
//                             continue;
//                         }
//                         let origin_swap = swap_details.unwrap();
//                         let origin_swap_tokens = match origin_swap.resolve_token_information_from_status_meta(&transaction.meta, &transaction.account_keys()) {
//                             Some(tokens) => tokens,
//                             None => {
//                                 error!("Failed to resolve token information for tx={}", transaction.signature);
//                                 continue
//                             }
//                         };
//                         let pending = CopyDetails {
//                             origin_signature: transaction.signature,
//                             origin_signer: *transaction.account_keys().get(0).unwrap(),
//                             origin_swap,
//                             origin_swap_tokens,
//                             swap_exec_retries: 0,
//                             send_txn_retries: 0,
//                             confirmed_error_retries: 0,
//                             retry_strategy: RetryStrategy::BuildTransaction,
//                             next_priority_level: PriorityLevel::Medium,
//                             quote_output: None
//                         };

//                         if let Err(e) = parsed_swap_tx.send(pending) {
//                             error!("Failed to send parsed swap details to swap processing task: {}", e);
//                         }
//                     }
//                 }
//             }
//         }
//     });

//     let num_sent_transactions = Arc::new(AtomicUsize::new(0));
//     let swap_executor_task = tokio::spawn({
//         let stream = UnboundedReceiverStream::new(parsed_swap_rx);
//         let user_pubkey = keypair.pubkey();
//         let rpc_client = Arc::clone(&rpc_client);
//         let sent_copy_txns = Arc::clone(&sent_copy_txns);
//         let unique_processed_txns = Arc::clone(&unique_processed_txns);
//         let pending_transactions = Arc::clone(&pending_transactions);
//         let send_transactions_handle = send_transactions_handle.clone();
//         let parsed_swap_tx = parsed_swap_tx.clone();
//         let keypair = keypair.clone();
//         let historical_trades = historical_trades.clone();
//         let (retry_tx, mut retry_rx) =
//             channel::<CopyDetails<ExecutorQuoteResponse>>(max_retry_queue_length);
//         let price_feed = price_feed.clone();
//         let executor = Arc::new(JupiterExecutor::new(quote_api_url, Arc::clone(&rpc_client)));

//         async move {
//             let mut stream = stream
//                 .map(|mut details| {
//                     let rpc_client = Arc::clone(&rpc_client);
//                     let executor = Arc::clone(&executor);
//                     let helius_priofees = helius_priofees.clone();
//                     let retry_tx = retry_tx.clone();
//                     let historical_trades = historical_trades.clone();
//                     let price_feed = price_feed.clone();
//                     let keypair = keypair.clone();

//                     async move {
//                         let tokens = details.origin_swap_tokens;
//                         log::info!("Origin swap: {:#?}", tokens);
//                         let input_token = tokens.in_token_mint;
//                         let output_token = tokens.out_token_mint;

//                         if input_token == output_token {
//                             return Err(anyhow!(
//                                 "Input and output mints are the same: token={}.tx={}.input-token-account={}.parsed={:#?}",
//                                 input_token, details.origin_signature, details.origin_swap.in_token_account, details.origin_swap
//                             ));
//                         }
//                         debug!("Input-token={}. Output-token={}", input_token, output_token);

//                         // First check: We only care about a `sell` if the input-token of a swap is one we have from
//                         // copying that wallet. We register a single-trade.
//                         //
//                         // If the output-token of a sell is not SOL | USDC | USDT then we're selling one token
//                         // to buy another, we register two trades.

//                         // We only care about a `buy` if the output-token is not SOL | USDC | USDT
//                         // We still register a single trade.
//                         let origin_wallet = details.origin_signer;
//                         // todo: Do we need to repeat this step for transactions?
//                         let execution_input = match historical_trades
//                             .get_history_for_wallet_and_token(origin_wallet, input_token)
//                         {
//                             Some(history) => {
//                                 let exec_prev_sell_percentage = (history.exec_sell_volume_token
//                                     as f64
//                                     / history.exec_buy_volume_token as f64)
//                                     * 100.0;
//                                 let swap_amount = tokens.amount_in;
//                                 let wallet_prev_sell_percentage = (history.origin_sell_volume_token
//                                     as f64
//                                     / history.origin_buy_volume_token as f64)
//                                     * 100.0;
//                                 let wallet_sell_percentage = swap_amount as f64
//                                     / history.origin_buy_volume_token as f64
//                                     * 100.0;

//                                 let input_amount = if wallet_prev_sell_percentage
//                                     + wallet_sell_percentage
//                                     > 80.0
//                                 {
//                                     // We treat this case as if the wallet is selling all of their tokens, and sell all of ours too
//                                     _ = historical_trades.remove_history(&origin_wallet, &input_token);
//                                     history.exec_buy_volume_token - history.exec_sell_volume_token
//                                 } else {
//                                     let tokens_left = history.exec_buy_volume_token
//                                         - history.exec_sell_volume_token;
//                                     let exec_input_amount = // sell the same percentage the wallet does
//                                         wallet_sell_percentage / 100.0 * tokens_left as f64;
//                                     exec_input_amount as u64
//                                 };

//                                 if output_token == SOL
//                                     || output_token == USDC
//                                     || output_token == USDT
//                                 {
//                                     // Normal sell. We follow our usual strategy and sell this token for SOL
//                                     // our input-token = `input-token`, our output-token = `SOL`(or whatever the strategy specifies)
//                                     // our sell amount is the origin's sell percentage * our balance for this token
//                                     SwapInput {
//                                         input_token_mint: input_token,
//                                         output_token_mint: trade_token,
//                                         slippage_bps,
//                                         amount: input_amount,
//                                         mode: SwapExecutionMode::ExactIn,
//                                     }
//                                 } else {
//                                     // Wallet is selling a token to buy another. We have that token so we do the same
//                                     // We're making two trades. Selling a token and buying another token
//                                     // our input-token = `input-token`, our output-token = `output-token`
//                                     SwapInput {
//                                         input_token_mint: input_token,
//                                         output_token_mint: output_token,
//                                         amount: input_amount,
//                                         slippage_bps,
//                                         mode: SwapExecutionMode::ExactIn,
//                                     }
//                                 }
//                             }
//                             None => {
//                                 if output_token == SOL
//                                     || output_token == USDC
//                                     || output_token == USDT
//                                 {
//                                     // Skip this. We don't buy SOL/USDC/USDT and we also don't have the input-token so nothing to do
//                                     info!(
//                                         "Skipping tx={} to sell token for SOL | USDC | USDT",
//                                         details.origin_signature
//                                     );
//                                     return Ok(None);
//                                 } else {
//                                     // We just buy the token whatever token they're buying. We don't care about their input-token.
//                                     // our input-token = `SOL`(or whatever the strategy specifies)
//                                     SwapInput {
//                                         input_token_mint: trade_token,
//                                         amount: input_amount,
//                                         output_token_mint: output_token,
//                                         slippage_bps,
//                                         mode: SwapExecutionMode::ExactIn,
//                                     }
//                                 }
//                             }
//                         };
//                         log::info!("Exec swap: {:#?}", execution_input);
//                         price_feed.subscribe_token(&execution_input.input_token_mint).await;
//                         price_feed.subscribe_token(&execution_input.output_token_mint).await;

//                         let quote = executor.quote(&execution_input, None).await?;
//                         let swap_transaction = executor
//                             .swap_transaction(keypair.pubkey(), quote.clone(), None)
//                             .await?;
//                         let helius_cu_micro_lamports =
//                             helius_priofees.get_priority_fee(details.next_priority_level);
//                         let transaction = match swap_transaction.transaction {
//                             TransactionVariant::Raw(raw) => raw,
//                             _ => unreachable!(),
//                         };
//                         details.quote_output = Some(quote);
//                         let mut transaction: VersionedTransaction =
//                             bincode::deserialize(&transaction)
//                                 .context("Failed to deserialize txn from jup response")?;

//                         if let Some(updated_cu_price) = helius_cu_micro_lamports {
//                             //info!("helius cu-price: {}", updated_cu_price);
//                             transaction
//                                 .message
//                                 .modify_compute_unit_price(updated_cu_price as u64);
//                         }
//                         let _priority_fee =
//                             PriorityDetails::from_versioned_transaction(&transaction);
//                         Ok::<_, anyhow::Error>(Some((details, transaction)))
//                     }
//                 })
//                 .buffer_unordered(50);

//             loop {
//                 tokio::select! {
//                     Some(result) = stream.next() => {
//                         match result {
//                             Ok(Some((copy_details, transaction))) => {
//                                 match send_transactions_handle
//                                     .send_transaction(transaction, &[&keypair])
//                                     .await
//                                 {
//                                     Err(e) => {
//                                         error!("Failed sending transaction to transaction task: {}", e)
//                                     }
//                                     Ok(res) => {
//                                         sent_copy_txns.fetch_add(1, Ordering::Relaxed);
//                                         if copy_details.send_txn_retries == 0 {
//                                             unique_processed_txns.fetch_add(1, Ordering::Relaxed);
//                                         }
//                                         pending_transactions.insert(res.txn_id, copy_details);
//                                     }
//                                 }
//                             }
//                             Ok(None) => {
//                                 warn!("No swap executed");
//                             }
//                             Err(err) => {
//                                 error!("Swap execution error: {}", err);
//                             }
//                         };
//                     },
//                     Some(mut details) = retry_rx.recv() => {
//                         if details.swap_exec_retries > retries_per_swap {
//                             error!(
//                                 "Exceeded max retries for origin-tx={}",
//                                 details.origin_signature
//                             );
//                         } else {
//                             details.swap_exec_retries += 1;
//                             match parsed_swap_tx.send(details) {
//                                 Err(e) => error!(
//                                     "Failed requeuing for origin-tx={}",
//                                     e.0.origin_signature
//                                 ),
//                                 Ok(_) => {}
//                             }
//                         }

//                     }
//                 }
//             }

//             #[allow(unreachable_code)]
//             Ok::<_, anyhow::Error>(())
//         }
//     });

//     // TODO: Task for processing confirmed transactions(e.g index into a db)
//     // This will handle also handle retry logic for failed or unconfirmed-txns.
//     //
//     // Each service only receives the transactions they're concerned with
//     let confirmed_transactions_task = {
//         let mut confirmed_txns_receiver = confirmed_txns_receiver;
//         let mut log_tick =
//             tokio::time::interval(std::time::Duration::from_secs(log_interval_seconds));
//         let pending_transactions = Arc::clone(&pending_transactions);
//         let keypair = Arc::clone(&keypair);
//         let historical_trades = historical_trades.clone();
//         let price_feed = price_feed.clone();

//         tokio::task::spawn({
//             let price_feed = price_feed;

//             async move {
//                 price_feed.subscribe_token(&SOL).await;
//                 price_feed.subscribe_token(&USDC).await;
//                 price_feed.subscribe_token(&USDT).await;

//                 loop {
//                     tokio::select! {
//                         Some(item) = confirmed_txns_receiver.recv() => {
//                             // TODO: Track confirmed transactions for their cu-limit to get a more appriopriate value
//                             let (txn_id, mut details) = pending_transactions.remove(&item.data.txn_id).unwrap();
//                             match item.response {
//                                 ConfirmTransactionResponse::Confirmed {
//                                     error,
//                                     confirmation_slot,
//                                 } => {
//                                     // This tracks all confirmed txns, including failed ones:
//                                     confirmed_copy_txns.fetch_add(1, Ordering::Relaxed);
//                                     match error {
//                                         Some(err) => {
//                                             // todo: Retry here is naive. Possible improvements are to increase slippage, etc
//                                             // Retries here should use a new transaction id(not `resend_transaction`)
//                                             error!("Txn {} was confirmed but failed. Queueing swap for retry..", txn_id);
//                                             // details.send_txn_retries = 0;
//                                             details.swap_exec_retries = 0; // reset swap-exec-retries in case this swap is queued again

//                                             if details.confirmed_error_retries < confirmed_error_retries {
//                                                 details.confirmed_error_retries += 1;
//                                                 if let Err(e) = parsed_swap_tx.send(details) {
//                                                     error!("Failed to requeue failed txn {}", txn_id);
//                                                 }
//                                             }
//                                         },
//                                         None => {
//                                             // - If we have an entry for the input-token then we sold that token. Add sell volume
//                                             // - If we have an entry for the output-token then we bought that token. Add buy volume
//                                             // let tokens = details.origin_swap_tokens;
//                                             // let _output = details.quote_output.expect("execution output is not null");
//                                             // let price = get_token_prices(&execution_output, &price_feed).await;
//                                             let origin_swap_tokens = details.origin_swap_tokens;
//                                             let quote_output = details.quote_output.expect("execution output is not null");
//                                             let (input_amount, output_amount) = if quote_output.amount_specified_is_input() {
//                                                 (quote_output.amount(), quote_output.other_amount())
//                                             } else {
//                                                 (quote_output.other_amount(), quote_output.amount())
//                                             };
//                                             let mut in_token_decimals = quote_output.input_token_decimals();
//                                             let mut out_token_decimals = quote_output.output_token_decimals();

//                                             if in_token_decimals.is_none() {
//                                                 let input_mint = quote_output.input_token();
//                                                 if input_mint == USDC || input_mint == USDT {
//                                                     in_token_decimals = Some(6)
//                                                 } else if input_mint == SOL {
//                                                     in_token_decimals = Some(9)
//                                                 } else if input_mint == origin_swap_tokens.in_token_mint {
//                                                     in_token_decimals = Some(origin_swap_tokens.in_token_decimals)
//                                                 }
//                                             }

//                                             if out_token_decimals.is_none() {
//                                                 let output_mint = quote_output.output_token();
//                                                 if output_mint == USDC || output_mint == USDT {
//                                                     out_token_decimals = Some(6)
//                                                 } else if output_mint == SOL {
//                                                     out_token_decimals = Some(9)
//                                                 } else if output_mint == origin_swap_tokens.out_token_mint {
//                                                     out_token_decimals = Some(origin_swap_tokens.in_token_decimals)
//                                                 }
//                                             }

//                                             if in_token_decimals.is_none() || out_token_decimals.is_none() {
//                                                 log::error!("Couldn't get token decimals for swap. tx={}. Skipping history insertion", item.data.signature);
//                                             }

//                                             let in_token_decimals = in_token_decimals.unwrap();
//                                             let out_token_decimals = out_token_decimals.unwrap();

//                                             let price = get_token_prices(
//                                                 &quote_output.input_token(),
//                                                 &quote_output.output_token(),
//                                                 input_amount,
//                                                 output_amount,
//                                                 in_token_decimals,
//                                                 out_token_decimals,
//                                                 &price_feed
//                                             ).await;
//                                             if !STATIC_TOKENS.contains(&quote_output.input_token()) {
//                                                 if let Err(e) = historical_trades.insert_trade(
//                                                     &details.origin_signer,
//                                                     &quote_output.input_token(),
//                                                     in_token_decimals,
//                                                     quote_output.amount(), // we know that this is exact-in
//                                                     details.origin_swap.amount_in.unwrap_or(origin_swap_tokens.amount_in),
//                                                     price.input_price_usd,
//                                                     price.input_price_sol,
//                                                     &TradeDirection::Sell,
//                                                     Some(moonbag_bps)
//                                                 ) {
//                                                     error!("Error inserting trade history: {}", e);
//                                                 }
//                                                 price_feed.unsubscribe_token(&quote_output.input_token()).await;
//                                             }

//                                             if !STATIC_TOKENS.contains(&quote_output.output_token()) {
//                                                 if let Err(e) = historical_trades.insert_trade(
//                                                     &details.origin_signer,
//                                                     &quote_output.output_token(),
//                                                     out_token_decimals,
//                                                     quote_output.other_amount(),
//                                                     details.origin_swap.amount_out,
//                                                     price.output_price_usd,
//                                                     price.output_price_sol,
//                                                     &TradeDirection::Buy,
//                                                     Some(moonbag_bps)
//                                                 ) {
//                                                     error!("Error inserting trade history: {}", e);
//                                                 }
//                                                 price_feed.unsubscribe_token(&quote_output.output_token()).await;
//                                             }
//                                         }
//                                     }
//                                 }
//                                 ConfirmTransactionResponse::Unconfirmed { reason } => {
//                                     // todo: Retry here is naive. Possible improvements are to increase prio-fees, send to
//                                     // more leaders, etc
//                                     if details.send_txn_retries < send_txn_retries {
//                                         details.send_txn_retries += 1;
//                                         details.next_priority_level = match details.next_priority_level {
//                                             PriorityLevel::Medium => PriorityLevel::High,
//                                             PriorityLevel::High => PriorityLevel::VeryHigh,
//                                             _ => PriorityLevel::High
//                                         };

//                                         match details.retry_strategy {
//                                             RetryStrategy::BuildTransaction => {
//                                                 info!("Txn {} wasn't confirmed. Building again...", txn_id);
//                                                 if let Err(e) = parsed_swap_tx.send(details) {
//                                                     error!("Failed to retry txn {}", txn_id);
//                                                 }
//                                             }
//                                             RetryStrategy::SendRaw(ref tx)=> {
//                                                 let keypair = &[&keypair];
//                                                 info!("Txn {} wasn't confirmed. Sending again...", txn_id);
//                                                 if let Err(e) = send_transactions_handle.resend_transaction(tx.clone(), keypair, txn_id).await {
//                                                     info!("Failed to resend raw txn {}: {}", txn_id, e);
//                                                 } else {
//                                                     // Note: Since we already removed it above, important to insert it again
//                                                     pending_transactions.insert(txn_id, details);
//                                                 }
//                                             }
//                                         }
//                                     } else {
//                                         unconfirmed_copy_txns.fetch_add(1, Ordering::Relaxed);
//                                         let quote_output = details.quote_output.expect("execution output has been set");
//                                         if !STATIC_TOKENS.contains(&quote_output.input_token()) {
//                                             price_feed.unsubscribe_token(&quote_output.input_token()).await;
//                                         }
//                                         if !STATIC_TOKENS.contains(&quote_output.output_token()) {
//                                             price_feed.unsubscribe_token(&quote_output.output_token()).await;
//                                         }
//                                         error!("[Copy service]: Failed to confirm txn {} after {} retry(s)", txn_id, details.send_txn_retries)
//                                     }
//                                 }
//                             }
//                         }
//                         _ = log_tick.tick() => {
//                             info!(
//                                 "[copy txns]: tracked={},processed={},sent={},confirmed={},unconfirmed={},elapsed={}",
//                                 received_tracked_txns.load(Ordering::Relaxed),
//                                 unique_processed_txns.load(Ordering::Relaxed),
//                                 sent_copy_txns.load(Ordering::Relaxed),
//                                 confirmed_copy_txns.load(Ordering::Relaxed),
//                                 unconfirmed_copy_txns.load(Ordering::Relaxed),
//                                 start.elapsed().as_secs()
//                             );
//                         }
//                     }
//                 }

//                 #[allow(unreachable_code)]
//                 Ok(())
//             }
//         })
//     };

//     let tasks = vec![
//         main_task,
//         swap_executor_task,
//         confirmed_transactions_task,
//         price_feed_task,
//     ];
//     Ok(tasks)
// }
