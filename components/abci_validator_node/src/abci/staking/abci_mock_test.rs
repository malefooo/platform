//!
//! # Integration Testing
//!
//! The content of on-chain governance is not covered.
//!

use crate::abci::server::{tx_sender::forward_txn_with_mode, ABCISubmissionServer};
use abci::*;
use cryptohash::sha256::{self, Digest};
use lazy_static::lazy_static;
use ledger::{
    data_model::{
        Operation, Transaction, TransferType, TxoRef, TxoSID, Utxo, ASSET_TYPE_FRA,
        BLACK_HOLE_PUBKEY, TX_FEE_MIN,
    },
    staking::{
        calculate_delegation_rewards, td_pubkey_to_td_addr, TendermintAddr,
        Validator as StakingValidator, COINBASE_KP, COINBASE_PK, FRA, FRA_TOTAL_AMOUNT,
    },
    store::{fra_gen_initial_tx, LedgerAccess},
};
use parking_lot::{Mutex, RwLock};
use rand_chacha::ChaChaRng;
use rand_core::SeedableRng;
use ruc::*;
use std::{
    collections::BTreeMap,
    env, mem,
    sync::{
        atomic::{AtomicI64, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
    time::Duration,
};
use txn_builder::{BuildsTransactions, TransactionBuilder, TransferOperationBuilder};
use zei::xfr::{
    asset_record::{open_blind_asset_record, AssetRecordType},
    sig::{XfrKeyPair, XfrPublicKey},
    structs::{AssetRecordTemplate, XfrAmount},
};

lazy_static! {
    static ref TD_MOCKER: Arc<RwLock<TendermintMocker>> = Arc::new(RwLock::new(TendermintMocker::new()));
    static ref ABCI_MOCKER: Arc<RwLock<AbciMocker>> = Arc::new(RwLock::new(AbciMocker::new()));
    static ref FAILED_TXS: Arc<RwLock<BTreeMap<Digest, Transaction>>> = Arc::new(RwLock::new(map! {B}));
    static ref SUCCESS_TXS: Arc<RwLock<BTreeMap<Digest, Transaction>>> = Arc::new(RwLock::new(map! {B}));
    /// will be used in [tx_sender](super::server::tx_sender)
    pub static ref CHAN: ChanPair = {
        let (s, r) = channel();
        (Arc::new(Mutex::new(s)), Arc::new(Mutex::new(r)))
    };
}

type ChanPair = (
    Arc<Mutex<Sender<Transaction>>>,
    Arc<Mutex<Receiver<Transaction>>>,
);

static TENDERMINT_BLOCK_HEIGHT: AtomicI64 = AtomicI64::new(0);

const ITV: u64 = 10;
const INIT_POWER: i64 = 1_0000 * FRA as i64;

struct AbciMocker(ABCISubmissionServer);

impl AbciMocker {
    fn new() -> AbciMocker {
        AbciMocker(pnk!(ABCISubmissionServer::new(None, String::new())))
    }

    fn produce_block(&mut self) {
        // do not generate empty blocks,
        // in order to reduce error messages
        let txs = CHAN.1.lock().try_iter().collect::<Vec<_>>();
        alt!(txs.is_empty(), return);

        let h = 1 + TENDERMINT_BLOCK_HEIGHT.fetch_add(1, Ordering::Relaxed);
        let proposer = pnk!(hex::decode(
            TD_MOCKER
                .read()
                .validators
                .keys()
                .next()
                .unwrap()
                .as_bytes()
        ));

        self.0.begin_block(&gen_req_begin_block(h, proposer));

        let mut failed_txs = FAILED_TXS.write();
        let mut successful_txs = SUCCESS_TXS.write();
        for tx in txs.into_iter() {
            let key = gen_tx_hash(&tx);
            if 0 == self.0.deliver_tx(&gen_req_deliver_tx(tx.clone())).code {
                assert!(successful_txs.insert(key, tx).is_none());
            } else {
                assert!(failed_txs.insert(key, tx).is_none());
            }
        }
        drop(failed_txs);
        drop(successful_txs);

        let resp = self.0.end_block(&gen_req_end_block());
        if 0 < resp.validator_updates.len() {
            TD_MOCKER.write().validators = resp
                .validator_updates
                .into_vec()
                .into_iter()
                .filter_map(|v| {
                    v.pub_key
                        .as_ref()
                        .map(|pk| (td_pubkey_to_td_addr(pk.get_data()), v.power))
                })
                .collect();
        }

        self.0.commit(&gen_req_commit());
    }

    fn get_owned_utxos(&self, addr: &XfrPublicKey) -> BTreeMap<TxoSID, Utxo> {
        self.0
            .la
            .read()
            .get_committed_state()
            .read()
            .get_status()
            .get_owned_utxos(addr)
    }

    fn get_owned_balance(&self, addr: &XfrPublicKey) -> u64 {
        self.get_owned_utxos(addr)
            .values()
            .map(|utxo| {
                if let XfrAmount::NonConfidential(am) = utxo.0.record.amount {
                    am
                } else {
                    0
                }
            })
            .sum()
    }
}

pub struct TendermintMocker {
    validators: BTreeMap<String, i64>,
}

impl TendermintMocker {
    fn new() -> TendermintMocker {
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(ITV));
                ABCI_MOCKER.write().produce_block();
            }
        });

        TendermintMocker {
            validators: map! {B hex::encode(&[0; 20]) => 1 },
        }
    }

    fn clean(&mut self) {
        CHAN.1.lock().try_iter().for_each(|_| {});
        self.validators = map! {B hex::encode(&[0; 20]) => 1 };
    }
}

