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
// 6. Understand PBST formatting while creating Tx1
// 7. Implement Tx1 Creation
// 8. Implement Tx1 tracking / database?
// 9. Implement Tx1 confirmation check
// 10. Implement Tx2 Creation
// 11. Implement Tx2 confirmation check

use std::borrow::Borrow;
use std::ops::Deref;

use bdk::bitcoin::{Network, Address};
use bdk::blockchain::{ElectrumBlockchain, GetHeight};
use bdk::database::MemoryDatabase;
use bdk::electrum_client::Client;
use bdk::keys::{DerivableKey, GeneratableKey, GeneratedKey, ExtendedKey, bip39::{Mnemonic, WordCount, Language}};
use bdk::template::Bip84;
use bdk::wallet::coin_selection::{DefaultCoinSelectionAlgorithm, CoinSelectionAlgorithm};
use bdk::wallet::{AddressIndex, tx_builder};
use bdk::{miniscript, Wallet, KeychainKind, SyncOptions, FeeRate, WeightedUtxo, LocalUtxo, Error, Utxo};

fn main() {
    let network = Network::Testnet;
    let client = Client::new("ssl://electrum.blockstream.info:60002").unwrap();
    let blockchain = ElectrumBlockchain::from(client);

    let mut arb_wallet: Option<Wallet<MemoryDatabase>> = None;
    let mut maker_wallet: Option<Wallet<MemoryDatabase>> = None;
    let mut taker_wallet: Option<Wallet<MemoryDatabase>> = None;

    println!("n3x BDK Derisk CLI");

    // listen and process subscriptions

     loop {
        // Sync
        let mut wallets = Vec::<&Wallet<MemoryDatabase>>::new();
        if let Some(arb_wallet) = &arb_wallet {
            wallets.push(arb_wallet);
        }
        if let Some(maker_wallet) = &maker_wallet {
            wallets.push(maker_wallet);
        }
        if let Some(taker_wallet) = &taker_wallet {
            wallets.push(taker_wallet);
        }
        sync_wallets(wallets, &blockchain);

        println!("=> Options");
        println!("  1a. Seed Arbitrator Wallet");
        println!("  1b. Seed Maker Wallet");
        println!("  1c. Seed Taker Wallet");
        println!("  2a. Fund Maker Wallet");
        println!("  2b. Fund Taker Wallet");
        println!("  3a. Query Arbitrator Wallet");
        println!("  3b. Query Maker Wallet");
        println!("  3c. Query Taker Wallet");
        println!("  4a. Create Commit PSBT (Maker)");
        println!("  4b. Create Commit PSBT (Taker)");
        println!("  4c. Sign Commit PSBT (Taker)");
        println!("  4d. Sign Commit PSBT (Maker)");
        println!("  4e. Broadcast Commit Tx (Maker)");
        println!("  5a. Create Payout PSBT (Taker)");
        println!("  5b. Create Payout PSBT (Maker)");
        println!("  5c. Sign Payout PSBT (Maker)");
        println!("  5d. Sign Payout PSBT (Taker)");
        println!("  5e. Broadcast Payout Tx (Taker)");

        let user_input = get_user_input();
        {
            match user_input.as_str() {
                "1a" => arb_wallet = Some(create_wallet(network)),
                "1b" => maker_wallet = Some(create_wallet(network)),
                "1c" => taker_wallet = Some(create_wallet(network)),
                "2a" => fund_wallet(&maker_wallet),
                "2b" => fund_wallet(&taker_wallet),
                "3a" => query_wallet(&arb_wallet),
                "3b" => query_wallet(&maker_wallet),
                "3c" => query_wallet(&taker_wallet),
                _ => println!("Invalid input. Please input a number."),
            }
        }
        println!("");
    }
}

// Common Util

fn get_user_input() -> String {
    let mut input = String::new();
    _ = std::io::stdin().read_line(&mut input).unwrap();
    println!("");

    input.truncate(input.len() - 1);
    input
}

fn sync_wallets(wallets: Vec<&Wallet<MemoryDatabase>>, blockchain: &ElectrumBlockchain) {
    for wallet in wallets {
        wallet.sync(blockchain, SyncOptions::default()).unwrap();
    }
}

// Generate Seeds

fn generate_seeds() -> Mnemonic {
    // Generate fresh mnemonic
    let mnemonic: GeneratedKey<_, miniscript::Segwitv0> = Mnemonic::generate((WordCount::Words12, Language::English)).unwrap();
    // Convert mnemonic to string
    let mnemonic_words = mnemonic.to_string();
    // Parse a mnemonic
    let mnemonic  = Mnemonic::parse(&mnemonic_words).unwrap();

    println!("Generated Seeds: {}", mnemonic_words);
    mnemonic
}

// Seed Wallet

fn create_wallet(network: Network) -> Wallet<MemoryDatabase> {
    println!("Please enter your seed (leave empty to generate new seeds):");
    let seed_string = get_user_input();

    let mnemonic: Mnemonic;
    if seed_string.is_empty() {
        mnemonic = generate_seeds();
    } else {
        mnemonic = Mnemonic::parse(&seed_string).unwrap();
    }

    // Generate the extended key
    let xkey: ExtendedKey = mnemonic.into_extended_key().unwrap();

    // Get xprv from the extended key
    let xprv = xkey.into_xprv(network).unwrap();

    // Create the wallet
    Wallet::new(
        Bip84(xprv, KeychainKind::External),
        Some(Bip84(xprv, KeychainKind::Internal)),
        network,
        MemoryDatabase::default(),
    )
    .unwrap()
}

