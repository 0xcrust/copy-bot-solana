use solana_sdk::signature::Keypair;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    let keypair = Keypair::from_base58_string("").to_bytes();
    println!("keypair: {:?}", keypair);
    //let keypair = Keypair::from_bytes(&[])?.to_base58_string();
    // println!("keypair string: {}", keypair);

    Ok(())
}