fn gen_req_begin_block(h: i64, proposer: Vec<u8>) -> RequestBeginBlock {
    let mut header = Header::new();
    header.set_height(h);
    header.set_proposer_address(proposer);

    let mut res = RequestBeginBlock::new();
    res.set_header(header);

    res
}

fn gen_req_deliver_tx(tx: Transaction) -> RequestDeliverTx {
    let mut res = RequestDeliverTx::new();
    res.set_tx(pnk!(serde_json::to_vec(&tx)));
    res
}

fn gen_req_end_block() -> RequestEndBlock {
    RequestEndBlock::new()
}

fn gen_req_commit() -> RequestCommit {
    RequestCommit::new()
}

fn gen_tx_hash(tx: &Transaction) -> Digest {
    sha256::hash(&pnk!(bincode::serialize(tx)))
}

fn gen_keypair() -> XfrKeyPair {
    XfrKeyPair::generate(&mut ChaChaRng::from_entropy())
}

fn get_owned_utxos(pk: &XfrPublicKey) -> BTreeMap<TxoSID, Utxo> {
    ABCI_MOCKER.read().get_owned_utxos(pk)
}

fn gen_transfer_op(
    owner_kp: &XfrKeyPair,
    target_pk: &XfrPublicKey,
    am: u64,
    rev: bool,
) -> Result<Operation> {
    let output_template = AssetRecordTemplate::with_no_asset_tracing(
        am,
        ASSET_TYPE_FRA,
        AssetRecordType::NonConfidentialAmount_NonConfidentialAssetType,
        *target_pk,
    );

    let mut trans_builder = TransferOperationBuilder::new();

    let mut am = am;
    let mut i_am;
    let utxos = get_owned_utxos(owner_kp.get_pk_ref()).into_iter();

    macro_rules! add_inputs {
        ($utxos: expr) => {
            for (sid, utxo) in $utxos {
                if let XfrAmount::NonConfidential(n) = utxo.0.record.amount {
                    alt!(n < am, i_am = n, i_am = am);
                    am = am.saturating_sub(n);
                } else {
                    continue;
                }

                open_blind_asset_record(&utxo.0.record, &None, owner_kp)
                    .c(d!())
                    .and_then(|ob| {
                        trans_builder
                            .add_input(TxoRef::Absolute(sid), ob, None, None, i_am)
                            .c(d!())
                    })?;
                alt!(0 == am, break);
            }
        };
    }

    alt!(rev, add_inputs!(utxos.rev()), add_inputs!(utxos));

    if 0 != am {
        return Err(eg!());
    }

    trans_builder
        .add_output(&output_template, None, None, None)
        .c(d!())?
        .balance()
        .c(d!())?
        .create(TransferType::Standard)
        .c(d!())?
        .sign(owner_kp)
        .c(d!())?
        .transaction()
        .c(d!())
}

