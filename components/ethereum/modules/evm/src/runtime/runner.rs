use super::stack::SubstrateStackState;
use crate::runtime::{Call, Create, Create2, Runner as RunnerT};
use crate::{App, Config, FeeCalculator, OnChargeEVMTransaction};
use evm::backend::Backend as BackendT;
use evm::executor::{StackExecutor, StackSubstateMetadata};
use evm::ExitReason;
use fp_core::ensure;
use fp_evm::{CallInfo, CreateInfo, ExecutionInfo, PrecompileSet, Vicinity};
use primitive_types::{H160, H256, U256};
use ruc::{eg, Result};
use sha3::{Digest, Keccak256};
use std::marker::PhantomData;

#[derive(Default)]
pub struct Runner<T: Config> {
    _marker: PhantomData<T>,
}

impl<T: Config> Runner<T> {
    /// Execute an EVM operation.
    pub fn execute<'config, F, R>(
        source: H160,
        value: U256,
        gas_limit: u64,
        gas_price: Option<U256>,
        nonce: Option<U256>,
        config: &'config evm::Config,
        f: F,
    ) -> Result<ExecutionInfo<R>>
    where
        F: FnOnce(
            &mut StackExecutor<'config, SubstrateStackState<'_, 'config, T>>,
        ) -> (ExitReason, R),
    {
        // Gas price check is skipped when performing a gas estimation.
        let gas_price = match gas_price {
            Some(gas_price) => {
                ensure!(
                    gas_price >= T::FeeCalculator::min_gas_price(),
                    "GasPriceTooLow"
                );
                gas_price
            }
            None => Default::default(),
        };

        let vicinity = Vicinity {
            gas_price,
            origin: source,
        };

        let metadata = StackSubstateMetadata::new(gas_limit, &config);
        let state = SubstrateStackState::new(&vicinity, metadata);
        let mut executor =
            StackExecutor::new_with_precompile(state, config, T::Precompiles::execute);

        let total_fee = gas_price
            .checked_mul(U256::from(gas_limit))
            .ok_or(eg!("FeeOverflow"))?;
        let total_payment =
            value.checked_add(total_fee).ok_or(eg!("PaymentOverflow"))?;
        let source_account = App::<T>::account_basic(&source);
        ensure!(source_account.balance >= total_payment, eg!("BalanceLow"));

        if let Some(nonce) = nonce {
            ensure!(source_account.nonce == nonce, eg!("InvalidNonce"));
        }

        // Deduct fee from the `source` account.
        let fee = T::OnChargeTransaction::withdraw_fee(&source, total_fee)?;

        // Execute the EVM call.
        let (reason, retv) = f(&mut executor);

        let used_gas = U256::from(executor.used_gas());
        let actual_fee = executor.fee(gas_price);
        log::debug!(
            target: "evm",
            "Execution {:?} [source: {:?}, value: {}, gas_limit: {}, actual_fee: {}]",
            reason,
            source,
            value,
            gas_limit,
            actual_fee
        );

        // Refund fees to the `source` account if deducted more before,
        T::OnChargeTransaction::correct_and_deposit_fee(&source, actual_fee, fee)?;

        let state = executor.into_state();

        for address in state.substate.deletes {
            log::debug!(
                target: "evm",
                "Deleting account at {:?}",
                address
            );
            App::<T>::remove_account(&address)
        }

        for log in &state.substate.logs {
            log::trace!(
                target: "evm",
                "Inserting log for {:?}, topics ({}) {:?}, data ({}): {:?}]",
                log.address,
                log.topics.len(),
                log.topics,
                log.data.len(),
                log.data
            );
            // Module::<T>::deposit_event(Event::<T>::Log(Log {
            //     address: log.address,
            //     topics: log.topics.clone(),
            //     data: log.data.clone(),
            // }));
        }

        Ok(ExecutionInfo {
            value: retv,
            exit_reason: reason,
            used_gas,
            logs: state.substate.logs,
        })
    }
}

impl<T: Config> RunnerT for Runner<T> {
    fn call(args: Call) -> Result<CallInfo> {
        Self::execute(
            args.source,
            args.value,
            args.gas_limit,
            args.gas_price,
            args.nonce,
            T::config(),
            |executor| {
                executor.transact_call(
                    args.source,
                    args.target,
                    args.value,
                    args.input,
                    args.gas_limit,
                )
            },
        )
    }

    fn create(args: Create) -> Result<CreateInfo> {
        Self::execute(
            args.source,
            args.value,
            args.gas_limit,
            args.gas_price,
            args.nonce,
            T::config(),
            |executor| {
                let address = executor.create_address(evm::CreateScheme::Legacy {
                    caller: args.source,
                });
                (
                    executor.transact_create(
                        args.source,
                        args.value,
                        args.init,
                        args.gas_limit,
                    ),
                    address,
                )
            },
        )
    }

    fn create2(args: Create2) -> Result<CreateInfo> {
        let code_hash = H256::from_slice(Keccak256::digest(&args.init).as_slice());
        Self::execute(
            args.source,
            args.value,
            args.gas_limit,
            args.gas_price,
            args.nonce,
            T::config(),
            |executor| {
                let address = executor.create_address(evm::CreateScheme::Create2 {
                    caller: args.source,
                    code_hash,
                    salt: args.salt,
                });
                (
                    executor.transact_create2(
                        args.source,
                        args.value,
                        args.init,
                        args.salt,
                        args.gas_limit,
                    ),
                    address,
                )
            },
        )
    }
}
