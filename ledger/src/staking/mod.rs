//!
//! # Staking
//!
//! - manage validator information
//! - manage delegation information
//! - manage the distribution of investment income
//! - manage on-chain governance
//! - manage the official re-distribution of FRA
//!

#![deny(warnings)]
#![deny(missing_docs)]

pub mod cosig;
mod init;
pub mod ops;

use crate::{
    data_model::{
        Operation, Transaction, TransferAsset, TxoSID, ASSET_TYPE_FRA, FRA_DECIMALS,
    },
    store::LedgerStatus,
};
use cosig::CoSigRule;
use cryptohash::sha256::{self, Digest};
use lazy_static::lazy_static;
use ops::fra_distribution::FraDistributionOps;
use ruc::*;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    convert::TryFrom,
    mem,
};
use zei::xfr::{
    sig::{XfrKeyPair, XfrPublicKey},
    structs::{XfrAmount, XfrAssetType},
};

/// Staking entry
///
/// Init:
/// 1. set_custom_block_height
/// 2. validator_set_at_height
///
/// Usage:
/// - validator_change_power ...
/// - validator_apply_at_height
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Staking {
    // the main logic when updating:
    // - the new validator inherits the original vote power, if any
    // - all delegate addresss locked on those outdated validators will be unlocked
    // immediately, and the related delegate income will also be settled immediately
    vi: ValidatorInfo,
    // all assets owned by these addrs are NOT permitted to be transfered out,
    // but receiving assets from outer addrs is permitted.
    //
    // when the end-time of delegations arrived,
    // we will try to paid the rewards until all is successful.
    di: DelegationInfo,
    // current block height in the context of tendermint.
    cur_height: BlockHeight,
    // FRA CoinBase.
    coinbase: CoinBase,
}

impl Staking {
    #[inline(always)]
    #[allow(missing_docs)]
    pub fn new() -> Self {
        let vd = init::get_inital_validators().unwrap_or_default();
        let cur_height = vd.height;
        Staking {
            vi: map! {B cur_height => vd },
            di: DelegationInfo::new(),
            cur_height,
            coinbase: CoinBase::gen(),
        }
    }

    /// Get the validators that exactly be setted at a specified height.
    #[inline(always)]
    pub fn validator_get_at_height(&self, h: BlockHeight) -> Option<Vec<&Validator>> {
        self.vi.get(&h).map(|v| v.body.values().collect())
    }

    // Check if there is some settings on a specified height.
    #[inline(always)]
    fn validator_has_settings_at_height(&self, h: BlockHeight) -> bool {
        self.vi.contains_key(&h)
    }

    /// Set the validators that will be used for the specified height.
    #[inline(always)]
    pub fn validator_set_at_height(
        &mut self,
        h: BlockHeight,
        v: ValidatorData,
    ) -> Result<()> {
        if self.validator_has_settings_at_height(h) {
            Err(eg!("already exists"))
        } else {
            self.validator_set_at_height_force(h, v);
            Ok(())
        }
    }