fn new_tx_builder() -> TransactionBuilder {
    let h = TENDERMINT_BLOCK_HEIGHT.load(Ordering::Relaxed) as u64;
    TransactionBuilder::from_seq_id(h.saturating_sub(20))
}

fn gen_fee_op(owner_kp: &XfrKeyPair) -> Result<Operation> {
    gen_transfer_op(owner_kp, &*BLACK_HOLE_PUBKEY, TX_FEE_MIN, true).c(d!())
}

fn gen_new_validators(n: u8) -> (Vec<StakingValidator>, Vec<XfrKeyPair>) {
    let kps = (0..n).map(|_| gen_keypair()).collect::<Vec<_>>();

    // ensure the sequence is same as them in BTreeMap
    let td_pubkey_bytes = (0..n)
        .map(|i| vec![i; 32])
        .map(|k| (td_pubkey_to_td_addr(&k), k))
        .collect::<BTreeMap<_, _>>();

    let v_set = td_pubkey_bytes
        .into_iter()
        .map(|(_, td_pk)| td_pk)
        .zip(kps.iter())
        .map(|(td_pk, kp)| StakingValidator::new(td_pk, INIT_POWER, kp.get_pk(), None))
        .collect::<Vec<_>>();

    (v_set, kps)
}

fn update_validator(
    owner_kp: &XfrKeyPair,
    cosig_kps: &[&XfrKeyPair],
    h: u64,
    v_set: Vec<StakingValidator>,
) -> Result<Digest> {
    let mut builder = new_tx_builder();

    builder
        .add_operation_update_validator(cosig_kps, h, v_set)
        .c(d!())
        .and_then(|b| {
            gen_fee_op(owner_kp)
                .c(d!())
                .map(move |op| b.add_operation(op))
        })?;

    let tx = builder.take_transaction();
    let h = gen_tx_hash(&tx);
    send_tx(tx).c(d!()).map(|_| h)
}

fn distribute_fra(
    owner_kp: &XfrKeyPair,
    cosig_kps: &[&XfrKeyPair],
    alloc_table: BTreeMap<XfrPublicKey, u64>,
) -> Result<Digest> {
    let mut builder = new_tx_builder();

    builder
        .add_operation_fra_distribution(cosig_kps, alloc_table)
        .c(d!())
        .and_then(|b| {
            gen_fee_op(owner_kp)
                .c(d!())
                .map(move |op| b.add_operation(op))
        })?;

    let tx = builder.take_transaction();
    let h = gen_tx_hash(&tx);
    send_tx(tx).c(d!()).map(|_| h)
}

fn delegate(
    owner_kp: &XfrKeyPair,
    validator: TendermintAddr,
    amount: u64,
    block_span: u64,
) -> Result<Digest> {
    delegate_x(owner_kp, validator, amount, block_span, false).c(d!())
}

fn delegate_x(
    owner_kp: &XfrKeyPair,
    validator: TendermintAddr,
    mut amount: u64,
    block_span: u64,
    is_evil: bool,
) -> Result<Digest> {
    let mut builder = new_tx_builder();
    builder.add_operation_delegation(owner_kp, validator, block_span);

    alt!(is_evil, amount = 1);
    let trans_to_self =
        gen_transfer_op(owner_kp, owner_kp.get_pk_ref(), amount, false).c(d!())?;
    builder.add_operation(trans_to_self);

    if builder.add_fee_relative_auto(&owner_kp).is_err() {
        builder.add_operation(gen_fee_op(owner_kp).c(d!())?);
    }

    let tx = builder.take_transaction();
    let h = gen_tx_hash(&tx);
    send_tx(tx).c(d!()).map(|_| h)
}

