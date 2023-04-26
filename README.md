# BDK Derisk Playground for n3xB

This is a small command line executable to derisk flows needed to implement the on-chain multi-sig trade mechanic proposed as one of the trade mechanics on top for n3xB.

## Install

```
cargo run
```

## Usage

The executable environment stores very little in memory and nothing on disk. Aside from seeding the keys and creating the wallet, the only thing saved temporarily in memory are PSBTs generated for the relevant steps. This is because its too long to be input through the command line prompt. Everything else is assumed to be output to screen and meant to be saved by copying and pasting to a note taking solution of choice. Inputs required for each executable Option is described right after the description for each command.

Once ran, you will see a list of Options as such:

```
n3x BDK Derisk CLI
=> Options
  1a. Seed Arbitrator Wallet
  1b. Seed Maker Wallet
  1c. Seed Taker Wallet
  2a. Generate xPub & xPriv from Arbitrator Wallet - Req key index
  2b. Generate Address from Maker Wallet
  2c. Generate Address from Taker Wallet
  3a. Query Arbitrator Wallet
  3b. Query Maker Wallet
  3c. Query Taker Wallet
  4a. Create Maker Sell PSBT (Maker) - Req Payout amount, Bond amount. Generates Maker xPub & xPriv
  4b. Complete Maker Sell PSBT (Taker) - Req Maker pubkey, Arb pubkey, Payout amount, Bond amount. Generates Taker xPub & xPriv & output descriptor
  4c. Sign & Broadcast PSBT (Maker)
  5a. Create Payout PSBT (Taker) - Req Taker output descriptor, Maker address, Bond amount
  5b. Sign & Broadcast Payout PSBT (Maker) - Req Maker xPrv, Taker pubkey, Arbitrator pubkey
  5c. Arbitrate Payout (Arbitrator) - Req Arbitrator xPriv, Maker pubkey, Taker pubkey
|
```

To execute each Option, input only the number and letter for the Option.

There's no checks or error handling for any incorrect inputs. Executable will crash on invalid or unexpected inputs.

## Flows

*Save* in all steps refers to copy and pasting to a document or note taker of choice.

### Create and fund wallets

1. Create new Arbitrator, Maker & Taker wallets using **1a**, **1b** and **1c**. Save generated Seeds
2. Use **2b** to generate address from Maker wallet and fund wallet with a Bitcoin Testnet Faucet
3. Use **2c** to generate address from Taker wallet and fund wallet with a Bitcoin Testnet Faucet
4. Wait for at least 1 confirmation from the Testnet faucet funding transactions
5. Use **3b** and **3c** to check that wallets have been funded

### Maker sells, Taker buys

Create 2 of 2 multi-sig + timelock for Arbitration. Payout from the 2 of 2 the amount to Taker and return bonds to both Maker and Taker. This assumes that Maker will contribute amount `ma` and bond `mb` to fund the transaction, and the Taker to contribute `nb` to fund the transaction. Recommend that `mb` to be half of `ma`, and `nb` be same as `ma`. Make sure the respective wallets has sufficient funds for the amount, bonds and room for fee. Assumption is that all fees will be paid by the Taker. Also recommends `ma` to be over 10000 sats. For test purposes, the timelock is set to 6 blocks (1 hour). A longer timelock is recommended before arbitration is allowed for real deployment.

#### **Funding tx to lock amount and bonds**

1. Restore Arbitrator, Maker & Taker wallets using **1a**, **1b** and **1c** using the saved Seeds if needed.
2. Use **4a** to let the Maker wallet create a new PSBT for the funding Transaction. Recommend to start key index at 0, but to avoid key reuse by incrementing from 0 for subsequent transactions. Save derived Maker xPriv and xPub. Resulting PSBT will be saved in temporary store in memory so copy & pasting of that is not required.
3. Use **2a** to derive Arbitrator xPriv and xPub. Keep key index the same as step 2 for simplicity, but not required to work. Save derived Arbitrator xPriv and xPub
4. Use **4b** to let the Taker wallet to complete & sign the PSBT. Again, keep key index the same for simplicity, but no required. Resulting PSBT will be saved in temporary store in memory so copy & pasting of that is not required. But save the derived Taker xPriv and xPub. Also save the output descriptor for the multi-sig output.
5. Use **4c** to have the Maker wallet sign the completed PSBT. If successful it will be broadcasted to Testnet.
6. You can find the TxID for the funding transaction by using **3b** or **3c** and look for either the latest, or the unconfirmed transaction. It should say `confirmation_time: None`.

