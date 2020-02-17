#![deny(warnings)]
use ledger::data_model::AssetTypeCode;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Output};
use std::str::from_utf8;

extern crate exitcode;

// TODOs:
// Derive path and command name from cwd
// Figure out how to colorize stdout and stderr

const COMMAND: &str = "../../target/debug/txn_builder_cli";

//
// Helper functions: create and store without path
//
#[cfg(test)]
fn create_no_path() -> io::Result<Output> {
  Command::new(COMMAND).arg("create").output()
}

#[cfg(test)]
fn keygen_no_path() -> io::Result<Output> {
  Command::new(COMMAND).arg("keygen").output()
}

#[cfg(test)]
fn pubkeygen_no_path() -> io::Result<Output> {
  Command::new(COMMAND).arg("pubkeygen").output()
}

#[cfg(test)]
fn store_sids_no_path(amount: &str) -> io::Result<Output> {
  Command::new(COMMAND).args(&["store", "sids"])
                       .args(&["--indices", amount])
                       .output()
}

#[cfg(test)]
fn store_blind_asset_record_no_path(amount: &str,
                                    asset_type: &str,
                                    pub_key_path: &str)
                                    -> io::Result<Output> {
  Command::new(COMMAND).args(&["store", "blind_asset_record"])
                       .args(&["--amount", amount])
                       .args(&["--asset_type", asset_type])
                       .args(&["--pub_key_path", pub_key_path])
                       .output()
}

#[cfg(test)]
fn get_findora_dir() -> String {
  let findora_dir = {
    let home_dir = dirs::home_dir().unwrap_or_else(|| Path::new(".").to_path_buf());
    format!("{}/.findora", home_dir.to_str().unwrap_or("./.findora"))
  };

  findora_dir
}

#[cfg(test)]
fn remove_txn_dir() {
  fs::remove_dir_all(format!("{}/txn", get_findora_dir())).unwrap();
}

#[cfg(test)]
fn remove_keypair_dir() {
  fs::remove_dir_all(format!("{}/keypair", get_findora_dir())).unwrap();
}

#[cfg(test)]
fn remove_pubkey_dir() {
  fs::remove_dir_all(format!("{}/pubkey", get_findora_dir())).unwrap();
}

#[cfg(test)]
fn remove_values_dir() {
  fs::remove_dir_all(format!("{}/values", get_findora_dir())).unwrap();
}

//
// Helper functions: create and store with path
//
#[cfg(test)]
fn create_with_path(path: &str) -> io::Result<Output> {
  Command::new(COMMAND).args(&["create", "--name", path])
                       .output()
}

#[cfg(test)]
fn create_overwrite_path(path: &str) -> io::Result<Output> {
  Command::new(COMMAND).args(&["create", "--name", path])
                       .arg("--force")
                       .output()
}

#[cfg(test)]
fn keygen_with_path(path: &str) -> io::Result<Output> {
  Command::new(COMMAND).args(&["keygen", "--name", path])
                       .output()
}

#[cfg(test)]
fn pubkeygen_with_path(path: &str) -> io::Result<Output> {
  Command::new(COMMAND).args(&["pubkeygen", "--name", path])
                       .output()
}

#[cfg(test)]
fn store_sids_with_path(path: &str, amount: &str) -> io::Result<Output> {
  Command::new(COMMAND).args(&["store", "sids"])
                       .args(&["--path", path])
                       .args(&["--indices", amount])
                       .output()
}

#[cfg(test)]
fn store_blind_asset_record_with_path(path: &str,
                                      amount: &str,
                                      asset_type: &str,
                                      pub_key_path: &str)
                                      -> io::Result<Output> {
  Command::new(COMMAND).args(&["store", "blind_asset_record"])
                       .args(&["--path", path])
                       .args(&["--amount", amount])
                       .args(&["--asset_type", asset_type])
                       .args(&["--pub_key_path", pub_key_path])
                       .output()
}

//
// Helper functions: define, issue and transfer
//
#[cfg(test)]
fn define_asset(txn_builder_path: &str,
                key_pair_path: &str,
                token_code: &str,
                memo: &str)
                -> io::Result<Output> {
  Command::new(COMMAND).args(&["--txn", txn_builder_path])
                       .args(&["--key_pair", key_pair_path])
                       .args(&["add", "define_asset"])
                       .args(&["--token_code", token_code])
                       .args(&["--memo", memo])
                       .output()
}