fn undelegate(owner_kp: &XfrKeyPair) -> Result<Digest> {
    let mut builder = new_tx_builder();
    builder.add_operation_undelegation(owner_kp);

    gen_fee_op(owner_kp)
        .c(d!())
        .map(|op| builder.add_operation(op))?;

    let tx = builder.take_transaction();
    let h = gen_tx_hash(&tx);
    send_tx(tx).c(d!()).map(|_| h)
}

fn gen_final_tx_auto_fee(
    owner_kp: &XfrKeyPair,
    ops: Vec<Operation>,
) -> Result<Transaction> {
    let mut builder = new_tx_builder();

    ops.into_iter().for_each(|op| {
        builder.add_operation(op);
    });

    if builder.add_fee_relative_auto(&owner_kp).is_err() {
        builder.add_operation(gen_fee_op(owner_kp).c(d!())?);
    }

    Ok(builder.take_transaction())
}

fn send_tx(tx: Transaction) -> Result<()> {
    forward_txn_with_mode("", tx, true).c(d!())
}

fn transfer(owner_kp: &XfrKeyPair, target_pk: &XfrPublicKey, am: u64) -> Result<Digest> {
    gen_transfer_op(owner_kp, target_pk, am, false)
        .c(d!())
        .and_then(|op| gen_final_tx_auto_fee(owner_kp, vec![op]).c(d!()))
        .and_then(|tx| {
            let h = gen_tx_hash(&tx);
            send_tx(tx).c(d!()).map(|_| h)
        })
}

fn wait_one_block() {
    wait_n_block(1);
}

fn wait_n_block(n: u8) {
    (0..n).for_each(|_| {
        sleep_ms!(2 * ITV);
    });
}

fn is_successful(tx_hash: &Digest) -> bool {
    SUCCESS_TXS.read().contains_key(tx_hash) && !FAILED_TXS.read().contains_key(tx_hash)
}

fn is_failed(tx_hash: &Digest) -> bool {
    !SUCCESS_TXS.read().contains_key(tx_hash) && FAILED_TXS.read().contains_key(tx_hash)
}

fn env_refresh(validator_num: u8) {
    // ensure the sequence is same as them in BTreeMap
    let td_pubkey_bytes = (0..validator_num)
        .map(|i| vec![i; 32])
        .map(|k| (td_pubkey_to_td_addr(&k), k))
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .next()
        .unwrap()
        .1;

    env::set_var("TD_NODE_SELF_ADDR", td_pubkey_to_td_addr(&td_pubkey_bytes));

    *ABCI_MOCKER.write() = AbciMocker::new();
    TD_MOCKER.write().clean();
}

