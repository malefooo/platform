//!
//! # Staking About Operations
//!
//! All the logic in this module relies on other operations in the same transaction
//! to prevent replay attacks, and it does not implement this mechanism by itself.
//!
//! In the current implementation, the first operation must be a `TransferAsset`.
//!

pub mod delegation;
pub mod governance;
pub mod update_validator;