#[cfg(test)]
fn issue_asset(txn_builder_path: &str,
               key_pair_path: &str,
               token_code: &str,
               amount: &str)
               -> io::Result<Output> {
  Command::new(COMMAND).args(&["--txn", txn_builder_path])
                       .args(&["--key_pair", key_pair_path])
                       .args(&["add", "issue_asset"])
                       .args(&["--token_code", token_code])
                       .args(&["--amount", amount])
                       .output()
}

#[cfg(test)]
fn transfer_asset(txn_builder_path: &str,
                  key_pair_path: &str,
                  sids_path: &str,
                  blind_asset_record_paths: &str,
                  input_amounts: &str,
                  output_amounts: &str,
                  address_paths: &str)
                  -> io::Result<Output> {
  Command::new(COMMAND).args(&["--txn", txn_builder_path])
                       .args(&["--key_pair", key_pair_path])
                       .args(&["add", "transfer_asset"])
                       .args(&["--sids_path", sids_path])
                       .args(&["--blind_asset_record_paths", blind_asset_record_paths])
                       .args(&["--input_amounts", input_amounts])
                       .args(&["--output_amounts", output_amounts])
                       .args(&["--address_paths", address_paths])
                       .output()
}

#[cfg(test)]
fn issue_and_transfer_asset(txn_builder_path: &str,
                            issuer_key_pair_path: &str,
                            recipient_key_pair_path: &str,
                            amount: &str,
                            token_code: &str)
                            -> io::Result<Output> {
  Command::new(COMMAND).args(&["--txn", txn_builder_path])
                       .args(&["--key_pair", issuer_key_pair_path])
                       .args(&["add", "issue_and_transfer_asset"])
                       .args(&["--recipient_key_pair_path", recipient_key_pair_path])
                       .args(&["--amount", amount])
                       .args(&["--token_code", token_code])
                       .output()
}

// Helper functions: submit transaction

#[cfg(test)]
fn submit(txn_builder_path: &str) -> io::Result<Output> {
  Command::new(COMMAND).args(&["--txn", txn_builder_path])
                       .arg("submit")
                       .output()
}

#[cfg(test)]
fn submit_and_store_sid(txn_builder_path: &str) -> io::Result<Output> {
  Command::new(COMMAND).args(&["--txn", txn_builder_path])
                       .arg("submit_and_store_sid")
                       .output()
}

// Helper function: load funds
#[cfg(test)]
fn load_funds(txn_builder_path: &str,
              issuer_key_pair_path: &str,
              recipient_key_pair_path: &str,
              amount: &str,
              token_code: &str)
              -> io::Result<Output> {
  Command::new(COMMAND).args(&["--txn", txn_builder_path])
                       .args(&["--key_pair", issuer_key_pair_path])
                       .arg("load_funds")
                       .args(&["--recipient_key_pair_path", recipient_key_pair_path])
                       .args(&["--amount", amount])
                       .args(&["--token_code", token_code])
                       .output()
}

// Helper function: initiate loan
#[cfg(test)]
fn init_loan(txn_builder_path: &str,
             issuer_key_pair_path: &str,
             lender_key_pair_path: &str,
             borrower_key_pair_path: &str,
             fiat_code: &str,
             debt_code: &str,
             amount: &str)
             -> io::Result<Output> {
  Command::new(COMMAND).args(&["--txn", txn_builder_path])
                       .args(&["--key_pair", issuer_key_pair_path])
                       .arg("init_loan")
                       .args(&["--lender_key_pair_path", lender_key_pair_path])
                       .args(&["--borrower_key_pair_path", borrower_key_pair_path])
                       .args(&["--fiat_code", fiat_code])
                       .args(&["--debt_code", debt_code])
                       .args(&["--amount", amount])
                       .output()
}

//
// No path
//
#[test]
fn test_no_path() {
  // Create transaction builder
  let output = create_no_path().expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());

  // Generate key pair
  let output = keygen_no_path().expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());

  // Generate public key
  let output = pubkeygen_no_path().expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());

  // Store sids
  let output = store_sids_no_path("1,2,4").expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());

  // Store blind asset record
  let pubkeygen_path = "pub_no_bar_path";
  pubkeygen_with_path(pubkeygen_path).expect("Failed to generate public key");

  let output = store_blind_asset_record_no_path("10", "0000000000000000", pubkeygen_path).expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file(pubkeygen_path).unwrap();
  assert!(output.status.success());

  remove_txn_dir();
  remove_keypair_dir();
  remove_pubkey_dir();
  remove_values_dir();
}

