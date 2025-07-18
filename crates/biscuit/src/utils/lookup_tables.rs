use std::collections::HashSet;
use std::sync::Arc;

use futures::{stream, StreamExt, TryStreamExt};
use log::info;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::address_lookup_table::{
    instruction::{create_lookup_table, extend_lookup_table},
    state::AddressLookupTable,
    AddressLookupTableAccount,
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::Transaction;

const DEFAULT_CHUNK_SIZE: usize = 30;

pub async fn create_and_extend_lookup_table(
    keypair: &Keypair,
    rpc_client: &Arc<RpcClient>,
    accounts: HashSet<Pubkey>,
    chunk_size: Option<usize>,
) -> Result<Pubkey, anyhow::Error> {
    let latest_blockhash = rpc_client.get_latest_blockhash().await?;
    let recent_slot = rpc_client.get_slot().await?;

    let (create_ix, alt_pubkey) =
        create_lookup_table(keypair.pubkey(), keypair.pubkey(), recent_slot);

    let mut transaction = Transaction::new_with_payer(&[create_ix], Some(&keypair.pubkey()));
    transaction.try_sign(&[keypair], latest_blockhash)?;

    let signature = rpc_client
        .send_and_confirm_transaction(&transaction)
        .await?;

    info!("Address lookup table creation tx signature: {}", signature);
    info!("Address lookup table address: {}", alt_pubkey);

    let chunk_size = chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE);

    let accounts: Vec<_> = accounts.into_iter().collect();
    for chunk in accounts.chunks(chunk_size) {
        let latest_blockhash = rpc_client.get_latest_blockhash().await?;
        let extend_ix = extend_lookup_table(
            alt_pubkey,
            keypair.pubkey(),
            Some(keypair.pubkey()),
            chunk.to_vec(),
        );

        let mut transaction = Transaction::new_with_payer(&[extend_ix], Some(&keypair.pubkey()));
        transaction.try_sign(&[keypair], latest_blockhash)?;

        let signature = rpc_client
            .send_and_confirm_transaction(&transaction)
            .await?;
        println!("Extended Address lookup table tx signature: {}", signature);
    }

    Ok(alt_pubkey)
}

pub async fn fetch_address_lookup_table(
    rpc_client: &RpcClient,
    address: Pubkey,
) -> anyhow::Result<AddressLookupTableAccount> {
    let raw = rpc_client.get_account_data(&address).await?;
    let data = AddressLookupTable::deserialize(&raw)?;
    Ok(AddressLookupTableAccount {
        key: address,
        addresses: data.addresses.to_vec(),
    })
}

pub async fn fetch_address_lookup_tables(
    rpc_client: &RpcClient,
    alts: impl Iterator<Item = &Pubkey>,
) -> anyhow::Result<Vec<AddressLookupTableAccount>> {
    let keys = alts.copied().collect::<Vec<_>>();
    Ok(rpc_client
        .get_multiple_accounts(&keys)
        .await?
        .into_iter()
        .zip(keys.into_iter())
        .filter_map(|(opt, key)| {
            opt.map::<Option<_>, _>(|acc| {
                Some(AddressLookupTableAccount {
                    key,
                    addresses: AddressLookupTable::deserialize(&acc.data)
                        .ok()
                        .map(|data| data.addresses.to_vec())?,
                })
            })
        })
        .flatten()
        .collect::<Vec<_>>())
}

pub async fn fetch_address_lookup_tables2(
    rpc_client: &RpcClient,
    alts: impl Iterator<Item = &Pubkey>,
) -> anyhow::Result<Vec<AddressLookupTableAccount>> {
    stream::iter(alts)
        .then(|a| fetch_address_lookup_table(rpc_client, *a))
        .try_collect::<Vec<_>>()
        .await
}
