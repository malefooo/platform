use super::get_keypair;
use super::get_serv_addr;
use super::utils;
use baseapp::{Action, CheckFee, CheckNonce};
use fp_core::account::{MintOutput, SmartAccount, TransferToUTXO};
use fp_core::crypto::{Address, Address32, MultiSignature};
use fp_core::ecdsa::Pair;
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
use zei::xfr::sig::XfrKeyPair;

/// transfer utxo assets to account(ed25519 or ecdsa address) balance.
pub fn transfer_to_account(amount: u64, address: Option<&str>) -> Result<()> {
    let mut builder = utils::new_tx_builder()?;

    let kp = get_keypair()?;
    let transfer_op = utils::gen_transfer_op(
        &kp,
        vec![(&BLACK_HOLE_PUBKEY_STAKING, amount)],
        false,
        false,
    )?;
    let target_address = match address {
        Some(s) => SmartAddress::from_string(s).c(d!())?,
        None => SmartAddress::Xfr(kp.get_pk()),
    };
    builder
        .add_operation(transfer_op)
        .add_operation_convert_account(&kp, target_address)?;
    utils::send_tx(&builder.take_transaction())?;
    Ok(())
}

pub enum Keypair {
    ED25519(XfrKeyPair),
    ECDSA(Pair),
}

impl Keypair {
    pub fn sign(&self, data: &[u8]) -> MultiSignature {
        match self {
            Keypair::ECDSA(kp) => MultiSignature::from(kp.sign(data)),
            Keypair::ED25519(kp) => {
                MultiSignature::from(kp.get_sk_ref().sign(data, kp.get_pk_ref()))
            }
        }
    }
}

/// transfer to uxto assets from account(ed25519 or ecdsa address) balance.
pub fn transfer_from_account(
    amount: u64,
    address: Option<&str>,
    eth_phrase: Option<&str>,
) -> Result<()> {
    let fra_kp = get_keypair()?;

    let target = match address {
        Some(s) => {
            if let Ok(address) = wallet::public_key_from_base64(&s) {
                address
            } else {
                wallet::public_key_from_bech32(&s)?
            }
        }
        None => fra_kp.get_pk(),
    };

    let output = MintOutput {
        target,
        amount,
        asset: ASSET_TYPE_FRA,
    };

    let (signer, kp) = if let Some(key_path) = eth_phrase {
        let kp = Pair::from_phrase(key_path, None)?.0;
        let signer = Address32::from(kp.address());
        (signer, Keypair::ECDSA(kp))
    } else {
        let signer = Address32::from(fra_kp.get_pk());
        (signer, Keypair::ED25519(fra_kp))
    };

    let tm_client = tendermint_rpc::HttpClient::new(
        format!("{}:26657", get_serv_addr().c(d!())?).as_str(),
    )
    .unwrap();
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
        outputs: vec![output],
    });
    let action = Action::Account(account_call);
    let extra = (CheckNonce::new(nonce), CheckFee::new(None));
    let msg = serde_json::to_vec(&(action.clone(), extra.clone())).unwrap();

    let signature = kp.sign(msg.as_slice());

    let tx = UncheckedTransaction::new_signed(action, signer, signature, extra);

    let txn = serde_json::to_vec(&tx).unwrap();

    Runtime::new()
        .unwrap()
        .block_on(tm_client.broadcast_tx_sync(txn.into()))
        .c(d!())?;

    Ok(())
}

/// Query contract account info by abci/query
pub fn contract_account_info(address: Option<&str>) -> Result<(Address, SmartAccount)> {
    let fra_kp = get_keypair()?;

    let address = match address {
        Some(s) => SmartAddress::from_string(s).c(d!())?,
        None => SmartAddress::Xfr(fra_kp.get_pk()),
    };
    let account: Address = address.into();

    let tm_client = tendermint_rpc::HttpClient::new(
        format!("{}:26657", get_serv_addr().c(d!())?).as_str(),
    )
    .unwrap();
    let query_ret = Runtime::new()
        .unwrap()
        .block_on(tm_client.abci_query(
            Some(tendermint::abci::Path::from_str("module/account/info").unwrap()),
            serde_json::to_vec(&account).unwrap(),
            None,
            false,
        ))
        .unwrap();
    Ok((
        account,
        serde_json::from_slice(query_ret.value.as_slice()).c(d!())?,
    ))
}