//
// Subcommand or argument missing
// Note: Not all cases are tested
//
#[test]
fn test_call_no_args() {
  let output = Command::new(COMMAND).output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert_eq!(output.status.code(), Some(exitcode::USAGE));
  assert!(from_utf8(&output.stdout).unwrap().contains(&"Subcommand missing or not recognized. Try --help".to_owned()));
}

#[test]
fn test_store_no_args() {
  let output = Command::new(COMMAND).arg("store")
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert_eq!(output.status.code(), Some(exitcode::USAGE));
  assert!(from_utf8(&output.stdout).unwrap().contains(&"Subcommand missing or not recognized. Try store --help".to_owned()));
}

#[test]
fn test_add_no_args() {
  keygen_no_path().expect("Failed to generate key pair");

  let output = Command::new(COMMAND).arg("add")
                                    .output()
                                    .expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  remove_keypair_dir();

  assert_eq!(output.status.code(), Some(exitcode::USAGE));
  assert!(from_utf8(&output.stdout).unwrap().contains(&"Subcommand missing or not recognized. Try add --help".to_owned()));
}

//
// "help" arg
// Note: Not all cases with "help" arg are tested
//
#[test]
fn test_call_with_help() {
  let output = Command::new(COMMAND).arg("help")
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success())
}

#[test]
fn test_create_with_help() {
  let output = Command::new(COMMAND).args(&["create", "--help"])
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success())
}

#[test]
fn test_keygen_with_help() {
  let output = Command::new(COMMAND).args(&["keygen", "--help"])
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());
}

#[test]
fn test_pubkeygen_with_help() {
  let output = Command::new(COMMAND).args(&["pubkeygen", "--help"])
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success())
}

#[test]
fn test_add_with_help() {
  let output = Command::new(COMMAND).args(&["add", "--help"])
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success())
}

#[test]
fn test_define_asset_with_help() {
  let output = Command::new(COMMAND).args(&["add", "define_asset", "--help"])
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success())
}

#[test]
fn test_issue_asset_with_help() {
  let output = Command::new(COMMAND).args(&["add", "issue_asset", "--help"])
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success())
}

#[test]
fn test_transfer_asset_with_help() {
  let output = Command::new(COMMAND).args(&["add", "transfer_asset", "--help"])
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success())
}

#[test]
fn test_submit_with_help() {
  let output = Command::new(COMMAND).args(&["submit", "--help"])
                                    .output()
                                    .expect("failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success())
}

//
// File creation (txn builder, key pair, and public key)
//
#[test]
fn test_invalid_valid_overwrite_and_rename_path() {
  // Invalid path
  let output = create_with_path(".").expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert_eq!(output.status.code(), Some(exitcode::USAGE));
  assert!(from_utf8(&output.stdout).unwrap()
                                   .contains(&"Is directory".to_owned()));

  // Valid path
  let path = "valid_path";
  let output = create_with_path(path).expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());

  // Overwrite existing file
  let output = create_overwrite_path(path).expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());

  // Rename existing file
  let output = create_with_path(path).expect("Failed to execute process");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());

  fs::remove_file("valid_path").unwrap();
  fs::remove_file("valid_path.0").unwrap();
}

#[test]
fn test_create_with_name() {
  // Create transaction builder
  let output = create_with_path("txn_builder").expect("Failed to create transaction builder");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file("txn_builder").unwrap();
  assert!(output.status.success());

  // Generate key pair
  let output = keygen_with_path("key_pair").expect("Failed to generate key pair");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file("key_pair").unwrap();
  assert!(output.status.success());

  // Generate public key
  let output = pubkeygen_with_path("pub").expect("Failed to generate public key");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file("pub").unwrap();
  assert!(output.status.success());
}

