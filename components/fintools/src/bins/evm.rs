use baseapp::Action;
use clap::{crate_authors, crate_version, App, SubCommand};
use fintools::fns::get_keypair;
use fintools::fns::utils;
use fp_core::account::{MintOutput, TransferToUTXO};
use fp_core::crypto::{Address32, MultiSignature};
// use fp_core::ecdsa::Pair;
use fp_core::transaction::UncheckedTransaction;
use ledger::address::SmartAddress;
use ledger::data_model::ASSET_TYPE_FRA;
use ledger::data_model::BLACK_HOLE_PUBKEY_STAKING;
use module_account::Action as AccountAction;
use ruc::*;
use std::str::FromStr;
use tendermint_rpc::Client;
use tokio::runtime::Runtime;
use txn_builder::BuildsTransactions;

fn transfer_amount(amount: u64, address: String) -> Result<()> {
    let mut builder = utils::new_tx_builder()?;

    let kp = get_keypair()?;
    let transfer_op = utils::gen_transfer_op(
        &kp,
        vec![(&BLACK_HOLE_PUBKEY_STAKING, amount)],
        false,
        false,
    )?;
    builder
        .add_operation(transfer_op)
        .add_operation_convert_account(
            &kp,
            SmartAddress::from_string(address).c(d!())?,
        )?;
    utils::send_tx(&builder.take_transaction())?;
    Ok(())
}

fn refsnart_amount(amount: u64, address: String) -> Result<()> {
    let addr = wallet::public_key_from_base64(&address)?;
    let output = MintOutput {
        target: addr,
        amount,
        asset: ASSET_TYPE_FRA,
    };
    // ed25519
    let kp = get_keypair()?;
    let signer = Address32::from(kp.get_pk());

    // ecdsa
    // let (kp, _, _) = Pair::generate_with_phrase(None);
    // let signer = Address32::from(kp.address());

    let tm_client = tendermint_rpc::HttpClient::new("http://127.0.0.1:26657").unwrap();
    let query_ret = Runtime::new()
        .unwrap()
        .block_on(tm_client.abci_query(
            Some(tendermint::abci::Path::from_str("module/account/nonce").unwrap()),
            serde_json::to_vec(&signer).unwrap(),
            None,
            false,
        ))
        .unwrap();
    let nonce = serde_json::from_slice::<u64>(query_ret.value.as_slice()).unwrap();

    let account_call = AccountAction::TransferToUTXO(TransferToUTXO {
        nonce,
        outputs: vec![output],
    });
    let account_of = Action::Account(account_call);

    let msg = serde_json::to_vec(&account_of).unwrap();

    let sig = kp.get_sk_ref().sign(msg.as_slice(), kp.get_pk_ref());
    let signature = MultiSignature::from(sig);

    let tx = UncheckedTransaction::new_signed(account_of, signer, signature);
    let txn = serde_json::to_vec(&tx).unwrap();

    let resp = Runtime::new()
        .unwrap()
        .block_on(tm_client.broadcast_tx_commit(txn.into()))
        .c(d!())?;

    println!("tx_bytes: {:?}", resp);
    Ok(())
}

fn run() -> Result<()> {
    let transfer = SubCommand::with_name("transfer")
        .arg_from_usage(
            "-b --balance=<Balance> transfer balance from utxo fra to account fra",
        )
        .arg_from_usage("-a --address=<Address> transfer target address");

    let refsnart = SubCommand::with_name("refsnart")
        .arg_from_usage(
            "-b --balance=<Balance> transfer balance from account fra to utxo fra",
        )
        .arg_from_usage("-a --address=<Address> transfer target address");

    let matchs = App::new("fe")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Findora evm compact operator tool")
        .subcommand(transfer)
        .subcommand(refsnart)
        .get_matches();

    if let Some(m) = matchs.subcommand_matches("transfer") {
        let amount = m.value_of("balance").c(d!())?;
        let address = m.value_of("address").c(d!())?;
        transfer_amount(
            u64::from_str_radix(amount, 10).c(d!())?,
            String::from(address),
        )?
    }

    if let Some(m) = matchs.subcommand_matches("refsnart") {
        let amount = m.value_of("balance").c(d!())?;
        let address = m.value_of("address").c(d!())?;
        refsnart_amount(
            u64::from_str_radix(amount, 10).c(d!())?,
            String::from(address),
        )?
    }
    Ok(())
}

fn main() {
    run().unwrap()
}