    /// Set the validators that will be used for the specified height,
    /// no matter if there is an existing set of validators at that height.
    #[inline(always)]
    pub fn validator_set_at_height_force(&mut self, h: BlockHeight, v: ValidatorData) {
        self.vi.insert(h, v);
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn validator_get_current(&self) -> Option<&ValidatorData> {
        self.validator_get_effective_at_height(self.cur_height)
    }

    /// Get the validators that will be used for the specified height.
    #[inline(always)]
    pub fn validator_get_effective_at_height(
        &self,
        h: BlockHeight,
    ) -> Option<&ValidatorData> {
        self.vi.range(0..=h).last().map(|(_, v)| v)
    }

    /// Remove the validators that will be used for the specified height.
    #[inline(always)]
    pub fn validator_remove_at_height(
        &mut self,
        h: BlockHeight,
    ) -> Result<Vec<Validator>> {
        self.vi
            .remove(&h)
            .map(|v| v.body.into_iter().map(|(_, v)| v).collect())
            .ok_or(eg!("not exists"))
    }

    /// Get the validators that will be used for a specified height.
    #[inline(always)]
    pub fn validator_get_effective_at_height_mut(
        &mut self,
        h: BlockHeight,
    ) -> Option<&mut ValidatorData> {
        self.vi.range_mut(0..=h).last().map(|(_, v)| v)
    }

    /// Get the validators exactly on a specified height.
    #[inline(always)]
    pub fn validator_get_at_height_mut(
        &mut self,
        h: BlockHeight,
    ) -> Option<&mut ValidatorData> {
        self.vi.get_mut(&h)
    }

    /// Make the validators at current height to be effective.
    #[inline(always)]
    pub fn validator_apply_current(&mut self) {
        let h = self.cur_height;
        self.validator_apply_at_height(h);

        // clean old data before current height
        self.validator_clean_before_height(h);
    }

    /// Make the validators at a specified height to be effective.
    pub fn validator_apply_at_height(&mut self, h: BlockHeight) {
        let prev = self.validator_get_effective_at_height(h - 1).cloned();

        if let Some(prev) = prev {
            if let Some(vs) = self.validator_get_at_height_mut(h) {
                // inherit the powers of previous settings
                // if new settings were found
                vs.body.iter_mut().for_each(|(k, v)| {
                    if let Some(pv) = prev.body.get(k) {
                        v.td_power = pv.td_power;
                    }
                });
            } else {
                // copy previous settings
                // if new settings were not found.
                self.validator_set_at_height_force(h, prev);
            }
        }
    }

    // Clean validator-info older than the specified height.
    #[inline(always)]
    fn validator_clean_before_height(&mut self, h: BlockHeight) {
        self.vi = self.vi.split_off(&h);
    }

    /// increase/decrease the power of a specified validator.
    fn validator_change_power(
        &mut self,
        validator: &XfrPublicKey,
        power: Amount,
    ) -> Result<()> {
        self.validator_check_power(power, validator)
            .c(d!())
            .and_then(|_| {
                self.validator_get_effective_at_height_mut(self.cur_height)
                    .ok_or(eg!())
            })
            .and_then(|cur| {
                cur.body
                    .get_mut(validator)
                    .map(|v| {
                        v.td_power += power;
                    })
                    .ok_or(eg!())
            })
    }

    /// Get the power of a specified validator at current term.
    #[inline(always)]
    pub fn validator_get_power(&self, vldtor: &XfrPublicKey) -> Result<i64> {
        self.validator_get_current()
            .and_then(|vd| vd.body.get(vldtor))
            .map(|v| v.td_power)
            .ok_or(eg!())
    }

    #[inline(always)]
    fn validator_check_power(
        &self,
        new_power: Amount,
        vldtor: &XfrPublicKey,
    ) -> Result<()> {
        let total_power = self.validator_total_power() + new_power;
        if MAX_TOTAL_POWER < total_power {
            return Err(eg!("total power overflow"));
        }

        let power = self.validator_get_power(vldtor).c(d!())?;

        if ((power + new_power) as u128)
            .checked_mul(MAX_POWER_PERCENT_PER_VALIDATOR[1])
            .ok_or(eg!())?
            > MAX_POWER_PERCENT_PER_VALIDATOR[0]
                .checked_mul(total_power as u128)
                .ok_or(eg!())?
        {
            return Err(eg!("validator power overflow"));
        }

        Ok(())
    }

    // calculate current total vote-power
    #[inline(always)]
    fn validator_total_power(&self) -> i64 {
        self.validator_get_effective_at_height(self.cur_height)
            .map(|vs| vs.body.values().map(|v| v.td_power).sum())
            .unwrap_or(0)
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn set_custom_block_height(&mut self, h: BlockHeight) {
        self.cur_height = h;
    }

    /// Start a new delegation.
    /// - increase the vote power of the co-responding validator
    ///
    /// Validator must do self-delegatation first,
    /// and its delegation end_height must be `i64::MAX`.
    ///
    /// **NOTE:** It is the caller's duty to ensure that
    /// there is enough FRAs existing in the target address(owner).
    pub fn delegate(
        &mut self,
        owner: XfrPublicKey,
        validator: TendermintAddrRef,
        am: Amount,
        start_height: BlockHeight,
        mut end_height: BlockHeight,
    ) -> Result<()> {
        let validator = self.td_addr_to_app_pk(validator).c(d!())?;

        if end_height > BLOCK_HEIGHT_MAX || start_height > end_height {
            return Err(eg!("invalid delegation heights"));
        }

        if !(MIN_DELEGATION_AMOUNT..=MAX_DELEGATION_AMOUNT).contains(&(am as u64)) {
            return Err(eg!("invalid delegation amount"));
        }

        if let Some(d) = self.delegation_get(&validator) {
            // check delegation deadline
            if BLOCK_HEIGHT_MAX != d.end_height {
                // should NOT happen
                return Err(eg!("invalid self-delegation of validator"));
            }
        } else if owner == validator {
            // do self-delegation
            end_height = BLOCK_HEIGHT_MAX;
        } else {
            return Err(eg!("self-delegation has not been finished"));
        }

        let k = owner;
        if self.di.addr_map.get(&k).is_some() {
            return Err(eg!("already exists"));
        }

        self.validator_change_power(&validator, am as Amount)
            .c(d!())?;

        let v = Delegation {
            amount: am,
            validator,
            rwd_pk: owner,
            start_height,
            end_height,
            state: DelegationState::Locked,
            rwd_amount: 0,
        };

        self.di.addr_map.insert(k, v);
        self.di
            .end_height_map
            .entry(end_height)
            .or_insert_with(BTreeSet::new)
            .insert(k);

        // total amount of all delegations
        self.di.total_amount += am;

        Ok(())
    }

    /// When delegation period expired,
    /// - compute rewards
    /// - decrease the vote power of the co-responding validator
    ///
    /// **NOTE:** validator self-undelegation is not permitted
    pub fn undelegate(&mut self, addr: &XfrPublicKey) -> Result<()> {
        let h = self.cur_height;
        let mut orig_h = None;

        if let Some(vs) = self.validator_get_effective_at_height(h) {
            if vs.body.get(addr).is_some() {
                return Err(eg!("validator self-undelegation is not permitted"));
            }
        }

        self.di
            .addr_map
            .get_mut(addr)
            .ok_or(eg!("not exists"))
            .and_then(|d| alt!(0 > d.rwd_amount, Err(eg!()), Ok(d)))
            .map(|d| {
                if d.end_height != h {
                    orig_h = Some(d.end_height);
                    d.end_height = h;
                }
            })?;

        // scene: forced un-delegation
        if let Some(orig_h) = orig_h {
            self.di
                .end_height_map
                .get_mut(&orig_h)
                .map(|set| set.remove(addr));
            self.di
                .end_height_map
                .entry(h)
                .or_insert_with(BTreeSet::new)
                .insert(addr.to_owned());
        }

        Ok(())
    }

    #[inline(always)]
    fn delegation_unbond(&mut self, addr: &XfrPublicKey) -> Result<Delegation> {
        let d = self.di.addr_map.remove(addr).ok_or(eg!("not exists"))?;
        if d.state == DelegationState::Paid {
            Ok(d)
        } else {
            // we assume that this probability is very low
            self.di.addr_map.insert(addr.to_owned(), d);
            Err(eg!("unpaid delegation"))
        }
    }

    /// Expand delegation scale
    pub fn delegation_extend(
        &mut self,
        owner: &XfrPublicKey,
        end_height: BlockHeight,
    ) -> Result<()> {
        let addr = owner;
        let d = if let Some(d) = self.di.addr_map.get_mut(addr) {
            d
        } else {
            return Err(eg!("not exists"));
        };

        if end_height > d.end_height {
            let orig_h = d.end_height;
            d.end_height = end_height;
            self.di
                .end_height_map
                .get_mut(&orig_h)
                .ok_or(eg!())?
                .remove(addr);
            self.di
                .end_height_map
                .entry(end_height)
                .or_insert_with(BTreeSet::new)
                .insert(addr.to_owned());
            Ok(())
        } else {
            Err(eg!("new end_height must be bigger than the old one"))
        }
    }

    /// Get the delegation instance of `addr`.
    #[inline(always)]
    pub fn delegation_get(&self, addr: &XfrPublicKey) -> Option<&Delegation> {
        self.di.addr_map.get(&addr)
    }

    /// Get the delegation instance of `addr`.
    #[inline(always)]
    pub fn delegation_get_mut(
        &mut self,
        addr: &XfrPublicKey,
    ) -> Option<&mut Delegation> {
        self.di.addr_map.get_mut(&addr)
    }

    /// Check if the `addr` is in a state of delegation
    #[inline(always)]
    pub fn delegation_has_addr(&self, addr: &XfrPublicKey) -> bool {
        self.di.addr_map.contains_key(&addr)
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn delegation_get_rewards(&self) -> HashMap<XfrPublicKey, u64> {
        self.delegation_get_rewards_before_height(self.cur_height)
    }

    /// Query delegation rewards before a specified height(included).
    #[inline(always)]
    pub fn delegation_get_rewards_before_height(
        &self,
        h: BlockHeight,
    ) -> HashMap<XfrPublicKey, u64> {
        self.delegation_get_bonds_before_height(h)
            .into_iter()
            .filter(|d| d.rwd_amount > 0)
            .map(|d| (d.rwd_pk, d.rwd_amount as u64))
            .collect()
    }

    /// Query all bond delegations.
    #[inline(always)]
    pub fn delegation_get_bonds(&self) -> Vec<&Delegation> {
        self.delegation_get_bonds_before_height(self.cur_height)
    }

    /// Query bond delegations before a specified height(included).
    #[inline(always)]
    pub fn delegation_get_bonds_before_height(
        &self,
        h: BlockHeight,
    ) -> Vec<&Delegation> {
        self.di
            .end_height_map
            .range(..=h)
            .flat_map(|(_, addrs)| {
                addrs
                    .iter()
                    .flat_map(|addr| self.di.addr_map.get(addr))
                    .filter(|d| {
                        0 < d.rwd_amount && matches!(d.state, DelegationState::Bond)
                    })
            })
            .collect()
    }

    /// Clean delegation states along with each new block.
    pub fn delegation_process(&mut self) {
        let h = self.cur_height;
        self.di
            .end_height_map
            .range(..=h)
            .map(|(_, addr)| addr)
            .flatten()
            .copied()
            .collect::<Vec<_>>()
            .into_iter()
            .for_each(|addr| {
                let mut need_change_power = false;
                let (v, am) = if let Some(d) = self.di.addr_map.get_mut(&addr) {
                    if DelegationState::Locked == d.state {
                        d.state = DelegationState::Bond;
                        need_change_power = true;
                    }
                    (d.validator, d.amount)
                } else {
                    return;
                };

                if need_change_power {
                    // total amount of all delegations
                    self.di.total_amount -= am;
                    // reduce the power of the target validator
                    ruc::info_omit!(self.validator_change_power(&v, -am));
                }
            });

        let h = h.saturating_sub(BOND_BLOCK_CNT);
        if 2 < h {
            self.deletation_process_finished_before_height(h);
        }
    }

    // call this when:
    // - the bond period expired
    // - rewards have been paid successfully.
    //
    // @param h: included
    fn deletation_process_finished_before_height(&mut self, h: BlockHeight) {
        self.di
            .end_height_map
            .range(0..=h)
            .map(|(k, v)| (k.to_owned(), (*v).clone()))
            .collect::<Vec<_>>()
            .iter()
            .for_each(|(h, addrs)| {
                addrs.iter().for_each(|addr| {
                    if self.delegation_unbond(addr).is_ok() {
                        self.di
                            .end_height_map
                            .get_mut(&h)
                            .map(|addrs| addrs.remove(addr));
                    }
                });
                // this unwrap is safe
                if self.di.end_height_map.get(&h).unwrap().is_empty() {
                    self.di.end_height_map.remove(&h);
                }
            });
    }

    /// Penalize the FRAs by a specified address.
    #[inline(always)]
    pub fn governance_penalty(
        &mut self,
        addr: TendermintAddrRef,
        am: Amount,
    ) -> Result<()> {
        self.td_addr_to_app_pk(addr)
            .c(d!())
            .and_then(|pk| self.governance_penalty_by_pubkey(&pk, am).c(d!()))
    }

    // Penalize the FRAs by a specified pubkey.
    #[inline(always)]
    fn governance_penalty_by_pubkey(
        &mut self,
        addr: &XfrPublicKey,
        am: Amount,
    ) -> Result<()> {
        if am <= 0 {
            return Err(eg!("the amount must be a positive integer"));
        }
        self.delegation_import_extern_amount(addr, -am).c(d!())
    }

    // Look up the `XfrPublicKey`
    // co-responding to a specified 'tendermint node address'.
    #[inline(always)]
    fn td_addr_to_app_pk(&self, addr: TendermintAddrRef) -> Result<XfrPublicKey> {
        self.validator_get_current()
            .ok_or(eg!())
            .and_then(|vd| vd.addr_td_to_app.get(addr).copied().ok_or(eg!()))
    }

    /// Import extern amount changes,
    /// eg.. 'Block Rewards'/'Governance Penalty'
    pub fn delegation_import_extern_amount(
        &mut self,
        addr: &XfrPublicKey,
        am: Amount,
    ) -> Result<()> {
        let d = if let Some(d) = self.di.addr_map.get_mut(addr) {
            d
        } else {
            return Err(eg!("not exists"));
        };

        if DelegationState::Paid == d.state {
            return Err(eg!("delegation has been paid"));
        } else {
            d.rwd_amount = d.rwd_amount.saturating_add(am);
        }

        // extern changes can NOT increase vote power
        if 0 > am && self.addr_is_validator(addr) {
            // governance penalty scene
            self.validator_change_power(addr, am).c(d!())?;
        }

        Ok(())
    }

    /// Generate sha256 digest.
    #[inline(always)]
    pub fn hash(&self) -> Result<Digest> {
        bincode::serialize(self)
            .c(d!())
            .map(|bytes| sha256::hash(&bytes))
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn coinbase_pubkey(&self) -> XfrPublicKey {
        self.coinbase.pubkey
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn coinbase_keypair(&self) -> &XfrKeyPair {
        &self.coinbase.keypair
    }

    /// Add new FRA utxo to CoinBase.
    #[inline(always)]
    pub fn coinbase_recharge(&mut self, txo_sid: TxoSID) {
        self.coinbase.bank.insert(txo_sid);
    }

    /// Get all avaliable utos owned by CoinBase.
    #[inline(always)]
    pub fn coinbase_txos(&self) -> BTreeSet<TxoSID> {
        self.coinbase.bank.clone()
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn coinbase_clean_spent_txos(&mut self, ls: &LedgerStatus) {
        self.coinbase.bank.clone().into_iter().for_each(|sid| {
            if !ls.is_unspent_txo(sid) {
                self.coinbase.bank.remove(&sid);
            }
        });
    }

    /// Add new fra distribution plan.
    pub fn coinbase_config_fra_distribution(
        &mut self,
        ops: FraDistributionOps,
    ) -> Result<()> {
        let h = ops.hash().c(d!())?;

        if self.coinbase.distribution_hist.contains(&h) {
            return Err(eg!("already exists"));
        }

        // Update fra distribution history first.
        self.coinbase.distribution_hist.insert(h);

        let mut v;
        for (k, am) in ops.data.alloc_table.into_iter() {
            v = self.coinbase.distribution_plan.entry(k).or_insert(0);
            *v = v.checked_add(am).ok_or(eg!("overflow"))?;
        }

        Ok(())
    }

    /// Do the final payment on staking structures.
    pub fn coinbase_pay(&mut self, tx: &Transaction) -> Result<()> {
        if !self.is_coinbase_ops(tx) {
            return Ok(());
        }

        if !self.seems_valid_coinbase_ops(tx) {
            return Err(eg!());
        }

        self.coinbase_collect_payments(tx)
            .c(d!())
            .map(|(distribution, delegation)| {
                self.coinbase_pay_fra_distribution(&distribution);
                self.coinbase_pay_delegation(&delegation);
            })
    }

    // Check if a tx contains any inputs from coinbase,
    // if it does, then it must pass all checkers about coinbase.
    #[inline(always)]
    fn is_coinbase_ops(&self, tx: &Transaction) -> bool {
        tx.body.operations.iter().any(|o| {
            if let Operation::TransferAsset(ref ops) = o {
                if ops
                    .body
                    .transfer
                    .inputs
                    .iter()
                    .any(|i| i.public_key == self.coinbase.pubkey)
                {
                    return true;
                }
            }
            false
        })
    }

    // Check if this is a valid coinbase operation.
    //
    // - only `TransferAsset` operations are allowed
    // - all inputs must be owned by `CoinBase`
    // - all inputs and outputs must be `NonConfidential`
    // - only FRA are involved in this transaction
    // - all outputs must be owned by addresses in 'fra distribution' or 'delegation'
    //
    // **NOTE:** amount is not checked in this function !
    fn seems_valid_coinbase_ops(&self, tx: &Transaction) -> bool {
        let inputs_is_valid = |o: &TransferAsset| {
            o.body
                .transfer
                .inputs
                .iter()
                .all(|i| i.public_key == self.coinbase.pubkey)
        };

        let outputs_is_valid = |o: &TransferAsset| {
            o.body.transfer.outputs.iter().all(|i| {
                self.coinbase_pubkey() == i.public_key
                    || self.addr_is_in_distribution_plan(&i.public_key)
                    || self.addr_is_in_bond_delegation(&i.public_key)
            })
        };

        let only_nonconfidential_fra = |o: &TransferAsset| {
            o.body
                .transfer
                .inputs
                .iter()
                .chain(o.body.transfer.outputs.iter())
                .all(|i| {
                    if let XfrAssetType::NonConfidential(t) = i.asset_type {
                        if ASSET_TYPE_FRA == t {
                            return matches!(i.amount, XfrAmount::NonConfidential(_));
                        }
                    }
                    false
                })
        };

        let ops_is_valid = |ops: &Operation| {
            if let Operation::TransferAsset(o) = ops {
                inputs_is_valid(o) && outputs_is_valid(o) && only_nonconfidential_fra(o)
            } else {
                false
            }
        };

        tx.body.operations.iter().all(|o| ops_is_valid(o))
    }

    fn coinbase_collect_payments(
        &self,
        tx: &Transaction,
    ) -> Result<(HashMap<XfrPublicKey, u64>, HashMap<XfrPublicKey, u64>)> {
        let mut v: &mut u64;
        let mut distribution = map! {};
        let mut delegation = map! {};

        for o in tx.body.operations.iter() {
            if let Operation::TransferAsset(ref ops) = o {
                for u in ops.body.transfer.outputs.iter() {
                    if let XfrAssetType::NonConfidential(t) = u.asset_type {
                        if t == ASSET_TYPE_FRA {
                            if let XfrAmount::NonConfidential(am) = u.amount {
                                if self.addr_is_in_distribution_plan(&u.public_key) {
                                    v = distribution.entry(u.public_key).or_insert(0);
                                    *v = v.checked_add(am).ok_or(eg!("overflow"))?;
                                }
                                if self.addr_is_in_bond_delegation(&u.public_key) {
                                    v = delegation.entry(u.public_key).or_insert(0);
                                    *v = v.checked_add(am).ok_or(eg!("overflow"))?;
                                }
                            }
                        }
                    }
                }
            }
        }

        let xa = distribution
            .iter()
            .any(|(addr, am)| self.coinbase.distribution_plan.get(addr).unwrap() != am);
        let xb = delegation.iter().any(|(addr, am)| {
            self.delegation_get(addr).unwrap().rwd_amount != *am as Amount
        });

        if xa || xb {
            return Err(eg!("amount not match"));
        }

        Ok((distribution, delegation))
    }

    // amounts have been checked in `coinbase_collect_payments`,
    fn coinbase_pay_fra_distribution(&mut self, payments: &HashMap<XfrPublicKey, u64>) {
        self.coinbase
            .distribution_plan
            .iter_mut()
            .for_each(|(k, am)| {
                // once paid, it was all paid
                if payments.contains_key(k) {
                    *am = 0;
                }
            });

        // clean 'completely paid' item
        self.coinbase.distribution_plan =
            mem::take(&mut self.coinbase.distribution_plan)
                .into_iter()
                .filter(|(_, am)| 0 < *am)
                .collect();
    }

    // - amounts have been checked in `coinbase_collect_payments`
    // - pubkey existances have been checked in `seems_valid_coinbase_ops`
    // - delegation states has been checked in `addr_is_in_bond_delegation`
    #[inline(always)]
    fn coinbase_pay_delegation(&mut self, payments: &HashMap<XfrPublicKey, u64>) {
        payments.keys().for_each(|k| {
            self.delegation_get_mut(k).unwrap().state = DelegationState::Paid;
        });
    }

    // For addresses in delegation state,
    // postpone the distribution until the delegation ends.
    #[inline(always)]
    fn addr_is_in_distribution_plan(&self, pk: &XfrPublicKey) -> bool {
        self.coinbase.distribution_plan.contains_key(pk)
            && !self.di.addr_map.contains_key(pk)
    }

    #[inline(always)]
    fn addr_is_in_bond_delegation(&self, pk: &XfrPublicKey) -> bool {
        if let Some(dlg) = self.di.addr_map.get(pk) {
            matches!(dlg.state, DelegationState::Bond)
        } else {
            false
        }
    }

    #[inline(always)]
    fn addr_is_validator(&self, pk: &XfrPublicKey) -> bool {
        self.validator_get_current()
            .map(|v| v.body.contains_key(pk))
            .unwrap_or(false)
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn fra_distribution_get_plan(&self) -> &BTreeMap<XfrPublicKey, u64> {
        &self.coinbase.distribution_plan
    }

    /// A helper for setting block rewards in ABCI.
    pub fn set_last_block_rewards(
        &mut self,
        addr: TendermintAddrRef,
        block_vote_power: Option<i64>,
    ) -> Result<()> {
        let pk = self.td_addr_to_app_pk(addr).c(d!())?;
        if !self.addr_is_validator(&pk) {
            return Err(eg!("not validator"));
        }

        let h = self.cur_height;
        let return_rate = self.get_block_rewards_rate();

        self.di
            .addr_map
            .values_mut()
            .filter(|d| d.validator == pk)
            .for_each(|d| {
                ruc::info_omit!(d.set_delegation_rewards(h, return_rate));
            });

        if let Some(power) = block_vote_power {
            let total_power = self.validator_total_power();
            if 0 < total_power {
                self.set_proposer_rewards(&pk, [power, total_power])
                    .c(d!())?;
            }
        }

        Ok(())
    }

    /// SEE:
    /// https://www.notion.so/findora/PoS-Stage-1-Consensus-Rewards-Penalties-72f5c9a697ff461c89c3728e34348834#3d2f1b8ff8244632b715abdd42b6a67b
    pub fn get_block_rewards_rate(&self) -> [u64; 2] {
        const RATE_RULE: [([u64; 2], u64); 8] = [
            ([0, 10], 20),
            ([10, 20], 17),
            ([20, 30], 14),
            ([30, 40], 11),
            ([40, 50], 8),
            ([50, 50], 5),
            ([60, 67], 2),
            ([67, 101], 1),
        ];

        let p = [self.di.total_amount as u64, FRA_TOTAL_AMOUNT];
        for ([low, high], rate) in RATE_RULE.iter() {
            if p[0] * 100 < p[1] * high && p[0] * 100 >= p[1] * low {
                return [*rate, 100];
            }
        }

        unreachable!();
    }

    // SEE:
    // https://www.notion.so/findora/PoS-Stage-1-Consensus-Rewards-Penalties-72f5c9a697ff461c89c3728e34348834#3d2f1b8ff8244632b715abdd42b6a67b
    fn set_proposer_rewards(
        &mut self,
        proposer: &XfrPublicKey,
        vote_percent: [i64; 2],
    ) -> Result<()> {
        const RATE_RULE: [([u64; 2], u64); 6] = [
            ([0, 66_6667], 0),
            ([66_6667, 75_0000], 1),
            ([75_0000, 83_3333], 2),
            ([83_3333, 91_6667], 3),
            ([91_6667, 100_0000], 4),
            ([100_0000, 100_0001], 5),
        ];

        let p = [vote_percent[0] as u64, vote_percent[1] as u64];

        if p[0] > p[1] {
            return Err(eg!());
        }

        for ([low, high], rate) in RATE_RULE.iter() {
            if p[0] * 100 < p[1] * high && p[0] * 100 >= p[1] * low {
                let h = self.cur_height;
                return self
                    .delegation_get_mut(proposer)
                    .ok_or(eg!())
                    .and_then(|d| d.set_delegation_rewards(h, [*rate, 100]).c(d!()));
            }
        }

        unreachable!();
    }
}

/// How many FRA units per FRA
pub const FRA: u64 = 10_u64.pow(FRA_DECIMALS as u32);

/// Total amount of FRA-units issuance.
pub const FRA_TOTAL_AMOUNT: u64 = 210_0000_0000 * FRA;

const MIN_DELEGATION_AMOUNT: u64 = 32 * FRA;
const MAX_DELEGATION_AMOUNT: u64 = FRA_TOTAL_AMOUNT;

const BLOCK_HEIGHT_MAX: u64 = i64::MAX as u64;

/// The 24-words mnemonic of 'FRA CoinBase Address'.
pub const COIN_BASE_MNEMONIC: &str = "load second west source excuse skin thought inside wool kick power tail universe brush kid butter bomb other mistake oven raw armed tree walk";

lazy_static! {
    #[allow(missing_docs)]
    pub static ref COINBASE_PK: XfrPublicKey = COINBASE_KP.get_pk();
    #[allow(missing_docs)]
    pub static ref COINBASE_KP: XfrKeyPair = pnk!(wallet::restore_keypair_from_mnemonic_default(COIN_BASE_MNEMONIC));
}

// A limitation from
// [tendermint](https://docs.tendermint.com/v0.33/spec/abci/apps.html#validator-updates)
//
// > Note that the maximum total power of the validator set
// > is bounded by MaxTotalVotingPower = MaxInt64 / 8.
// > Applications are responsible for ensuring
// > they do not make changes to the validator set
// > that cause it to exceed this limit.
const MAX_TOTAL_POWER: Amount = Amount::MAX / 8;

// The max vote power of any validator
// can not exceed 20% of total power.
const MAX_POWER_PERCENT_PER_VALIDATOR: [u128; 2] = [1, 5];

// Block time interval, in seconds.
const BLOCK_INTERVAL: u64 = 15 + 1;

/// The lock time after the delegation expires, about 21 days.
#[cfg(not(feature = "abci_mock"))]
pub const BOND_BLOCK_CNT: u64 = 3600 * 24 * 21 / BLOCK_INTERVAL;

/// used in test env
#[cfg(feature = "abci_mock")]
pub const BOND_BLOCK_CNT: u64 = 10;

// minimal number of validators
pub(crate) const VALIDATORS_MIN: usize = 6;

/// The minimum weight threshold required
/// when updating validator information, 2/3.
pub const COSIG_THRESHOLD_DEFAULT: [u64; 2] = [2, 3];

type Memo = String;

/// block height of tendermint
pub type BlockHeight = u64;

// use i64 to keep compatible with the logic of asset penalty
type Amount = i64;

/// sha256(pubkey)[:20] in hex format
pub type TendermintAddr = String;
type TendermintAddrRef<'a> = &'a str;

type ValidatorInfo = BTreeMap<BlockHeight, ValidatorData>;

/// Data of the effective validators on a specified height.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ValidatorData {
    pub(crate) height: BlockHeight,
    pub(crate) cosig_rule: CoSigRule,
    /// major data of validators.
    pub body: BTreeMap<XfrPublicKey, Validator>,
    // <tendermint validator address> => XfrPublicKey
    addr_td_to_app: BTreeMap<TendermintAddr, XfrPublicKey>,
}

impl Default for ValidatorData {
    fn default() -> Self {
        ValidatorData {
            height: 1,
            cosig_rule: pnk!(Self::gen_cosig_rule(&[])),
            body: BTreeMap::new(),
            addr_td_to_app: BTreeMap::new(),
        }
    }
}

impl ValidatorData {
    #[allow(missing_docs)]
    pub fn new(h: BlockHeight, v_set: Vec<Validator>) -> Result<Self> {
        if h < 1 {
            return Err(eg!("invalid start height"));
        }

        let mut body = BTreeMap::new();
        let mut addr_td_to_app = BTreeMap::new();
        for v in v_set.into_iter() {
            addr_td_to_app.insert(td_pubkey_to_td_addr(&v.td_pubkey), v.id);
            if body.insert(v.id, v).is_some() {
                return Err(eg!("duplicate entries"));
            }
        }

        let cosig_rule =
            Self::gen_cosig_rule(&body.keys().copied().collect::<Vec<_>>()).c(d!())?;

        Ok(ValidatorData {
            height: h,
            cosig_rule,
            body,
            addr_td_to_app,
        })
    }

    fn gen_cosig_rule(validator_ids: &[XfrPublicKey]) -> Result<CoSigRule> {
        CoSigRule::new(
            COSIG_THRESHOLD_DEFAULT,
            validator_ids.iter().copied().map(|v| (v, 1)).collect(),
        )
    }

    /// The initial weight of every validators is equal(vote power == 1).
    pub fn set_cosig_rule(&mut self, validator_ids: &[XfrPublicKey]) -> Result<()> {
        Self::gen_cosig_rule(validator_ids).c(d!()).map(|rule| {
            self.cosig_rule = rule;
        })
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn get_cosig_rule(&self) -> &CoSigRule {
        &self.cosig_rule
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn get_cosig_rule_mut(&mut self) -> &mut CoSigRule {
        &mut self.cosig_rule
    }

    #[inline(always)]
    #[allow(missing_docs)]
    pub fn get_validators(&self) -> &BTreeMap<XfrPublicKey, Validator> {
        &self.body
    }
}

// the same address is not allowed to delegate twice at the same time,
// so it is feasible to use `XfrPublicKey` as the map key.
#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
struct DelegationInfo {
    total_amount: Amount,
    addr_map: BTreeMap<XfrPublicKey, Delegation>,
    end_height_map: BTreeMap<BlockHeight, BTreeSet<XfrPublicKey>>,
}

impl DelegationInfo {
    fn new() -> Self {
        DelegationInfo {
            total_amount: 0,
            addr_map: BTreeMap::new(),
            end_height_map: BTreeMap::new(),
        }
    }
}

/// Validator info
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Validator {
    /// pubkey in the context of tendermint
    pub td_pubkey: Vec<u8>,
    /// vote power in the context of Staking
    pub td_power: Amount,
    /// public key of validator, aka 'Validator ID'.
    ///
    /// staking rewards will be paid to this addr
    /// - eg.. self-delegation rewards
    /// - eg.. block rewards
    pub id: XfrPublicKey,
    /// optional descriptive information
    pub memo: Option<Memo>,
}

impl Validator {
    #[allow(missing_docs)]
    pub fn new(
        td_pubkey: Vec<u8>,
        td_power: Amount,
        id: XfrPublicKey,
        memo: Option<Memo>,
    ) -> Self {
        Validator {
            td_pubkey,
            td_power,
            id,
            memo,
        }
    }
}

/// FRA delegation, include:
/// - user delegation
/// - validator's self-delegation
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Delegation {
    /// total `NonConfidential` FRAs in a staking address
    pub amount: Amount,
    /// the target validator to delegated to
    pub validator: XfrPublicKey,
    /// delegation rewards will be paid to this pk
    pub rwd_pk: XfrPublicKey,
    /// the height at which the delegation starts
    pub start_height: BlockHeight,
    /// the height at which the delegation ends
    ///
    /// **NOTE:** before users can actually get the rewards,
    /// they need to wait for an extra `BOND_BLOCK_CNT` period
    pub end_height: BlockHeight,
    #[allow(missing_docs)]
    pub state: DelegationState,
    /// set this field when `Locked` state finished
    pub rwd_amount: Amount,
}

#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum DelegationState {
    /// during delegation
    Locked,
    /// delegation finished,
    /// entered bond time
    Bond,
    /// during or after bond time,
    /// and rewards have been paid successfully,
    /// the co-responding account should be unbond
    Paid,
}

impl Default for DelegationState {
    fn default() -> Self {
        DelegationState::Locked
    }
}

impl Delegation {
    /// > **NOTE:**
    /// > use 'AssignAdd' instead of 'Assign'
    /// > to keep compatible with the logic of governance penalty.
    pub fn set_delegation_rewards(
        &mut self,
        cur_height: BlockHeight,
        return_rate: [u64; 2],
    ) -> Result<()> {
        if self.end_height < cur_height || DelegationState::Locked != self.state {
            return Ok(());
        }

        calculate_delegation_rewards(self.amount, return_rate)
            .c(d!())
            .and_then(|n| {
                self.rwd_amount
                    .checked_add(n as Amount)
                    .ok_or(eg!("overflow"))
            })
            .map(|n| {
                self.rwd_amount = n;
            })
    }
}

/// Calculate the amount(in FRA units) that
/// should be paid to the owner of this delegation.
pub fn calculate_delegation_rewards(amount: i64, return_rate: [u64; 2]) -> Result<u64> {
    let am = amount as u128;
    let block_itv = BLOCK_INTERVAL as u128;
    let return_rate = [return_rate[0] as u128, return_rate[1] as u128];

    am.checked_mul(return_rate[0])
        .and_then(|i| i.checked_mul(block_itv))
        .and_then(|i| {
            return_rate[1]
                .checked_mul(365 * 24 * 3600)
                .and_then(|j| i.checked_div(j))
        })
        .ok_or(eg!("overflow"))
        .and_then(|n| u64::try_from(n).c(d!()))
}

// All transactions sent from CoinBase must support idempotence.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct CoinBase {
    pubkey: XfrPublicKey,
    keypair: XfrKeyPair,
    bank: BTreeSet<TxoSID>,
    distribution_hist: BTreeSet<Digest>,
    distribution_plan: BTreeMap<XfrPublicKey, u64>,
}

impl Eq for CoinBase {}

impl PartialEq for CoinBase {
    fn eq(&self, other: &Self) -> bool {
        self.pubkey == other.pubkey
    }
}

impl Default for CoinBase {
    fn default() -> Self {
        Self::gen()
    }
}

impl CoinBase {
    fn gen() -> Self {
        CoinBase {
            pubkey: COINBASE_KP.get_pk(),
            keypair: COINBASE_KP.clone(),
            bank: BTreeSet::new(),
            distribution_hist: BTreeSet::new(),
            distribution_plan: BTreeMap::new(),
        }
    }
}

/// sha256(pubkey)[:20]
#[inline(always)]
pub fn td_pubkey_to_td_addr(pubkey: &[u8]) -> String {
    hex::encode(&sha2::Sha256::digest(pubkey)[..20])
}

#[cfg(test)]
mod test {
    // TODO
}