//
// Store (sids and blind asset record)
//
#[test]
fn test_store_with_path() {
  // Store sids
  let output = store_sids_with_path("sids", "1,2,4").expect("Failed to store sids");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file("sids").unwrap();
  assert!(output.status.success());

  // Store blind asset record
  let pubkeygen_path = "pub_with_bar_path";
  pubkeygen_with_path(pubkeygen_path).expect("Failed to generate public key");

  let output = store_blind_asset_record_with_path("bar", "10", "0000000000000000", pubkeygen_path).expect("Failed to store blind asset record");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file(pubkeygen_path).unwrap();
  fs::remove_file("bar").unwrap();

  assert!(output.status.success());
}

//
// Define, issue and transfer
//
#[test]
fn test_define_issue_and_transfer_with_args() {
  // Create transaction builder and key pair
  let txn_builder_file = "tb";
  let key_pair_file = "kp";
  create_with_path(txn_builder_file).expect("Failed to create transaction builder");
  keygen_with_path(key_pair_file).expect("Failed to generate key pair");

  // Define asset
  let token_code = AssetTypeCode::gen_random().to_base64();
  let output = define_asset(txn_builder_file,
                            key_pair_file,
                            &token_code,
                            "define an asset").expect("Failed to define asset");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());

  // Issue asset
  let output =
    issue_asset(txn_builder_file, key_pair_file, &token_code, "10").expect("Failed to issue asset");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  assert!(output.status.success());

  // Create files and generate public keys
  let files = vec!["pub1", "pub2", "pub3", "addr1", "addr2", "addr3", "s", "bar1", "bar2", "bar3"];
  for file in &files[0..6] {
    pubkeygen_with_path(file).expect("Failed to generate public key");
  }

  // Store sids and blind asset records
  store_sids_with_path(files[6], "1,2,4").expect("Failed to store sids");
  store_blind_asset_record_with_path(files[7],
                               "10",
                               &token_code,
                               files[0]).expect("Failed to store blind asset record");
  store_blind_asset_record_with_path(files[8],
                               "100",
                               &token_code,
                               files[1]).expect("Failed to store blind asset record");
  store_blind_asset_record_with_path(files[9],
                               "1000",
                               &token_code,
                               files[2]).expect("Failed to store blind asset record");

  // Transfer asset
  let output = transfer_asset(txn_builder_file,
                              key_pair_file,
                              files[6],
                              "bar1,bar2,bar3",
                              "1,2,3",
                              "1,1,4",
                              "addr1,addr2,addr3").expect("Failed to transfer asset");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file(txn_builder_file).unwrap();
  fs::remove_file(key_pair_file).unwrap();
  for file in files {
    fs::remove_file(file).unwrap();
  }

  assert!(output.status.success());
}

//
// Compose transaction and submit
//
#[test]
fn test_define_and_submit_with_args() {
  // Create txn builder and key pair
  let txn_builder_file = "tb_define_submit";
  let key_pair_file = "kp_define_submit";
  create_with_path(txn_builder_file).expect("Failed to create transaction builder");
  keygen_with_path(key_pair_file).expect("Failed to generate key pair");

  // Define asset
  define_asset(txn_builder_file,
               key_pair_file,
               &AssetTypeCode::gen_random().to_base64(),
               "Define an asset").expect("Failed to define asset");

  // Submit transaction
  let output = submit(txn_builder_file).expect("Failed to submit transaction");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file(txn_builder_file).unwrap();
  fs::remove_file(key_pair_file).unwrap();

  assert!(output.status.success());
}

#[test]
fn test_issue_transfer_and_submit_with_args() {
  // Create txn builder and key pairs
  let txn_builder_file = "tb_issue_transfer_args";
  let issuer_key_pair_file = "ikp";
  let recipient_key_pair_file = "rkp";
  create_with_path(txn_builder_file).expect("Failed to create transaction builder");
  keygen_with_path(issuer_key_pair_file).expect("Failed to generate key pair for the issuer");
  keygen_with_path(recipient_key_pair_file).expect("Failed to generate key pair for the recipient");

  // Define token code
  let token_code = AssetTypeCode::gen_random().to_base64();

  // Define asset
  define_asset(txn_builder_file,
               issuer_key_pair_file,
               &token_code,
               "Define an asset").expect("Failed to define asset");
  submit(txn_builder_file).expect("Failed to submit transaction");

  // Issue and transfer
  issue_and_transfer_asset(txn_builder_file,
                           issuer_key_pair_file,
                           recipient_key_pair_file,
                           "1000",
                           &token_code).expect("Failed to issue and transfer asset");

  // Submit transaction
  let output = submit(txn_builder_file).expect("Failed to submit transaction");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file(txn_builder_file).unwrap();
  fs::remove_file(issuer_key_pair_file).unwrap();
  fs::remove_file(recipient_key_pair_file).unwrap();

  assert!(output.status.success());
}