[Example TxID - c19345415e93af33096c00f06b90f03bb71102fdf98d23746fc3777f360231b6](https://mempool.space/testnet/tx/c19345415e93af33096c00f06b90f03bb71102fdf98d23746fc3777f360231b6)

#### **Multi-sig tx to payout the amount and return the bonds**

Make sure the funding transaction have at least 1 confirmation.
1. Restore Arbitrator, Maker & Taker wallets using **1a**, **1b** and **1c** using the saved Seeds if needed.
2. Use **2b** to generate a new address from the maker wallet to get the bond back
3. Use **5a** to create the Multi-sig payout tx.
4. Use **5b** to sign and broadcast the payout PSBT.
5. You can find the TxID for the funding transaction by using **3b** or **3c** and look for either the latest, or the unconfirmed transaction. It should say `confirmation_time: None`.
6. Confirmed balance of Maker and Taker wallet should increase as expected once the payout transaction has at least 1 confirmation.

[Example TxID - 37c648e8e81838a3459562d2049ac0cc1e74e45f3b7961b2ff6688f3b54affea](https://mempool.space/testnet/tx/37c648e8e81838a3459562d2049ac0cc1e74e45f3b7961b2ff6688f3b54affea)

#### **Arbitrator sweeps funds as timelock hits**

The assumption here is we have a funded multi-sig that has not been paid out. This flow is the alternative scenario to the above flow **Multi-sig tx to payout the amount and return the bonds**.
1. Restore Arbitrator, Maker & Taker wallets using **1a**, **1b** and **1c** using the saved Seeds if needed. At least 6 blocks of confirmations for the funding transaction is required before the flow can be executed. You should see a failure if one attempts this before 6 blocks have elapsed.
1. Restore Arbitrator, Maker & Taker wallets using **1a**, **1b** and **1c** using the saved Seeds if needed.
2. Use **5c** to create, sign and broadcast the Arbitrator payout transaction.
3. You can find the TxID for the funding transaction by using **3a** look for either the latest, or the unconfirmed transaction. It should say `confirmation_time: None`.
4. Confirmed balance of the Arbitrator wallet should increase as expected once the payout transaction has at least 1 confirmation.

[Example TxID - 09451312645a8be5455c6de0c9500ac108d9bd0d81554a93db9ccb523f06d9ea](https://mempool.space/testnet/tx/09451312645a8be5455c6de0c9500ac108d9bd0d81554a93db9ccb523f06d9ea)

## Anatomy of the Locking Multi-sig

The locked multi-sig has a policy as below. See [this article](https://shiftcrypto.ch/blog/understanding-bitcoin-miniscript-part-2/) on what *policies* are and their relationship to mini-scripts.

```
or(10@thresh(2,pk(A),pk(B)),and(pk(C),older(6)))
```
where A & B are keys of the maker & taker, and C is the key of the arbitrator

This merely state the funds in the multi-sig can be redeemed either by
- A & B (maker & taker / buyer & seller) both signed.
- 6 blocks have elapsed and C (arbitrator) has signed.

An equivalent output descriptor, different for each party of the trade (maker vs taker vs arbitrator) is generated and is needed as a backup so a wallet that's capable of signing and redeeming the multi-sig can be restored. The output descriptor differs for each party because each party has one of the derived private key, with the respective public keys for the other 2 key slots.

## Anatomy of the Funding/Locking Transaction

For the funding transaction, the maker first attempts to fund the transaction with the amount and the bond that its required to pay by doing UTXO selection. The way to do so using the BDK is to place the total as 'fee' as a placeholder. This forces the BDK wallet to do coin selection that satisfy the total, along with the correct change output, despite having no other actual recipient specified.

The taker will complete the PSBT by adding its share of the funding as input, create the multi-sig output, and teh correct change output for the taker's funding UTXO selected. This also places the responsibility of determining and paying for fees on the taker as desired.

[Example TxID -c19345415e93af33096c00f06b90f03bb71102fdf98d23746fc3777f360231b6](https://mempool.space/testnet/tx/c19345415e93af33096c00f06b90f03bb71102fdf98d23746fc3777f360231b6)