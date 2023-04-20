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
use bdk::bitcoin::blockdata::{script, block};
use miniscript::serde::__private::de;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::ops::Deref;
use std::str::FromStr;

use bdk::bitcoin::consensus::{deserialize, encode::serialize};
use bdk::bitcoin::util::psbt::PartiallySignedTransaction;
use bdk::bitcoin::{Network, Script};
use bdk::blockchain::{ElectrumBlockchain, GetHeight, Blockchain};
use bdk::database::MemoryDatabase;
use bdk::electrum_client::Client;
use bdk::keys::{
    bip39::{Language, Mnemonic, WordCount},
    DerivableKey, ExtendedKey, GeneratableKey, GeneratedKey,
};
use bdk::template::Bip84;
use bdk::wallet::coin_selection::{CoinSelectionAlgorithm, DefaultCoinSelectionAlgorithm};
use bdk::wallet::AddressIndex;
use bdk::{
    miniscript, Error, FeeRate, KeychainKind, LocalUtxo, SyncOptions, Utxo, Wallet, WeightedUtxo, SignOptions,
};

use miniscript::policy::Concrete;
use miniscript::{Descriptor, DefiniteDescriptorKey, DescriptorPublicKey};

fn main() {
    let network = Network::Testnet;
    let client = Client::new("ssl://electrum.blockstream.info:60002").unwrap();
    let blockchain = ElectrumBlockchain::from(client);

    let mut arb_wallet: Option<Wallet<MemoryDatabase>> = None;
    let mut maker_wallet: Option<Wallet<MemoryDatabase>> = None;
    let mut taker_wallet: Option<Wallet<MemoryDatabase>> = None;
    let mut multi_wallet: Option<Wallet<MemoryDatabase>> = None;

    let mut commit_psbt = "".to_string();

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
        println!("  1d. Create Multi-Sig HTLC Wallet");
        println!("  2a. Generate Address from Maker Wallet");
        println!("  2b. Generate Address from Taker Wallet");
        println!("  3a. Query Arbitrator Wallet");
        println!("  3b. Query Maker Wallet");
        println!("  3c. Query Taker Wallet");
        println!("  4a. Create Maker Sell PSBT (Maker)");
        println!("  4b. Complete Maker Sell PSBT (Taker)");
        println!("  4c. Sign PSBT (Maker)");
        println!("  4d. Broadcast Signed Tx (Maker)");
        println!("  4e. Print Current PSBT");
        println!("  5a1. Create Maker Sell Payout PSBT (Maker)");
        println!("  5a2. Create Payout PSBT (Taker)");
        println!("  5b1. Sign Payout PSBT (Maker)");
        println!("  5b2. Sign Payout PSBT (Taker)");
        println!("  5c1. Broadcast Payout Tx (Maker)");
        println!("  5c2. Broadcast Payout Tx (Taker)");

        let user_input = get_user_input();
        {
            match user_input.as_str() {
                "1a" => arb_wallet = Some(create_wallet(network)),
                "1b" => maker_wallet = Some(create_wallet(network)),
                "1c" => taker_wallet = Some(create_wallet(network)),
                "1d" => {
                    multi_wallet = Some(create_multisig_wallet(
                        &maker_wallet,
                        &taker_wallet,
                        Network::Testnet
                    ))
                }
                "2a" => generate_addr(&maker_wallet),
                "2b" => generate_addr(&taker_wallet),
                "3a" => query_wallet(&arb_wallet),
                "3b" => query_wallet(&maker_wallet),
                "3c" => query_wallet(&taker_wallet),
                "4a" => commit_psbt = create_maker_sell_psbt(&maker_wallet),
                "4b" => commit_psbt = complete_maker_sell_psbt(&taker_wallet, &multi_wallet, &commit_psbt),
                "4c" => commit_psbt = sign_psbt(&maker_wallet, &commit_psbt),
                "4d" => broadcast_psbt(&blockchain, &commit_psbt),
                "4e" => print_psbt(&commit_psbt),
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
    let mnemonic: GeneratedKey<_, miniscript::Segwitv0> =
        Mnemonic::generate((WordCount::Words12, Language::English)).unwrap();
    // Convert mnemonic to string
    let mnemonic_words = mnemonic.to_string();
    // Parse a mnemonic
    let mnemonic = Mnemonic::parse(&mnemonic_words).unwrap();

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

// Create Multisig HTLC Wallet. 
// Doing implemenation for a watch only wallet for now.
// Will add capability for one of them to be a signing wallet later.

fn extract_substring_between_brackets(input: &str) -> Option<String> {
    let start = input.find('(')?;
    let end = input.find(')')?;

    if start < end {
        Some(input[start + 1..end].to_string())
    } else {
        None
    }
}

fn get_new_descriptor(
    wallet: &Wallet<MemoryDatabase>,
    keychain: KeychainKind,
) -> String {
    let new_address = wallet.get_address(AddressIndex::New).unwrap();
    let script_pubkey = new_address.script_pubkey();
    let secp = wallet.secp_ctx();
    let xpub = wallet.get_descriptor_for_keychain(keychain);
    let (_, descriptor) = xpub.find_derivation_index_for_spk(secp, &script_pubkey, 0..100).unwrap().unwrap();

    println!("New Descriptor: {}", descriptor);
    println!("Original Adddress: {}, Derived Address: {}", new_address, descriptor.address(wallet.network()).unwrap());

    let extracted_key = extract_substring_between_brackets(descriptor.to_string().as_str()).unwrap();
    println!("Extracted Key: {}", extracted_key);
    extracted_key
}

fn create_multisig_htlc_wallet(
    maker_wallet: &Option<Wallet<MemoryDatabase>>,
    taker_wallet: &Option<Wallet<MemoryDatabase>>,
    arb_wallet: &Option<Wallet<MemoryDatabase>>,
    network: Network,
) -> Wallet<MemoryDatabase> {
    let maker_xkey = match maker_wallet {
        Some(wallet) => get_new_descriptor(wallet, KeychainKind::External),
        None => panic!("Maker Wallet not found."),
    };
    let taker_xkey = match taker_wallet {
        Some(wallet) => get_new_descriptor(wallet, KeychainKind::External),
        None => panic!("Taker Wallet not found."),
    };
    let arbs_xkey = match arb_wallet {
        Some(wallet) => get_new_descriptor(wallet, KeychainKind::External),
        None => panic!("Arbitrator Wallet not found."),
    };

    // Handcoding this policy. Compiled and verified using https://bitcoin.sipa.be/miniscript/
    // Policy: or(10@thresh(2,pk(A),pk(B)),and(pk(C), older(576)))
    // Miniscript: or_i(and_v(v:pkh(C),older(576)),and_v(v:pk(A),pk(B)))
    // Descriptor: wsh(or_i(and_v(v:pkh(C),older(576)),and_v(v:pk(A),pk(B))))

    // let descriptor = format!("wsh(or_i(and_v(v:pkh({}),older(576)),and_v(v:pk({}),pk({}))))", arbs_xkey.to_string(), maker_xkey.to_string(), taker_xkey.to_string());
    let policy_string = format!("or(10@thresh(2,pk({}),pk({})),and(pk({}),older(576)))", maker_xkey, taker_xkey, arbs_xkey);
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

fn create_multisig_wallet(
    a_wallet: &Option<Wallet<MemoryDatabase>>,
    b_wallet: &Option<Wallet<MemoryDatabase>>,
    network: Network,
) -> Wallet<MemoryDatabase> {
    let a_xkey = match a_wallet {
        Some(wallet) => get_new_descriptor(wallet, KeychainKind::External),
        None => panic!("A Wallet not found."),
    };
    let b_xkey = match b_wallet {
        Some(wallet) => get_new_descriptor(wallet, KeychainKind::External),
        None => panic!("B Wallet not found."),
    };

    let policy_string = format!("thresh(2,pk({}),pk({}))", a_xkey, b_xkey);
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

// Generate Address (to fund Wallet)

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

// Query Wallet

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

// Select UTXO (not under use consideration)

fn get_available_utxos(wallet: &Wallet<MemoryDatabase>) -> Vec<(LocalUtxo, usize)> {
    // WARNING: This assumes that the wallet has enough funds to cover the input amount
    wallet
        .list_unspent()
        .unwrap()
        .into_iter()
        .map(|utxo| {
            let keychain = utxo.keychain;
            (
                utxo,
                wallet
                    .get_descriptor_for_keychain(keychain)
                    .max_satisfaction_weight()
                    .unwrap(),
            )
        })
        .collect()
}

const COINBASE_MATURITY: u32 = 100;

fn preselect_utxos(
    wallet: &Wallet<MemoryDatabase>,
    blockchain: &ElectrumBlockchain,
    must_only_use_confirmed_tx: bool,
) -> Result<Vec<WeightedUtxo>, Error> {
    let mut may_spend = get_available_utxos(wallet);

    // Make sure UTXOs at least have minimum number of confirmations
    let satisfies_confirmed = may_spend
        .iter()
        .map(|u| {
            wallet.get_tx(&u.0.outpoint.txid, true).map(|tx| match tx {
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
                                spendable &=
                                    (current_height.saturating_sub(t.height)) >= COINBASE_MATURITY;
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
    may_spend.retain(|_u| {
        // WARNING: Removed check on Change Policy
        let retain = satisfies_confirmed[i];
        i += 1;
        retain
    });

    let may_spend = may_spend
        .into_iter()
        .map(|(local_utxo, satisfaction_weight)| WeightedUtxo {
            satisfaction_weight,
            utxo: Utxo::Local(local_utxo),
        })
        .collect();

    Ok(may_spend)
}

fn select_utxos(
    some_wallet: &Option<Wallet<MemoryDatabase>>,
    blockchain: &ElectrumBlockchain,
    input_amt: u64,
) {
    match some_wallet {
        Some(wallet) => {
            // Ensure there are enough funds to cover the input amount
            let balance = wallet.get_balance().unwrap();
            if balance.confirmed < input_amt {
                print!("Insufficient funds");
                return;
            }

            let optional_utxos = preselect_utxos(wallet, &blockchain, true).unwrap();

            let coin_selection_result = DefaultCoinSelectionAlgorithm::default().coin_select(
                wallet.database().borrow().deref(),
                vec![],
                optional_utxos,
                FeeRate::from_sat_per_vb(5.0),
                input_amt,
                &Script::default(),
            );

            println!("Coin Selection Result: {:?}", coin_selection_result);
        }
        None => println!("Wallet not found"),
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

            // Create PSBT with a fixed fee as the total amount, receipient as the change address
            let mut builder = wallet.build_tx();
            builder
                .enable_rbf()
                .fee_absolute(payout_amount + bond_amount)
                .add_recipient(Script::new_op_return(&[]), 0);
            let (psbt, details) = builder.finish().unwrap();
            println!("PSBT: {:#?}", psbt);

            // Serialize and display PSBT
            let encoded_psbt: String = general_purpose::STANDARD_NO_PAD.encode(&serialize(&psbt));
            println!("Encoded PSBT: {}", encoded_psbt);
            println!("");
            println!("Details: {:#?}", details);

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

fn complete_maker_sell_psbt(taker_wallet: &Option<Wallet<MemoryDatabase>>, multi_wallet:&Option<Wallet<MemoryDatabase>>, commit_psbt: &String) -> String {
    let taker_wallet = if let Some(wallet) = taker_wallet {
        wallet
    } else {
        println!("Taker Wallet not found");
        return "".to_string();
    };

    let multi_wallet = if let Some(wallet) = multi_wallet {
        wallet
    } else {
        println!("Multi Wallet not found");
        return "".to_string();
    };

    // See if the PSBT is valid
    let psbt = general_purpose::STANDARD_NO_PAD
        .decode(&commit_psbt)
        .unwrap();
    let psbt: PartiallySignedTransaction = deserialize(&psbt).unwrap();

    // Ask user for the agree upon payout amount
    println!("What is the payout amount?");
    let payout_amount = get_user_input().parse::<u64>().unwrap();

    // Ask user for the agreed upon taker bond amount
    println!("What is the taker's bond amount?");
    let bond_amount = get_user_input().parse::<u64>().unwrap();

    // We are assuming the the arbitrator & maker pubkeys are passed in
    // And in combination with the taker pubkey, we creatd a Multisig + HTLC wallet
    // When in fact we have pre-created the wallet in step 1d.

    // Get Receipient Address from Mutlisig/HTLC wallet
    let multi_wallet_address = multi_wallet.get_address(AddressIndex::New).unwrap();

    // Make created Script receipient to total amount (payout + bonds)
    let mut builder = taker_wallet.build_tx();
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
    let finalized = taker_wallet.sign(&mut psbt, sign_options).unwrap();
    println!("PSBT finalized: {}\n{:#?}", finalized, psbt);

    // Serialize and display PSBT
    let encoded_psbt: String = general_purpose::STANDARD_NO_PAD.encode(&serialize(&psbt));
    println!("Encoded PSBT: {}", encoded_psbt);
    println!("");
    println!("Details: {:#?}", details);
    encoded_psbt

}

// 4c. Sign Commit PSBT, probably the Maker

fn sign_psbt(some_wallet: &Option<Wallet<MemoryDatabase>>, commit_psbt: &String) -> String {
    let wallet = if let Some(wallet) = some_wallet {
        wallet
    } else {
        println!("Maker Wallet not found");
        return "".to_string();
    };

    // See if the PSBT is valid
    let psbt = general_purpose::STANDARD_NO_PAD
        .decode(&commit_psbt)
        .unwrap();
    let mut psbt: PartiallySignedTransaction = deserialize(&psbt).unwrap();

    // Sign and finalize the PSBT
    let sign_options = SignOptions {
        // try_finalize = false;
        ..Default::default()
    };

    let finalized = wallet.sign(&mut psbt, sign_options).unwrap();
    println!("PSBT finalized: {}\n{:#?}", finalized, psbt);

    // Serialize and display PSBT
    let encoded_psbt = general_purpose::STANDARD_NO_PAD.encode(&serialize(&psbt));
    println!("Encoded PSBT: {}", encoded_psbt);
    encoded_psbt

}

// 4d. Broadcast Commit Tx

fn broadcast_psbt(blockchain: &ElectrumBlockchain, commit_psbt: &String) {

    // See if the PSBT is valid
    let psbt = general_purpose::STANDARD_NO_PAD
        .decode(&commit_psbt)
        .unwrap();
    let psbt: PartiallySignedTransaction = deserialize(&psbt).unwrap();

    blockchain.broadcast(&psbt.extract_tx()).unwrap();
}

fn print_psbt(commit_psbt: &String) {
    // See if the PSBT is valid
    let psbt = general_purpose::STANDARD_NO_PAD
        .decode(&commit_psbt)
        .unwrap();
    let psbt: PartiallySignedTransaction = deserialize(&psbt).unwrap();

    println!("PartiallySignedTransaction: {:#?}", psbt);
    println!("");
}