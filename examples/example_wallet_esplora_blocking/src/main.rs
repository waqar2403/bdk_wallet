use std::{collections::BTreeSet, io::Write};

use bdk_esplora::{esplora_client, EsploraExt};
use bdk_wallet::{
    bitcoin::{Amount, Network},
    file_store::Store,
    KeychainKind, SignOptions, Wallet,
};

const DB_MAGIC: &str = "bdk_wallet_esplora_example";
const DB_PATH: &str = "bdk-example-esplora-blocking.db";
const SEND_AMOUNT: Amount = Amount::from_sat(5000);
const STOP_GAP: usize = 5;
const PARALLEL_REQUESTS: usize = 5;

// changed wallet descriptors that are from my regtest wallet and URL as well
const NETWORK: Network = Network::Regtest;
const EXTERNAL_DESC: &str = "wpkh(tprv8ZgxMBicQKsPdcJb2xWj2dTT2FE7oKsWpr15h5X3mzgpAJYktDwcsWJj8f7n3HgpNHXnur9yNyxXQmRoeNdsbA8YBaJMDZFU18vJLbFWqHX/84'/1'/0'/1/*)#fzrwq0g9";
const INTERNAL_DESC: &str = "wpkh(tprv8ZgxMBicQKsPcuTSdeggHbMpov7sFS5cD5yHzNZ2EUmQCRSSDzaMoczdMcQ7rtvDUivJLpbRgVMgKZzKU8Sqh8cnUyeTWbRSoXK7gLVytZT/84'/1'/0'/0/*)#03r8jjqd";
const ESPLORA_URL: &str = "http://127.0.0.1:3002";

fn main() -> Result<(), anyhow::Error> {
    let mut db = Store::<bdk_wallet::ChangeSet>::open_or_create_new(DB_MAGIC.as_bytes(), DB_PATH)?;

    let wallet_opt = Wallet::load()
        .descriptor(KeychainKind::External, Some(EXTERNAL_DESC))
        .descriptor(KeychainKind::Internal, Some(INTERNAL_DESC))
        .extract_keys()
        .check_network(NETWORK)
        .load_wallet(&mut db)?;
    let mut wallet = match wallet_opt {
        Some(wallet) => wallet,
        None => Wallet::create(EXTERNAL_DESC, INTERNAL_DESC)
            .network(NETWORK)
            .create_wallet(&mut db)?,
    };

    let address = wallet.next_unused_address(KeychainKind::External);
    wallet.persist(&mut db)?;
    println!(
        "Next unused address: ({}) {}",
        address.index, address.address
    );

    let balance = wallet.balance();
    println!("Wallet balance before syncing: {}", balance.total());

    print!("Syncing...");
    let client = esplora_client::Builder::new(ESPLORA_URL).build_blocking();

    let request = wallet.start_full_scan().inspect({
        let mut stdout = std::io::stdout();
        let mut once = BTreeSet::<KeychainKind>::new();
        move |keychain, spk_i, _| {
            if once.insert(keychain) {
                print!("\nScanning keychain [{:?}] ", keychain);
            }
            print!(" {:<3}", spk_i);
            stdout.flush().expect("must flush")
        }
    });

    let update = client.full_scan(request, STOP_GAP, PARALLEL_REQUESTS)?;

    wallet.apply_update(update)?;
    wallet.persist(&mut db)?;
    println!();

    let balance = wallet.balance();
    println!("Wallet balance after syncing: {}", balance.total());

    if balance.total() < SEND_AMOUNT {
        println!(
            "Please send at least {} to the receiving address",
            SEND_AMOUNT
        );
        std::process::exit(0);
    }

    let mut tx_builder = wallet.build_tx();
    tx_builder.add_recipient(address.script_pubkey(), SEND_AMOUNT);

    let mut psbt = tx_builder.finish()?;
    let finalized = wallet.sign(&mut psbt, SignOptions::default())?;
    assert!(finalized);

    let tx = psbt.extract_tx()?;
    client.broadcast(&tx)?;
    println!("Tx broadcasted! Txid: {}", tx.compute_txid());

    Ok(())
}