// basic scene without punishments
//
// 0. issue FRA
// 1. update validators
// 2. paid 400m FRAs to CoinBase
// 3. transfer some FRAs to a new addr `x`
//
// 4. use `x` to propose a delegation, and ensure it will fail
// because that all validators have not done self-delegation
//
// 5. make validators to finish their self-delegations
//
// 6. use `x` to propose a delegation(block span = 10)
// 7. ensure `x` can not do transfer within block span
// 8. ensure the power of co-responding validator is increased
// 9. wait for the end of bond state
// 10. ensure the power of co-responding validator is decreased
// 11. ensure delegation reward is calculated and paid correctly
// 12. ensure `x` can do transfer after bond-state expired
//
// 13. delegation with an amount bigger than its actual amount will fail
// 14. use `x` to propose a delegation(block span = 10_0000)
// 15. ensure `x` can not do transfer within block span
// 16. ensure the power of co-responding validator is increased
// 17  propose a `UnDelegation` tx to force end the delegation
// 18. ensure the power of co-responding validator is decreased
// 19. ensure delegation reward is calculated and paid correctly
// 20. ensure `x` can do transfer after bond-state expired
//
// 21. transfer FRAs from CoinBase to out-plan addr, and ensure it will fail
//
// 22. use `FraDistribution` to transfer FRAs to multi addrs
// 23. ensure the result of `FraDistribution` is correct
// 24. use these addrs to delegate to different validators(block span = 10)
// 25. ensure the power of each validator is increased correctly
// 26. wait for the end of bond state
// 27. ensure the power of each validator is decreased correctly
//
// 28. re-delegate those multi addrs one by one(block span = 10_0000),
// ensure delegation-rewards-rate is correct in different global delegation levels
//
// 29. ensure the vote power of any vallidator can not exceed 20% of total power
//
// 30. replay old transactions and ensure all of them is failed
fn staking_scene_1() -> Result<()> {
    const VALIDATORS_NUM: u8 = 6;

    env_refresh(VALIDATORS_NUM);

    let keypair = gen_keypair();

    // mid-util:
    // send a tx to trigger next block
    macro_rules! trigger_next_block {
        () => {
            let _ = transfer(&keypair, &COINBASE_PK, 1).c(d!())?;
            wait_one_block();
        };
    }

    // 0. issue FRA

    let tx = fra_gen_initial_tx(&keypair);
    let tx_hash = gen_tx_hash(&tx);
    send_tx(tx).c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    // 1. update validators

    let (v_set, kps) = gen_new_validators(VALIDATORS_NUM);
    assert_eq!(v_set.len(), kps.len());

    // update validators at height 2
    let tx_hash = update_validator(&keypair, &[], 2, v_set.clone()).c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    wait_one_block();
    let td_mocker = TD_MOCKER.read();
    let td_v_set = &td_mocker.validators;
    assert_eq!(v_set.len(), td_v_set.len());
    v_set.iter().for_each(|v| {
        assert_eq!(
            &INIT_POWER,
            pnk!(td_v_set.get(&td_pubkey_to_td_addr(&v.td_pubkey)))
        );
    });

    drop(td_mocker);

    // 2. paid 400m FRAs to CoinBase

    let tx_hash = transfer(&keypair, &COINBASE_PK, 400 * 1_0000 * FRA).c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    // 3. transfer some FRAs to a new addr `x`

    let x_kp = gen_keypair();

    let tx_hash = transfer(&keypair, x_kp.get_pk_ref(), 1_0000 * FRA).c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    // 4. use `x` to propose a delegation, and ensure that
    // it will fail because that all validators have not done self-delegation

    let tx_hash = delegate(
        &x_kp,
        td_pubkey_to_td_addr(&v_set[0].td_pubkey),
        32 * FRA,
        10,
    )
    .c(d!())?;
    wait_one_block();
    assert!(is_failed(&tx_hash));

    let tx_hash = undelegate(&x_kp).c(d!())?;
    wait_one_block();
    assert!(is_failed(&tx_hash));

    // 5. make validators to finish their self-delegations

    for (i, kp) in kps.iter().enumerate() {
        let tx_hash = transfer(&keypair, &v_set[i].id, 100 * FRA).c(d!())?;
        wait_one_block();
        assert!(is_successful(&tx_hash));

        let tx_hash = delegate(
            kp,
            td_pubkey_to_td_addr(&v_set[i].td_pubkey),
            100 * FRA,
            10_0000,
        )
        .c(d!())?;
        wait_one_block();
        assert!(is_successful(&tx_hash));
    }

    // validators are not allowed to do undelegation
    for kp in kps.iter() {
        let tx_hash = undelegate(kp).c(d!())?;
        wait_one_block();
        assert!(is_failed(&tx_hash));
    }

    // 6. use `x` to propose a delegation(block span = 10)

    let tx_hash = delegate(
        &x_kp,
        td_pubkey_to_td_addr(&v_set[0].td_pubkey),
        32 * FRA,
        10,
    )
    .c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    // dup-delegation will fail
    let tx_hash = delegate(
        &x_kp,
        td_pubkey_to_td_addr(&v_set[0].td_pubkey),
        32 * FRA,
        10,
    )
    .c(d!())?;
    wait_one_block();
    assert!(is_failed(&tx_hash));

    // 7. ensure `x` can not do transfer within block span

    let tx_hash = transfer(&x_kp, &COINBASE_PK, 1).c(d!())?;
    wait_one_block();
    assert!(is_failed(&tx_hash));

    // 8. ensure the power of co-responding validator is increased

    let power = ABCI_MOCKER
        .read()
        .0
        .la
        .read()
        .get_committed_state()
        .read()
        .get_staking()
        .validator_get_power(&v_set[0].id)
        .c(d!())?;
    assert_eq!((32 + 100) * FRA as i64 + INIT_POWER, power);

    // 9. wait for the end of bond state

    for _ in 0..20 {
        trigger_next_block!();
        wait_one_block();
    }

    // 10. ensure the power of co-responding validator is decreased

    let power = ABCI_MOCKER
        .read()
        .0
        .la
        .read()
        .get_committed_state()
        .read()
        .get_staking()
        .validator_get_power(&v_set[0].id)
        .c(d!())?;
    assert_eq!(100 * FRA as i64 + INIT_POWER, power);

    // 11. ensure delegation reward is calculated and paid correctly

    for _ in 0..2 {
        trigger_next_block!();
        wait_one_block();
    }

    let return_rate = ABCI_MOCKER
        .read()
        .0
        .la
        .read()
        .get_committed_state()
        .read()
        .get_staking()
        .get_block_rewards_rate();

    let rewards =
        calculate_delegation_rewards(32 * FRA as i64, return_rate).c(d!())? * 10;
    assert!(
        10000 * FRA - TX_FEE_MIN
            < ABCI_MOCKER.read().get_owned_balance(x_kp.get_pk_ref())
    );
    assert!(
        10000 * FRA + rewards - TX_FEE_MIN
            >= ABCI_MOCKER.read().get_owned_balance(x_kp.get_pk_ref())
    );

    // 12. ensure `x` can do transfer after bond-state expired

    let tx_hash = transfer(&x_kp, &COINBASE_PK, 1).c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    // 13. delegation with an amount bigger than its actual amount will fail

    let tx_hash = delegate_x(
        &x_kp,
        td_pubkey_to_td_addr(&v_set[0].td_pubkey),
        32_0000_0000 * FRA,
        10,
        true,
    )
    .c(d!())?;
    wait_one_block();
    assert!(is_failed(&tx_hash));

    // 14. use `x` to propose a delegation(block span = 10_0000)

    let tx_hash = delegate(
        &x_kp,
        td_pubkey_to_td_addr(&v_set[0].td_pubkey),
        32 * FRA,
        10_0000,
    )
    .c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    // 15. ensure `x` can not do transfer within block span

    let tx_hash = transfer(&x_kp, &COINBASE_PK, 1).c(d!())?;
    wait_one_block();
    assert!(is_failed(&tx_hash));

    // 16. ensure the power of co-responding validator is increased

    let power = ABCI_MOCKER
        .read()
        .0
        .la
        .read()
        .get_committed_state()
        .read()
        .get_staking()
        .validator_get_power(&v_set[0].id)
        .c(d!())?;
    assert_eq!((32 + 100) * FRA as i64 + INIT_POWER, power);

    // 17  propose a `UnDelegation` tx to force end the delegation

    for _ in 0..8 {
        trigger_next_block!();
        wait_one_block();
    }

    let tx_hash = undelegate(&x_kp).c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    // 18. ensure the power of co-responding validator is decreased

    for _ in 0..2 {
        trigger_next_block!();
        wait_one_block();
    }

    let power = ABCI_MOCKER
        .read()
        .0
        .la
        .read()
        .get_committed_state()
        .read()
        .get_staking()
        .validator_get_power(&v_set[0].id)
        .c(d!())?;
    assert_eq!(100 * FRA as i64 + INIT_POWER, power);

    // 19. ensure delegation reward is calculated and paid correctly

    for _ in 0..20 {
        trigger_next_block!();
        wait_one_block();
    }

    let return_rate = ABCI_MOCKER
        .read()
        .0
        .la
        .read()
        .get_committed_state()
        .read()
        .get_staking()
        .get_block_rewards_rate();

    let rewards =
        calculate_delegation_rewards(32 * FRA as i64, return_rate).c(d!())? * 10;
    assert!(
        10000 * FRA - 4 * TX_FEE_MIN
            < ABCI_MOCKER.read().get_owned_balance(x_kp.get_pk_ref())
    );
    assert!(
        10000 * FRA + rewards - 4 * TX_FEE_MIN
            >= ABCI_MOCKER.read().get_owned_balance(x_kp.get_pk_ref())
    );

    // 20. ensure `x` can do transfer after bond-state expired

    let tx_hash = transfer(&x_kp, &COINBASE_PK, 1).c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    // 21. transfer FRAs from CoinBase to out-plan addr, and ensure it will fail

    let tx_hash = transfer(&COINBASE_KP, keypair.get_pk_ref(), 1).c(d!())?;
    wait_one_block();
    assert!(is_failed(&tx_hash));

    // 22. use `FraDistribution` to transfer FRAs to multi addrs

    // rewards rate:
    //   - ([0, 10], 20)
    //   - ([10, 20], 17)
    //   - ([20, 30], 14)
    //   - ([30, 40], 11)
    //   - ([40, 50], 8)
    //   - ([50, 50], 5)
    //   - ([60, 67], 2)
    //   - ([67, 101], 1)
    let (a_kp, a_am) = (gen_keypair(), 1 + FRA_TOTAL_AMOUNT * 5 / 100); // 5%, total 5%
    let (b_kp, b_am) = (gen_keypair(), 2 + FRA_TOTAL_AMOUNT * 10 / 100); // 10%, total 15%
    let (c_kp, c_am) = (gen_keypair(), 3 + FRA_TOTAL_AMOUNT * 10 / 100); // 10%, total 25%
    let (d_kp, d_am) = (gen_keypair(), 4 + FRA_TOTAL_AMOUNT * 10 / 100); // 10%, total 35%
    let (e_kp, e_am) = (gen_keypair(), 5 + FRA_TOTAL_AMOUNT * 10 / 100); // 10%, total 45%
    let (f_kp, f_am) = (gen_keypair(), 6 + FRA_TOTAL_AMOUNT * 10 / 100); // 10%, total 55%
    let (g_kp, g_am) = (gen_keypair(), 7 + FRA_TOTAL_AMOUNT * 10 / 100); // 10%, total 65%
    let (h_kp, h_am) = (gen_keypair(), 8 + FRA_TOTAL_AMOUNT * 3 / 100); // 3%, total 68%
    let (i_kp, i_am) = (gen_keypair(), 9 + FRA_TOTAL_AMOUNT * 12 / 100); // 12%, total 80%

    // Transfer 80% of total FRAs to CoinBase.
    let tx_hash = transfer(&keypair, &COINBASE_PK, FRA_TOTAL_AMOUNT * 9 / 10).c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    let alloc_table_x = [
        (&a_kp, a_am),
        (&b_kp, b_am),
        (&c_kp, c_am),
        (&d_kp, d_am),
        (&e_kp, e_am),
        (&f_kp, f_am),
        (&g_kp, g_am),
        (&h_kp, h_am),
        (&i_kp, i_am),
    ]
    .iter()
    .map(|(kp, am)| (*kp, *am))
    .collect::<Vec<(&XfrKeyPair, u64)>>();

    let alloc_table = alloc_table_x
        .iter()
        .map(|(kp, am)| (kp.get_pk(), *am))
        .collect::<BTreeMap<_, _>>();

    let cosig_kps = kps.iter().collect::<Vec<_>>();

    let coinbase_balance = ABCI_MOCKER.read().get_owned_balance(&COINBASE_PK);

    let tx_hash = distribute_fra(&keypair, &cosig_kps, alloc_table.clone()).c(d!())?;
    wait_one_block();
    assert!(is_successful(&tx_hash));

    for _ in 0..2 {
        trigger_next_block!();
        wait_one_block();
    }

    // 23. ensure the result of `FraDistribution` is correct

    let abci_mocker = ABCI_MOCKER.read();
    assert!(
        alloc_table
            .iter()
            .all(|(pk, am)| *am == abci_mocker.get_owned_balance(pk))
    );
    assert!(
        alloc_table.values().sum::<u64>()
            <= 2 + coinbase_balance - abci_mocker.get_owned_balance(&COINBASE_PK)
    );

    drop(abci_mocker);

    // 24. use these addrs to delegate to different validators(block span = 10)

    for (v, (kp, _)) in v_set.iter().zip(alloc_table_x.iter()) {
        let tx_hash =
            delegate(kp, td_pubkey_to_td_addr(&v.td_pubkey), 32 * FRA, 10).c(d!())?;
        wait_one_block();
        assert!(is_successful(&tx_hash));
    }

    // 25. ensure the power of each validator is increased correctly

    let n = alt!(
        v_set.len() > alloc_table.len(),
        alloc_table.len(),
        v_set.len()
    );

    for v in v_set.iter().take(n) {
        let power = ABCI_MOCKER
            .read()
            .0
            .la
            .read()
            .get_committed_state()
            .read()
            .get_staking()
            .validator_get_power(&v.id)
            .c(d!())?;
        assert_eq!((32 + 100) * FRA as i64 + INIT_POWER, power);
    }

    // 26. wait for the end of bond state

    for _ in 0..20 {
        trigger_next_block!();
        wait_one_block();
    }

    // 27. ensure the power of each validator is decreased correctly

    for v in v_set.iter().take(n) {
        let power = ABCI_MOCKER
            .read()
            .0
            .la
            .read()
            .get_committed_state()
            .read()
            .get_staking()
            .validator_get_power(&v.id)
            .c(d!())?;
        assert_eq!(100 * FRA as i64 + INIT_POWER, power);
    }

    // 28. re-delegate those multi addrs one by one(block span = 10_0000),
    // ensure delegation-rewards-rate is correct in different global delegation levels
    // ...........................................
    // .... will be tested in unit-test cases ....
    // ...........................................

    // 29. ensure the vote power of any vallidator can not exceed 20% of total power

    let tx_hash = delegate(
        &keypair,
        td_pubkey_to_td_addr(&v_set[0].td_pubkey),
        32_0000 * FRA,
        10,
    )
    .c(d!())?;
    wait_one_block();
    assert!(is_failed(&tx_hash));

    // 30. replay old transactions and ensure all of them is failed
    let old_txs = mem::take(&mut *SUCCESS_TXS.write())
        .into_iter()
        .chain(mem::take(&mut *FAILED_TXS.write()).into_iter())
        .map(|(tx_hash, tx)| send_tx(tx).c(d!()).map(|_| tx_hash))
        .collect::<Result<Vec<_>>>();

    wait_n_block(5);

    for tx_hash in old_txs.c(d!())?.iter() {
        assert!(is_failed(tx_hash));
    }

    Ok(())
}

// test on-chain governance
fn staking_scene_2() -> Result<()> {
    // TODO
    Ok(())
}

#[test]
fn staking_integration() {
    pnk!(staking_scene_1());
    pnk!(staking_scene_2());
}
