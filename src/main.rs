use bdk::{
    bitcoin::{
        util::psbt::PartiallySignedTransaction, Address, Network, Script, PublicKey
    },
    blockchain::ElectrumBlockchain,
    database::MemoryDatabase,
    descriptor::{Descriptor, policy::BuildSatisfaction},
    electrum_client::Client,
    SyncOptions,
    Wallet,
};

use std::str::FromStr;

// main
// Create new Wallet?
// Create Arb Wallet
//   Seed? xPub?
// Create Maker Wallet
//   Seed? xPub?
// Create Taker Wallet
//   Seed? xPub?
//
// Get Coins
// Maker Wallet Generate and Present Pubkey for new Faucet Coins
// Taker Wallet Generate and Present Pubkey for new Fuacet Coins
//
// Tx 1 Creation
// Maker Flow(wallet, input amount, output amount) -> PBST, pubkey (for multi-sig)
// Taker Flow(wallet, input amount, output amount, maker pubkey, pbst) -> pbst
// Taker Sign(wallet, input amount, output amoutn, pbst) -> pbst
// Maker Sign(wallet, input amount, output amount, pbst) -> pbst
// Maker Broadcast(pbst)
// ?? How does Maker and Taker keep track of the existence of the resulting Multi-Sig?
//
// Check Tx1
// Maker look for confirmation for Tx, checks all relevant balances once complete
// Taker looks for confirmation for Tx, checks all relevant balances once complete
//
// Tx 2 Creation
// Taker Sign(wallet, multi-sig input?, maker output amount, taker output amount) -> pbst
// Maker Sign(wallet, mutli-sig input?, maker output amount, taker output amount) -> Pbst
// Maker Broadcast(pbst)
//
// Check Tx2
// Maker look for confirmation for Tx, checks all relevant balances once complete
// Taker looks for confirmation for Tx, checks all relevant balances once complete


// TODOs:
// 1. Understand xPubs
// 2. Understand Descriptors
// 3. Implement Seed routines and Wallet creation
// 4. Implement Get Coins routines
// 5. Figure out how to top up wallet with testnet coins
// 6. Understand PBST formatting while creating Tx1
// 7. Implement Tx1 Creation
// 8. Implement Tx1 tracking / database?
// 9. Implement Tx1 confirmation check
// 10. Implement Tx2 Creation
// 11. Implement Tx2 confirmation check

fn main() -> Result<(), bdk::Error> {
    let electrum_url = "<electrum_server>";
    let client = Client::new(electrum_url)?;
    let blockchain = ElectrumBlockchain::from(client);

    let descriptor_str = "wpkh([c258d2e4/84h/1h/0h]tpubDDYkZojQFQjht8Tm4jsS3iuEmKjTiEGjG6KnuFNKKJb5A6ZUCUZKdvLdSDWofKi4ToRCwb9poe1XdqfUnP4jaJjCB2Zwv11ZLgSbnZSNecE/0/*)";
    let change_descriptor_str = "wpkh([c258d2e4/84h/1h/0h]tpubDDYkZojQFQjht8Tm4jsS3iuEmKjTiEGjG6KnuFNKKJb5A6ZUCUZKdvLdSDWofKi4ToRCwb9poe1XdqfUnP4jaJjCB2Zwv11ZLgSbnZSNecE/1/*)";

    let descriptor = Descriptor::from_str(&descriptor_str)?;
    let change_descriptor = Descriptor::from_str(&change_descriptor_str)?;

    let wallet = Wallet::new(
        descriptor.clone(),
        Some(change_descriptor.clone()),
        Network::Testnet,
        MemoryDatabase::default(),
    )?;

    wallet.sync(&blockchain, SyncOptions::default())?;

    let input_amt = 100_000;
    let output_amt = 80_000;

    let pubkeys = [
        PublicKey::from_str("<pubkey1>")?,
        PublicKey::from_str("<pubkey2>")?,
    ];
    let multisig_script = Script::new_multisig(2, &pubkeys);
    let multisig_addr = Address::from_script(&multisig_script, Network::Testnet).unwrap();

    let psbt = create_psbt(&wallet, input_amt, output_amt, multisig_addr).await?;

    println!("Created PSBT: {}", base64::encode(&serialize(&psbt)));

    Ok(())
}

async fn create_psbt(
    wallet: &Wallet<MemoryDatabase>,
    input_amt: u64,
    output_amt: u64,
    multisig_addr: Address,
) -> Result<PartiallySignedTransaction, Box<dyn std::error::Error>> {
    // Ensure there are enough funds to cover the input amount
    let balance = wallet.get_balance().await?;
    if balance < input_amt {
        return Err("Insufficient funds".into());
    }

    // Create a transaction builder
    let mut tx_builder = wallet.build_tx();

    // Add the desired output
    tx_builder.add_recipient(multisig_addr.script_pubkey(), output_amt);

    // Set an appropriate fee rate (in satoshis per vbyte)
    tx_builder.set_fee_rate(bdk::FeeRate::from_sat_per_vb(2.0));

    // Use the BranchAndBound coin selection algorithm
    let mut cs = BranchAndBound::new(input_amt, 5);
    tx_builder.coin_selection(cs.as_coin_selection());

    // Enable change output
    tx_builder.set_single_recipient(None);

    // Create the transaction
    let (mut psbt, _details) = tx_builder.finish().unwrap();

    // Sign the transaction
    wallet.sign(&mut psbt, BuildSatisfaction::default())?;

    Ok(psbt)
}
