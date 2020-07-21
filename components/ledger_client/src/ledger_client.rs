#![deny(warnings)]
use cryptohash::sha256::Digest as BitDigest;
use ledger::data_model::{AssetRules, AssetTypeCode, Operation, Transaction};
use ledger::store::helpers::*;
use rand_chacha::ChaChaRng;
use rand_core::SeedableRng;
use zei::xfr::sig::XfrSignature;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let protocol = "http";
  let host = "localhost";
  let port = "8668";

  let client = reqwest::Client::new();

  let mut resp_gs = client.get(&format!("{}://{}:{}/global_state", protocol, host, port))
                          .send()?;
  let (_comm, seq_id, _sig): (BitDigest, u64, XfrSignature) =
    serde_json::from_str(&resp_gs.text()?[..]).unwrap();

  let mut prng = ChaChaRng::from_entropy();
  let mut tx = Transaction::from_seq_id(seq_id);

  let token_code1 = AssetTypeCode::from_identical_byte(1);
  let keypair = build_keys(&mut prng);

  let asset_body = asset_creation_body(&token_code1,
                                       keypair.get_pk_ref(),
                                       AssetRules::default(),
                                       None,
                                       None);
  let asset_create = asset_creation_operation(&asset_body, &keypair);
  tx.body
    .operations
    .push(Operation::DefineAsset(asset_create));

  // env_logger::init();

  let token_code_b64 = token_code1.to_base64();
  println!("\n\nQuery asset_token {:?}", &token_code1);

  let mut res = reqwest::get(&format!("http://{}:{}/{}/{}",
                                      &host, &port, "asset_token", &token_code_b64))?;

  println!("Status: {}", res.status());
  println!("Headers:\n{:?}", res.headers());

  // copy the response body directly to stdout
  std::io::copy(&mut res, &mut std::io::stdout())?;

  println!("\n\nSubmit transaction");

  res = client.post(&format!("http://{}:{}/{}", &host, &port, "submit_transaction"))
              .json(&tx)
              .send()?;
  println!("Status: {}", res.status());
  println!("Headers:\n{:?}", res.headers());

  // copy the response body directly to stdout
  std::io::copy(&mut res, &mut std::io::stdout())?;

  println!("\n\nQuery global_state {:?} again", &token_code1);
  res = reqwest::get(&format!("http://{}:{}/{}/{}", &host, &port, "global_state", 0))?;

  println!("Status: {}", res.status());
  println!("Headers:\n{:?}", res.headers());

  let mut res = reqwest::get(&format!("http://{}:{}/{}/{}",
                                      &host, &port, "asset_token", &token_code_b64))?;

  println!("Status: {}", res.status());
  println!("Headers:\n{:?}", res.headers());

  // copy the response body directly to stdout
  std::io::copy(&mut res, &mut std::io::stdout())?;

  println!("\n\nDone.");

  Ok(())
}