#[test]
fn test_load_funds_with_args() {
  // Create txn builder, key pairs, and public key
  let txn_builder_file = "tb_load_funds_args";
  let issuer_key_pair_file = "ikp_load_funds_args";
  let recipient_key_pair_file = "rkp_load_funds_args";
  create_with_path(txn_builder_file).expect("Failed to create transaction builder");
  keygen_with_path(issuer_key_pair_file).expect("Failed to generate key pair for the issuer");
  keygen_with_path(recipient_key_pair_file).expect("Failed to generate key pair for the recipient");

  // Define token code
  let token_code = AssetTypeCode::gen_random().to_base64();

  // Define asset
  define_asset(txn_builder_file,
               issuer_key_pair_file,
               &token_code,
               "Define an asset").expect("Failed to define asset");
  submit(txn_builder_file).expect("Failed to submit transaction");

  // Set the original record for the recipient
  issue_and_transfer_asset(txn_builder_file,
                           issuer_key_pair_file,
                           recipient_key_pair_file,
                           "1000",
                           &token_code).expect("Failed to issue and transfer asset");
  submit_and_store_sid(txn_builder_file).expect("Failed to submit transaction");

  // Load funds
  let output = load_funds(txn_builder_file,
                          issuer_key_pair_file,
                          recipient_key_pair_file,
                          "500",
                          &token_code).expect("Failed to load funds");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file(txn_builder_file).unwrap();
  fs::remove_file(issuer_key_pair_file).unwrap();
  fs::remove_file(recipient_key_pair_file).unwrap();

  assert!(output.status.success());
}

#[test]
fn test_init_loan_with_args() {
  // Create txn builder, key pairs, and public key
  let txn_builder_file = "tb_init_loan_args";
  let issuer_key_pair_file = "ikp_init_loan_args";
  let lender_key_pair_file = "lkp_init_loan_args";
  let borrower_key_pair_file = "bkp_init_loan_args";
  create_with_path(txn_builder_file).expect("Failed to create transaction builder");
  keygen_with_path(issuer_key_pair_file).expect("Failed to generate key pair for the issuer");
  keygen_with_path(lender_key_pair_file).expect("Failed to generate key pair for the lender");
  keygen_with_path(borrower_key_pair_file).expect("Failed to generate key pair for the borrower");

  // Define fiat asset
  let fiat_code = AssetTypeCode::gen_random().to_base64();
  define_asset(txn_builder_file,
               issuer_key_pair_file,
               &fiat_code,
               "Define fiat asset").expect("Failed to define fiat asset");
  submit(txn_builder_file).expect("Failed to submit transaction");
  fs::remove_file(txn_builder_file).unwrap();

  // Define debt asset
  create_with_path(txn_builder_file).expect("Failed to create transaction builder");
  let debt_code = AssetTypeCode::gen_random().to_base64();
  define_asset(txn_builder_file,
               borrower_key_pair_file,
               &debt_code,
               "Define debt asset").expect("Failed to define debt asset");
  submit(txn_builder_file).expect("Failed to submit transaction");

  // Set the original record for the borrower
  issue_and_transfer_asset(txn_builder_file,
                           issuer_key_pair_file,
                           borrower_key_pair_file,
                           "1000",
                           &fiat_code).expect("Failed to issue and transfer asset");
  submit_and_store_sid(txn_builder_file).expect("Failed to submit transaction");

  // Initiate loan
  let output = init_loan(txn_builder_file,
                         issuer_key_pair_file,
                         lender_key_pair_file,
                         borrower_key_pair_file,
                         &fiat_code,
                         &debt_code,
                         "500").expect("Failed to load funds");

  io::stdout().write_all(&output.stdout).unwrap();
  io::stdout().write_all(&output.stderr).unwrap();

  fs::remove_file(txn_builder_file).unwrap();
  fs::remove_file(issuer_key_pair_file).unwrap();
  fs::remove_file(lender_key_pair_file).unwrap();
  fs::remove_file(borrower_key_pair_file).unwrap();

  assert!(output.status.success());
}
