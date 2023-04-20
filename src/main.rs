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

use base64::{engine::general_purpose, Engine as _};
use bdk::bitcoin::util::bip32::ExtendedPrivKey;
use std::str::FromStr;

use bdk::bitcoin::consensus::{deserialize, encode::serialize};
use bdk::bitcoin::util::psbt::PartiallySignedTransaction;
use bdk::bitcoin::{Network, Script};
use bdk::blockchain::{ElectrumBlockchain, Blockchain};
use bdk::database::MemoryDatabase;
use bdk::electrum_client::Client;
use bdk::keys::{
    bip39::{Language, Mnemonic, WordCount},
    DerivableKey, ExtendedKey, GeneratableKey, GeneratedKey,
};
use bdk::template::Bip84;
use bdk::wallet::AddressIndex;
use bdk::{
    miniscript, KeychainKind, SyncOptions, Wallet, SignOptions,
};

use miniscript::policy::Concrete;
use miniscript::Descriptor;

fn main() {
    let network = Network::Testnet;
    let client = Client::new("ssl://electrum.blockstream.info:60002").unwrap();
    let blockchain = ElectrumBlockchain::from(client);

    let mut arb_wallet: Option<Wallet<MemoryDatabase>> = None;
    let mut maker_wallet: Option<Wallet<MemoryDatabase>> = None;
    let mut taker_wallet: Option<Wallet<MemoryDatabase>> = None;

    let mut arb_xprv: Option<ExtendedPrivKey> = None;
    let mut maker_xprv: Option<ExtendedPrivKey> = None;
    let mut taker_xprv: Option<ExtendedPrivKey> = None;

    let mut temp_psbt: Option<String> = None;

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
        println!("  2a. Generate Pubkey from Arbitrator Wallet");
        println!("  2b. Generate Address from Maker Wallet");
        println!("  2c. Generate Address from Taker Wallet");
        println!("  3a. Query Arbitrator Wallet");
        println!("  3b. Query Maker Wallet");
        println!("  3c. Query Taker Wallet");
        println!("  4a. Create Maker Sell PSBT (Maker)");  // Needs Payout amount, Bond amount. Also generates Maker pubkey
        println!("  4b. Complete Maker Sell PSBT (Taker)");  // Needs Maker pubkey, Arb pubkey, Payout Amount, Bond Amount
        println!("  4c. Sign & Broadcast PSBT (Maker)");
        println!("  5a. Create Payout PSBT (Taker)");  // Needs Maker pubkey, Maker address, Maker amount, Taker amount
        println!("  5b. Sign & Broadcast Payout PSBT (Maker)");  // Needs Taker pubkey
        println!("  5c. Arbitrate Payout (Arbitrator)"); // Needs Taker pubkey, Maker pubkey, Payout address

        let user_input = get_user_input();
        {
            match user_input.as_str() {
                "1a" => (arb_wallet, arb_xprv) = create_wallet(network),
                "1b" => (maker_wallet, maker_xprv) = create_wallet(network),
                "1c" => (taker_wallet, taker_xprv) = create_wallet(network),
                "2a" => _ = generate_pubkey(&arb_wallet),
                "2b" => generate_addr(&maker_wallet),
                "2c" => generate_addr(&taker_wallet),
                "3a" => query_wallet(&arb_wallet),
                "3b" => query_wallet(&maker_wallet),
                "3c" => query_wallet(&taker_wallet),
                "4a" => temp_psbt = Some(create_maker_sell_psbt(&maker_wallet)),
                "4b" => temp_psbt = Some(complete_maker_sell_psbt(&taker_wallet, &taker_xprv, &temp_psbt)),
                "4c" => sign_broadcast_psbt(&blockchain, &maker_wallet, &temp_psbt),
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

fn create_trade_wallet(
    a_xprv: &ExtendedPrivKey,
    b_pubkey: String,
    network: Network
) -> Wallet<MemoryDatabase> {

    // policy_string = format!("or(10@thresh(2,pk({}),pk({})),and(pk({}),older(576)))", maker_xkey, taker_xkey, arbs_xkey);
    let policy_string = format!("thresh(2,pk({}),pk({}))", a_xprv.to_string(), b_pubkey);
    println!("Policy: {}", policy_string);

    let policy = Concrete::<String>::from_str(policy_string.as_str()).unwrap();
    let descriptor = Descriptor::new_wsh(policy.compile().unwrap()).unwrap();

    Wallet::new(
        &format!("{}", descriptor),
        None,
        network,
        MemoryDatabase::default(),
    )
    .unwrap()
}

// 1. Generate Seeds

fn generate_seeds() -> Mnemonic {
    // Generate fresh mnemonic
    let mnemonic: GeneratedKey<_, miniscript::Segwitv0> =
        Mnemonic::generate((WordCount::Words12, Language::English)).unwrap();
    // Convert mnemonic to string
    let mnemonic_words = mnemonic.to_string();
    // Parse a mnemonic
    let mnemonic = Mnemonic::parse(&mnemonic_words).unwrap();

    println!("Generated Seeds: {}", mnemonic_words);
    mnemonic
}

// 1. Seed Wallet

fn create_wallet(network: Network) -> (Option<Wallet<MemoryDatabase>>, Option<ExtendedPrivKey>) {
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
    let wallet = Wallet::new(
        Bip84(xprv, KeychainKind::External),
        Some(Bip84(xprv, KeychainKind::Internal)),
        network,
        MemoryDatabase::default(),
    )
    .unwrap();

    (Some(wallet), Some(xprv))

}

// 2a. Generate Pubkey

fn extract_substring_between_brackets(input: &str) -> Option<String> {
    let start = input.find('(')?;
    let end = input.find(')')?;

    if start < end {
        Some(input[start + 1..end].to_string())
    } else {
        None
    }
}

fn generate_pubkey(some_wallet: &Option<Wallet<MemoryDatabase>>) -> String {
    let wallet = match some_wallet {
        Some(wallet) => wallet,
        None => {
            println!("Wallet not found.");
            return "".to_string();
        }
    };
    let new_address = wallet.get_address(AddressIndex::New).unwrap();
    let script_pubkey = new_address.script_pubkey();
    let secp = wallet.secp_ctx();
    let xpub = wallet.get_descriptor_for_keychain(KeychainKind::External);
    let (_, descriptor) = xpub.find_derivation_index_for_spk(secp, &script_pubkey, 0..100).unwrap().unwrap();

    println!("New Descriptor: {}", descriptor);
    println!("Original Adddress: {}, Derived Address: {}", new_address, descriptor.address(wallet.network()).unwrap());

    let extracted_key = extract_substring_between_brackets(descriptor.to_string().as_str()).unwrap();
    println!("Extracted PubKey: {}", extracted_key);
    extracted_key
}

// 2b. Generate Address

fn generate_addr(some_wallet: &Option<Wallet<MemoryDatabase>>) {
    match some_wallet {
        Some(wallet) => {
            // Generate a new receiving address
            let testnet_address = wallet.get_address(AddressIndex::New).unwrap();
            println!("Generated Address: {}", testnet_address.to_string());
        }
        None => println!("Wallet not found."),
    }
}

// 3. Query Wallet

fn query_wallet(some_wallet: &Option<Wallet<MemoryDatabase>>) {
    match some_wallet {
        Some(wallet) => {
            // Get the total wallet balance
            let balance = wallet.get_balance().unwrap();

            // Print the balance
            println!("Total wallet balance: {} satoshis", balance);

            let transactions = wallet.list_transactions(false).unwrap();

            // Print all the transactions
            println!("Transactions: {:#?}", transactions);
        }
        None => println!("Wallet not found."),
    }
}

// 4a. Maker Create Initial Commit PSBT
// Maker will only add the it's input and corresponding change output
// Taker will complete the transaction by adding the 2nd input and also the multi-sig output

fn create_maker_sell_psbt(some_wallet: &Option<Wallet<MemoryDatabase>>) -> String {
    match some_wallet {
        Some(wallet) => {
            // Ask user for agreed upon payout amount
            println!("What is the payout amount?");
            let payout_amount = get_user_input().parse::<u64>().unwrap();

            // Ask user for agreed upon maker bond amount
            println!("What is the maker's bond amount?");
            let bond_amount = get_user_input().parse::<u64>().unwrap();

            // Create PSBT with a fixed fee as the total amount
            // The wallet will automatically select UTXO and add the necessary change output
            let mut builder = wallet.build_tx();
            builder
                .enable_rbf()
                .fee_absolute(payout_amount + bond_amount)
                .add_recipient(Script::new_op_return(&[]), 0);
            let (psbt, details) = builder.finish().unwrap();
            println!("PSBT: {:#?}\n", psbt);
            println!("Details: {:#?}\n", details);

            // Serialize and display PSBT
            let encoded_psbt: String = general_purpose::STANDARD_NO_PAD.encode(&serialize(&psbt));
            println!("Encoded PSBT: {}\n", encoded_psbt);

            // Generate and display new Maker pubkey
            let maker_pubkey = generate_pubkey(some_wallet);
            println!("Maker's Pubkey: {}\n", maker_pubkey);
            encoded_psbt
        }
        None => {
            println!("Wallet not found");
            return "".to_string();
        }
    }
}

// 4b. Taker completes the PSBT
// Taker cannot confirm the Maker's input amount or weight of the UTXO
// However the Taker will also specify more output amount than the amount of input its going to fund
// So the Maker cannot defraud the Taker in any case

fn complete_maker_sell_psbt(some_wallet: &Option<Wallet<MemoryDatabase>>, some_xprv: &Option<ExtendedPrivKey>, commit_psbt: &Option<String>) -> String {
    let wallet = if let Some(wallet) = some_wallet {
        wallet
    } else {
        println!("Taker Wallet not found");
        return "".to_string();
    };

    let xprv = if let Some(xprv) = some_xprv {
        xprv
    } else {
        println!("Taker xprv not found");
        return "".to_string();
    };

    let psbt = if let Some(psbt) = commit_psbt {
        psbt
    } else {
        println!("Commit PSBT not found");
        return "".to_string();
    };

    // See if the PSBT is valid
    let psbt = general_purpose::STANDARD_NO_PAD
        .decode(&psbt)
        .unwrap();
    let psbt: PartiallySignedTransaction = deserialize(&psbt).unwrap();

    // Ask user for maker pubkey
    println!("What is the maker's pubkey?");
    let maker_pubkey = get_user_input();

    // Ask user for arbitrator's pubkey

    // Ask user for the agree upon payout amount
    println!("What is the payout amount?");
    let payout_amount = get_user_input().parse::<u64>().unwrap();

    // Ask user for the agreed upon taker bond amount
    println!("What is the taker's bond amount?");
    let bond_amount = get_user_input().parse::<u64>().unwrap();

    // Create a trade wallet using Taker's xprv, Maker's pubkey and Arbitrator's pubkey
    let trade_wallet = create_trade_wallet(xprv, maker_pubkey, xprv.network);

    // Get Receipient Address from Mutlisig/HTLC wallet
    let multi_wallet_address = trade_wallet.get_address(AddressIndex::New).unwrap();

    // Make created Script recipient to total amount (payout + bonds)
    let mut builder = wallet.build_tx();
    builder.enable_rbf();
    builder.add_recipient(multi_wallet_address.script_pubkey(), payout_amount + 2*bond_amount);
    
    // We need to combine this with the Maker's PSBT...
    for i in 0..psbt.inputs.len() {
        let outpoint = psbt.unsigned_tx.input[i].previous_output.clone();
        let input = psbt.inputs[i].clone();
        let weight = 4 + 1 + 73 + 33; // p2wpkh weight from https://github.com/GoUpNumber/gun/blob/4ac6cac5afb6923615d1e9cef2f8704d6cd34c1e/src/betting/wallet_impls/offer.rs#L89. Don't know why exactly
        builder.add_foreign_utxo(outpoint, input, weight).unwrap(); // Catch Error?
    }

    for i in 0..psbt.outputs.len() {
        let output = &psbt.unsigned_tx.output[i];
        if output.value > 0 {
            builder.add_recipient(output.script_pubkey.clone(), output.value);
        }
    }
    
    // We assume here that the Taker wallet will take care of
    //   1. Adding sufficient UTXOs as input to satisfy the amount specified in the Multi-sig output along with the Maker's change output
    //   2. Adding a change output for the Taker
    let (mut psbt, details) = builder.finish().unwrap();

    let sign_options = SignOptions {
        // try_finalize = false;
        ..Default::default()
    };
    
    // Taker signs PSBT
    let finalized = wallet.sign(&mut psbt, sign_options).unwrap();
    println!("PSBT finalized: {}\n{:#?}", finalized, psbt);
    println!("Details: {:#?}\n", details);

    // Serialize and display PSBT
    let encoded_psbt: String = general_purpose::STANDARD_NO_PAD.encode(&serialize(&psbt));
    println!("Encoded PSBT: {}\n", encoded_psbt);

    // Generate and display new Maker pubkey
    let maker_pubkey = generate_pubkey(some_wallet);
    println!("Taker's Pubkey: {}\n", maker_pubkey);
    encoded_psbt

}

// 4c. Sign Commit PSBT, probably the Maker

fn sign_broadcast_psbt(blockchain: &ElectrumBlockchain, some_wallet: &Option<Wallet<MemoryDatabase>>, commit_psbt: &Option<String>) {
    let wallet = if let Some(wallet) = some_wallet {
        wallet
    } else {
        println!("Maker Wallet not found");
        return;
    };

    let psbt = if let Some(psbt) = commit_psbt {
        psbt
    } else {
        println!("Commit PSBT not found");
        return;
    };

    // See if the PSBT is valid
    let psbt = general_purpose::STANDARD_NO_PAD
        .decode(&psbt)
        .unwrap();
    let mut psbt: PartiallySignedTransaction = deserialize(&psbt).unwrap();

    // Sign and finalize the PSBT
    let sign_options = SignOptions {
        // try_finalize = false;
        ..Default::default()
    };

    let finalized = wallet.sign(&mut psbt, sign_options).unwrap();
    println!("PSBT finalized: {}\n{:#?}", finalized, psbt);

    let psbt_tx = psbt.extract_tx();
    blockchain.broadcast(&psbt_tx).unwrap();
    println!("Broadcasted Tx: {:#?}", psbt_tx);

}