// Fund Wallet

fn fund_wallet(some_wallet: &Option<Wallet<MemoryDatabase>>) {
    match some_wallet {
        Some(wallet) => {
            // Generate a new receiving address
            let testnet_address = wallet.get_address(AddressIndex::New).unwrap();
            println!("Generated Address: {}", testnet_address.to_string());
        }
        None => println!("Wallet not found.")
    }
}

// Query Wallet

fn query_wallet(some_wallet: &Option<Wallet<MemoryDatabase>>) {
    match some_wallet {
        Some(wallet) => {
            // Get the total wallet balance
            let balance = wallet.get_balance().unwrap();

            // Print the balance
            println!("Total wallet balance: {} satoshis", balance);
        }
        None => println!("Wallet not found.")
    }
}

// Create Commit PSBT
// This only introudce an input into the PSBT

fn get_available_utxos(wallet: &Wallet<MemoryDatabase>) -> Vec<(LocalUtxo, usize)> {
    // WARNING: This assumes that the wallet has enough funds to cover the input amount
    wallet.list_unspent().unwrap()
        .into_iter()
        .map(|utxo| {
            let keychain = utxo.keychain;
            (
                utxo,
                wallet.get_descriptor_for_keychain(keychain)
                    .max_satisfaction_weight()
                    .unwrap(),
            )
        })
        .collect()
}

const COINBASE_MATURITY: u32 = 100;

fn preselect_utxos(wallet: &Wallet<MemoryDatabase>, 
    blockchain: &ElectrumBlockchain, 
    change_policy: tx_builder::ChangeSpendPolicy, 
    must_only_use_confirmed_tx: bool
) -> Result<Vec<WeightedUtxo>, Error> {
    let mut may_spend = get_available_utxos(wallet);

    // Make sure UTXOs at least have minimum number of confirmations
    let satisfies_confirmed = may_spend
        .iter()
        .map(|u| {
            wallet
                .get_tx(&u.0.outpoint.txid, true)
                .map(|tx| match tx {
                    // We don't have the tx in the db for some reason,
                    // so we can't know for sure if it's mature or not.
                    // We prefer not to spend it.
                    None => false,
                    Some(tx) => {
                        // Whether the UTXO is mature and, if needed, confirmed
                        let mut spendable = true;
                        if must_only_use_confirmed_tx && tx.confirmation_time.is_none() {
                            return false;
                        }
                        if tx
                            .transaction
                            .expect("We specifically ask for the transaction above")
                            .is_coin_base()
                        {
                            let current_height = blockchain.get_height().unwrap();
                            match &tx.confirmation_time {
                                Some(t) => {
                                    // https://github.com/bitcoin/bitcoin/blob/c5e67be03bb06a5d7885c55db1f016fbf2333fe3/src/validation.cpp#L373-L375
                                    spendable &= (current_height.saturating_sub(t.height))
                                        >= COINBASE_MATURITY;
                                }
                                None => spendable = false,
                            }
                        }
                        spendable
                    }
                })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut i = 0;
    may_spend.retain(|u| {
        // WARNING: Removed check on Change Policy
        let retain = satisfies_confirmed[i];
        i += 1;
        retain
    });

    let mut may_spend = may_spend
        .into_iter()
        .map(|(local_utxo, satisfaction_weight)| WeightedUtxo {
            satisfaction_weight,
            utxo: Utxo::Local(local_utxo),
        })
        .collect();

    Ok(may_spend)
}

fn create_psbt(some_wallet: &Option<Wallet<MemoryDatabase>>, input_amt: u64) {
    match some_wallet {
        Some(wallet) => {
            // Ensure there are enough funds to cover the input amount
            let balance = wallet.get_balance().unwrap();
            if balance < input_amt {
                print!("Insufficient funds");
                return;
            }

            let coin_selection_result = DefaultCoinSelectionAlgorithm::default().coin_select(
                wallet.database().borrow().deref(),
                vec![],
                wallet.list_unspent(),
                FeeRate::from_sat_per_vb(5.0),
                input_amt,
                0,
            );



            // Create a transaction builder
            let mut tx_builder = wallet.build_tx();

            // Satisfy the specified user amount
            wallet.satisfy_user_amount(&mut tx_builder, input_amt).unwrap();

            // Create the transaction
            let (mut psbt, details) = tx_builder.finish().unwrap();
 
            // Print the PSBT
            println!("PSBT: {}", psbt);

            // Print the transaction details
            println!("Transaction details: {:?}", details);
        }
        None => println!("Wallet not found")
    }
}

// Complete Commit PSBT
// This adds an input into the PSBT, and also expects a 2 of 2 multisig output
fn complete_psbt(some_wallet: &Option<Wallet<MemoryDatabase>>, input_amt: u64, multisig_addr: Address, arb_addr: Address) {
    // Add the desired output
    tx_builder.add_recipient(multisig_addr.script_pubkey(), output_amt);
}
// Complete Commit PSBT with Arbitration HTLC
// This adds an input into the PSBT, and also expects a 2 of 2 multisig output, with the arbitrator getting all funds on HTLC expiry

// Sign Commit PSBT

// Broadcast Commit Tx

