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
use bdk::bitcoin::secp256k1::Secp256k1;
use bdk::bitcoin::util::bip32::{ExtendedPrivKey, ExtendedPubKey, DerivationPath, KeySource};
use bdk::keys::DescriptorKey;
use std::str::FromStr;

use bdk::bitcoin::consensus::{deserialize, encode::serialize};
use bdk::bitcoin::util::psbt::PartiallySignedTransaction;
use bdk::bitcoin::{Network, Script, Address};
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
use miniscript::{Descriptor, Legacy, Segwitv0};

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
        println!("  2a. Generate xPub & xPriv from Arbitrator Wallet - Req key index");
        println!("  2b. Generate Address from Maker Wallet");
        println!("  2c. Generate Address from Taker Wallet");
        println!("  3a. Query Arbitrator Wallet");
        println!("  3b. Query Maker Wallet");
        println!("  3c. Query Taker Wallet");
        println!("  4a. Create Maker Sell PSBT (Maker) - Req Payout amount, Bond amount. Generates Maker xPub & xPriv"); 
        println!("  4b. Complete Maker Sell PSBT (Taker) - Req Maker pubkey, Arb pubkey, Payout amount, Bond amount. Generates Taker contract descriptor");
        println!("  4c. Sign & Broadcast PSBT (Maker) - Req Maker xPrv. Generates Maker contract descriptor");
        println!("  5a. Create Payout PSBT (Taker) - Req Taker contract descriptor, Taker amount, Maker address, Maker amount");
        println!("  5b. Sign & Broadcast Payout PSBT (Maker) - Req Maker contract descriptor");
        println!("  5c. Arbitrate Payout (Arbitrator) - Req Arb xPriv, Maker pubkey, Taker pubkey");

        let user_input = get_user_input();
        {
            match user_input.as_str() {
                "1a" => (arb_wallet, arb_xprv) = create_wallet(network),
                "1b" => (maker_wallet, maker_xprv) = create_wallet(network),
                "1c" => (taker_wallet, taker_xprv) = create_wallet(network),
                "2a" => _ = generate_priv_pub(&arb_wallet, &arb_xprv),
                "2b" => generate_addr(&maker_wallet),
                "2c" => generate_addr(&taker_wallet),
                "3a" => query_wallet(&arb_wallet),
                "3b" => query_wallet(&maker_wallet),
                "3c" => query_wallet(&taker_wallet),
                "4a" => temp_psbt = Some(create_maker_sell_psbt(&maker_wallet, &maker_xprv)),
                "4b" => temp_psbt = Some(complete_maker_sell_psbt(&taker_wallet, &taker_xprv, &temp_psbt)),
                "4c" => sign_broadcast_commit_psbt(&blockchain, &maker_wallet, &temp_psbt),
                "5a" => temp_psbt = Some(create_payout_psbt(&blockchain, &taker_wallet)),
                "5b" => sign_broadcast_payout_psbt(&blockchain, &maker_xprv, &temp_psbt),
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
    a_xprv: String,
    b_pubkey: String,
    network: Network,
    flip: bool,
) -> Wallet<MemoryDatabase> {

    // policy_string = format!("or(10@thresh(2,pk({}),pk({})),and(pk({}),older(576)))", maker_xkey, taker_xkey, arbs_xkey);
    let policy_string = if flip {
        format!("thresh(2,pk({}),pk({}))", b_pubkey, a_xprv)
    } else {
        format!("thresh(2,pk({}),pk({}))", a_xprv, b_pubkey)
    };
    println!("Policy: {}", policy_string);

    let policy = Concrete::<String>::from_str(policy_string.as_str()).unwrap();
    let descriptor = Descriptor::new_wsh(policy.compile().unwrap()).unwrap();
    let descriptor_string = format!("{}", descriptor);
    println!("Wallet Descriptor: {}", descriptor_string);

    Wallet::new(
        descriptor_string.as_str(),
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

fn generate_priv_pub(some_wallet:&Option<Wallet<MemoryDatabase>>, some_xprv:&Option<ExtendedPrivKey>) -> (String, String) {
    let wallet = if let Some(wallet) = some_wallet {
        wallet
    } else {
        println!("Taker Wallet not found");
        return ("".to_string(), "".to_string());
    };

    let xprv = if let Some(xprv) = some_xprv {
        xprv
    } else {
        println!("Taker xprv not found");
        return ("".to_string(), "".to_string());
    };

    // Ask user for agreed upon payout amount
    println!("At what key index?");
    let key_index = get_user_input().parse::<u64>().unwrap();

    let secp = Secp256k1::new();

    // BDK Wallet generates new (testnet) addresses with m/84'/1'/0'/0/* for deposits (Keychain::External) 
    // m/84'/1'/1'/0/* for change (Keychain::Internal). With the first non-hardened index as the 'account' number.
    // 
    // For this example, we are going to generate new keys for multi-sig contract purposes with an account number of 6102.
    // This doesn't necessarily help with wallet backup nor determining address reuse. There's no way for wallet restoration
    // to figure out which is the last used inedex as no key or addresses, not even a hash of it, can be derived from the 
    // blockchain itself. Counterparties' keys are required to even have a chance ot figure out which local keys have been used 
    // and thus the corresponding deriviation paths and private keys to sign.
    // 
    // Wallets must backup actual wallet descriptors that contains xpub used, deriviation paths metadata and counterparty 
    // public keys to have a chance of restoring in-progress trade contracts

    // The following printout code can be used to verify and confirm the above findings

    // let new_address = wallet.get_address(AddressIndex::New).unwrap();
    // println!("Wallet new Address: {}, Type: {}",new_address.to_string(), new_address.address_type().unwrap().to_string());

    // let path = DerivationPath::from_str("m/84'/1'/0'/0/0").unwrap();
    // let derived_xpriv = xprv.derive_priv(&secp, &path).unwrap();
    // let derived_xpub = ExtendedPubKey::from_priv(&secp, &derived_xpriv);
    // let derived_pubkey = PublicKey::new(derived_xpub.public_key);
    // println!("Derived xPub: {}", derived_xpub.to_string());
    // println!("Public Key: {}", derived_xpub.public_key.to_string());
    // println!("P2WPKH Address: {}", Address::p2wpkh(&derived_pubkey, wallet.network()).unwrap().to_string());

    let coin_index = match wallet.network() {
        Network::Bitcoin => 0,
        Network::Testnet => 1,
        Network::Regtest => 3,
        Network::Signet => 2,
    };
    let derivation_string = format!("m/84'/{}'/0'/6102'/{}", coin_index, key_index);
    let path = DerivationPath::from_str(derivation_string.as_str()).unwrap();
    let derived_xpriv = xprv.derive_priv(&secp, &path).unwrap();
    let derived_xpub = ExtendedPubKey::from_priv(&secp, &derived_xpriv);

    println!("Derivation Path: {}", derivation_string);
    println!("Derived xPriv: {}", derived_xpriv.to_string());
    println!("Derived xPub: {}", derived_xpub.to_string());
    (derived_xpriv.to_string(), derived_xpub.to_string())

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

fn create_maker_sell_psbt(some_wallet: &Option<Wallet<MemoryDatabase>>, some_xprv: &Option<ExtendedPrivKey>) -> String {
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
            // println!("PSBT: {:#?}\n", psbt);
            println!("Details: {:#?}\n", details);

            // Serialize and display PSBT
            let encoded_psbt: String = general_purpose::STANDARD_NO_PAD.encode(&serialize(&psbt));
            println!("Encoded PSBT: {}\n", encoded_psbt);

            // Generate and display new Maker xPub & xPriv
            println!("Maker's Derived xPub & xPriv");
            _ = generate_priv_pub(some_wallet, some_xprv);
            
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

    _ = if let Some(xprv) = some_xprv {
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

    // Derive a new xPriv & xPub pair
    println!("Taker's Derived xPub & xPriv");
    let (taker_xpriv, _) = generate_priv_pub(some_wallet, some_xprv);

    // Create a trade wallet using Taker's xprv, Maker's pubkey and Arbitrator's pubkey
    let trade_wallet = create_trade_wallet(taker_xpriv, maker_pubkey, wallet.network(), false);

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
    // println!("PSBT finalized: {}\n{:#?}", finalized, psbt);
    println!("Details: {:#?}\n", details);

    // Serialize and display PSBT
    let encoded_psbt: String = general_purpose::STANDARD_NO_PAD.encode(&serialize(&psbt));
    println!("Encoded PSBT: {}\n", encoded_psbt);

    // Generate and display
    encoded_psbt

}

// 4c. Sign Commit PSBT, probably the Maker

fn sign_broadcast_commit_psbt(blockchain: &ElectrumBlockchain, some_wallet: &Option<Wallet<MemoryDatabase>>, commit_psbt: &Option<String>) {
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

// 5a. Create Payout PSBT, probably by the Taker

fn create_payout_psbt(blockchain: &ElectrumBlockchain, some_wallet: &Option<Wallet<MemoryDatabase>>) -> String {
    let wallet = if let Some(wallet) = some_wallet {
        wallet
    } else {
        println!("Taker Wallet not found");
        return "".to_string();
    };

    // Ask user for Maker pubkey
    println!("What is the Taker contract descriptor?");
    let contract_descriptor = get_user_input();

    // Ask user for Maker address
    println!("What is the Maker's address?");
    let maker_address_string = get_user_input();
    let maker_address = Address::from_str(&maker_address_string).unwrap();
    
    // Ask user for Maker's amount
    println!("What is the Maker's amount?");
    let maker_amount = get_user_input().parse::<u64>().unwrap();

    // Create an address to get the Taker's amount
    let taker_address = wallet.get_address(AddressIndex::New).unwrap();

    // Create a trade wallet using Taker's xprv, Maker's pubkey and Arbitrator's pubkey
    let trade_wallet = Wallet::new(contract_descriptor.as_str(), None, wallet.network(), MemoryDatabase::default()).unwrap();
    trade_wallet.sync(blockchain, SyncOptions::default()).unwrap();

    let trade_balance = trade_wallet.get_balance().unwrap();
    println!("Trade Wallet Balance: {:#?}", trade_balance);

    // Payout the approrpriate amount to the Taker and the Maker accordingly
    let mut builder = trade_wallet.build_tx();
    builder
    .enable_rbf()
    .add_recipient(maker_address.script_pubkey(), maker_amount)
    .drain_to(taker_address.script_pubkey());
    
    let (mut psbt, details) = builder.finish().unwrap();

    let sign_options = SignOptions {
        // try_finalize = false;
        ..Default::default()
    };
    
    // Signs PSBT
    let finalized = trade_wallet.sign(&mut psbt, sign_options).unwrap();
    println!("PSBT finalized: {}\n{:#?}", finalized, psbt);
    println!("Details: {:#?}\n", details);

    // Serialize and display PSBT
    let encoded_psbt: String = general_purpose::STANDARD_NO_PAD.encode(&serialize(&psbt));
    println!("Encoded PSBT: {}\n", encoded_psbt);
    encoded_psbt

}

fn sign_broadcast_payout_psbt(blockchain: &ElectrumBlockchain, some_xprv: &Option<ExtendedPrivKey>, payout_psbt: &Option<String>) {
    let xprv = if let Some(xprv) = some_xprv {
        xprv
    } else {
        println!("Maker xprv not found");
        return;
    };

    let psbt = if let Some(psbt) = payout_psbt {
        psbt
    } else {
        println!("Payout PSBT not found");
        return;
    };

    // See if the PSBT is valid
    let psbt = general_purpose::STANDARD_NO_PAD
        .decode(&psbt)
        .unwrap();
    let mut psbt: PartiallySignedTransaction = deserialize(&psbt).unwrap();

    // What was the Maker's derived xPriv
    println!("What was the Maker's derived xPriv?");
    let maker_xpriv = get_user_input();

    // Ask user for Taker pubkey
    println!("What is the Taker's pubkey?");
    let taker_pubkey = get_user_input();

    // Create a trade wallet using Maker's xprv, Taker's pubkey and Arbitrator's pubkey
    let trade_wallet = create_trade_wallet(maker_xpriv, taker_pubkey, xprv.network, true);
    trade_wallet.sync(blockchain, SyncOptions::default()).unwrap();

    let trade_balance = trade_wallet.get_balance().unwrap();
    println!("Trade Wallet Balance: {:#?}", trade_balance);

    // In reality, Maker should check every aspect of the PSBT before signing. Especially the outputs.

    // Sign and finalize the PSBT
    let sign_options = SignOptions {
        // try_finalize = false;
        ..Default::default()
    };

    let finalized = trade_wallet.sign(&mut psbt, sign_options).unwrap();
    println!("PSBT finalized: {}\n{:#?}", finalized, psbt);

    let psbt_tx = psbt.extract_tx();
    blockchain.broadcast(&psbt_tx).unwrap();
    println!("Broadcasted Tx: {:#?}", psbt_tx);

}