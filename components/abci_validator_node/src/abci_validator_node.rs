#![deny(warnings)]
use abci::*;
use ledger::data_model::errors::PlatformError;
use ledger::data_model::Transaction;
use ledger::store::*;
use ledger_api_service::RestfulApiService;
use log::info;
use std::path::Path;
use rand_chacha::ChaChaRng;
use rand_core::SeedableRng;
use std::net::SocketAddr;
use std::process::Command;
use std::sync::{Arc, RwLock};
use std::thread;
use submission_api::SubmissionApi;
use submission_server::{convert_tx, SubmissionServer, TxnForward};
use utils::HashOf;

#[derive(Default)]
pub struct TendermintForward;

impl TxnForward for TendermintForward {
  fn forward_txn(&self, txn: Transaction) -> Result<(), PlatformError> {
    let txn_json = serde_json::to_string(&txn)?;
    info!("forward_txn: {}", txn_json);
    // TODO (Nathan/John): send txn_json to tendermint
    let arg = format!("http://localhost:26657/broadcast_tx_commit?tx=\"{}\"",
                      txn_json);
    let result = Command::new("curl").args(&[arg]).output()?;
    info!("stdout output: {:?}", result.stdout);
    info!("stderr output: {:?}", result.stderr);
    Ok(())
  }
}

struct ABCISubmissionServer {
  la: Arc<RwLock<SubmissionServer<ChaChaRng, LedgerState, TendermintForward>>>,
}

impl ABCISubmissionServer {
  fn new(base_dir: Option<&Path>) -> Result<ABCISubmissionServer, PlatformError> {
  let ledger_state = match base_dir {
    None => LedgerState::test_ledger(),
    Some(base_dir) => LedgerState::load_or_init(base_dir).unwrap(),
  };
    let prng = rand_chacha::ChaChaRng::from_entropy();
    Ok(ABCISubmissionServer { la:
                                Arc::new(RwLock::new(SubmissionServer::new_no_auto_commit(prng,
                                                                     Arc::new(RwLock::new(ledger_state)),
                                                                     Some(TendermintForward::default()))?)) })
  }
}

// TODO: implement abci hooks
impl abci::Application for ABCISubmissionServer {
  fn check_tx(&mut self, req: &RequestCheckTx) -> ResponseCheckTx {
    // Get the Tx [u8] and convert to u64
    let mut resp = ResponseCheckTx::new();

    if let Some(tx) = convert_tx(req.get_tx()) {
      if let Ok(la) = self.la.read() {
        if la.get_committed_state().write().is_ok() {
        if TxnEffect::compute_effect(tx).is_err() {
          resp.set_code(1);
          resp.set_log(String::from("Check failed"));
        }
      }
      } else {
        resp.set_code(1);
        resp.set_log(String::from("Could not access ledger"));
      }
    } else {
      resp.set_code(1);
      resp.set_log(String::from("Could not unpack transaction"));
    }

    resp
  }

  fn deliver_tx(&mut self, req: &RequestDeliverTx) -> ResponseDeliverTx {
    // Get the Tx [u8]
    let mut resp = ResponseDeliverTx::new();
    if let Some(tx) = convert_tx(req.get_tx()) {
      if let Ok(mut la) = self.la.write() { if la.cache_transaction(tx).is_ok() {
        return resp;
      }
    }
    }
    resp.set_code(1);
    resp
  }

  fn begin_block(&mut self, _req: &RequestBeginBlock) -> ResponseBeginBlock {
    if let Ok(mut la) = self.la.write() { la.begin_block(); }
    ResponseBeginBlock::new()
  }

  fn end_block(&mut self, _req: &RequestEndBlock) -> ResponseEndBlock {
    // TODO: this should propagate errors instead of panicking
    if let Ok(mut la) = self.la.write() { la.end_block().unwrap(); }
    ResponseEndBlock::new()
  }

  fn commit(&mut self, _req: &RequestCommit) -> ResponseCommit {
    // Tendermint does not accept an error return type here.
    let error_commitment = (HashOf::new(&None), 0);
    let mut r = ResponseCommit::new();
    if let Ok(mut la) = self.la.write() {
    la.begin_commit();
    let commitment = if let Ok(state) = la.get_committed_state().read() {
      state.get_state_commitment()
    } else {
      error_commitment
    };
    la.end_commit();
    r.data = commitment.0.as_ref().to_vec();
  }
    r
  }

  fn query(&mut self, req: &RequestQuery) -> ResponseQuery {
    println!("{:?}", &req);
    let q = &req.data;
    println!("Path = {}, data = {:?}", &req.path, q);
    ResponseQuery::new()
  }
}

fn main() {
  // Tendermint ABCI port
  // let addr = "127.0.0.1:26658".parse().unwrap();

  // abci::run(addr, ABCISubmissionServer::default());
  flexi_logger::Logger::with_env().start().unwrap();
  let base_dir = std::env::var_os("LEDGER_DIR").filter(|x| !x.is_empty());
  let base_dir = base_dir.as_ref().map(Path::new);
  let app = ABCISubmissionServer::new(base_dir).unwrap();
  let submission_server = Arc::clone(&app.la);
  let cloned_lock = Arc::clone(&submission_server.read().unwrap().borrowable_ledger_state());

  let host = std::env::var_os("SERVER_HOST").filter(|x| !x.is_empty())
                                            .unwrap_or_else(|| "localhost".into());
  let host2 = host.clone();
  let submission_port = std::env::var_os("SUBMISSION_PORT").filter(|x| !x.is_empty())
                                                           .unwrap_or_else(|| "8669".into());
  let ledger_port = std::env::var_os("LEDGER_PORT").filter(|x| !x.is_empty())
                                                   .unwrap_or_else(|| "8668".into());
  thread::spawn(move || {
    let submission_api = SubmissionApi::create(submission_server,
                                               host.to_str().unwrap(),
                                               submission_port.to_str().unwrap()).unwrap();
    println!("Starting submission service");
    match submission_api.run() {
      Ok(_) => println!("Successfully ran submission service"),
      Err(_) => println!("Error running submission service"),
    }
  });

  let _join = thread::spawn(move || {
    let query_service = RestfulApiService::create(cloned_lock,
                                                 host2.to_str().unwrap(),
                                                 ledger_port.to_str().unwrap()).unwrap();
  println!("Starting ledger service");
  match query_service.run() {
    Ok(_) => println!("Successfully ran standalone"),
    Err(_) => println!("Error running standalone"),
  }
  });

  let abci_host = std::option_env!("ABCI_HOST").unwrap_or("0.0.0.0");
  let abci_port = std::option_env!("ABCI_PORT").unwrap_or("26658");

  // TODO: pass the address and port in on the command line
  let addr_str = format!("{}:{}", abci_host, abci_port);
  let addr: SocketAddr = addr_str.parse().expect("Unable to parse socket address");

  abci::run(addr, app);
}
