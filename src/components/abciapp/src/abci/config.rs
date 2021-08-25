use global_cfg::CFG;
use ruc::*;
use serde::Deserialize;
use std::{convert::TryFrom, fs, path::Path};

#[derive(Debug)]
pub struct ABCIConfig {
    pub tendermint_host: String,
    pub tendermint_port: u16,
    pub submission_port: u16,
    pub ledger_port: u16,
    pub query_port: u16,
    pub evm_api_port: u16,
}

#[derive(Deserialize)]
pub struct ABCIConfigStr {
    pub tendermint_host: String,
    pub tendermint_port: String,
    pub submission_port: String,
    pub ledger_port: String,
    pub evm_api_port: String,
}

impl TryFrom<ABCIConfigStr> for ABCIConfig {
    type Error = Box<dyn RucError>;
    fn try_from(cfg: ABCIConfigStr) -> Result<Self> {
        let ledger_port = cfg.ledger_port.parse::<u16>().c(d!())?;
        let query_port = ledger_port - 1;
        let evm_api_port = cfg.evm_api_port.parse::<u16>().c(d!())?;
        Ok(ABCIConfig {
            tendermint_host: cfg.tendermint_host,
            tendermint_port: cfg.tendermint_port.parse::<u16>().c(d!())?,
            submission_port: cfg.submission_port.parse::<u16>().c(d!())?,
            ledger_port,
            query_port,
            evm_api_port,
        })
    }
}

impl ABCIConfig {
    pub fn from_env() -> Result<ABCIConfig> {
        let tendermint_host = CFG.tendermint_host.to_owned();
        let tendermint_port = CFG.tendermint_port;

        // client ------> abci(host, port, for submission)
        let submission_port = CFG.submission_service_port;

        // client ------> abci(host, port, for ledger access)
        let ledger_port = CFG.ledger_service_port;

        let query_port = ledger_port - 1;

        let evm_api_port = CFG.evm_api_port;

        Ok(ABCIConfig {
            tendermint_host,
            tendermint_port,
            submission_port,
            ledger_port,
            query_port,
            evm_api_port,
        })
    }

    pub fn from_file() -> Result<ABCIConfig> {
        let config_path = Path::new(&CFG.ledger_dir.c(d!())?).join("abci.toml");
        let file_contents = fs::read_to_string(config_path).c(d!())?;
        let toml_string = toml::from_str::<ABCIConfigStr>(&file_contents).c(d!())?;
        let config = ABCIConfig::try_from(toml_string).c(d!())?;
        Ok(config)
    }
}

pub(crate) mod global_cfg {
    use clap::{crate_authors, App, Arg, ArgGroup, ArgMatches, SubCommand};
    use lazy_static::lazy_static;
    use ruc::*;
    use std::env;

    use crate::abci::init::InitMode;

    lazy_static! {
        /// Global config.
        pub static ref CFG: Config = pnk!(get_config());

        static ref ABCI_HOST: Option<String> = env::var("ABCI_HOST").ok();
        static ref ABCI_PORT: Option<String> = env::var("ABCI_PORT").ok();
        static ref TENDERMINT_HOST: Option<String> = env::var("TENDERMINT_HOST").ok();
        static ref TENDERMINT_PORT: Option<String> = env::var("TENDERMINT_PORT").ok();
        static ref SERVER_HOST: Option<String> = env::var("SERVER_HOST").ok();
        static ref SUBMISSION_PORT: Option<String> = env::var("SUBMISSION_PORT").ok();
        static ref LEDGER_PORT: Option<String> = env::var("LEDGER_PORT").ok();
        static ref LEDGER_DIR: Option<String> = env::var("LEDGER_DIR").ok();
        static ref ENABLE_LEDGER_SERVICE: Option<String> = env::var("ENABLE_LEDGER_SERVICE").ok();
        static ref ENABLE_QUERY_SERVICE: Option<String> = env::var("ENABLE_QUERY_SERVICE").ok();
        static ref TD_NODE_SELF_ADDR: Option<String> = env::var("TD_NODE_SELF_ADDR").ok();
        static ref TENDERMINT_NODE_KEY_CONFIG_PATH: Option<String> = env::var("TENDERMINT_NODE_KEY_CONFIG_PATH").ok();
        static ref ENABLE_ETH_EMPTY_BLOCKS: Option<String> = env::var("ENABLE_ETH_EMPTY_BLOCKS").ok();
        static ref ENABLE_ETH_API_SERVICE: Option<String> = env::var("ENABLE_ETH_API_SERVICE").ok();
        static ref EVM_API_PORT: Option<String> = env::var("EVM_API_PORT").ok();

        static ref TENDERMINT_HOME: Option<String> = env::var("TENDERMINT_HOME").ok();
        static ref TENDERMINT_CONFIG: Option<String> = env::var("TENDERMINT_CONFIG").ok();
        static ref COMMAND: Option<String> = None;


        static ref M: ArgMatches<'static> = {
            let node = SubCommand::with_name("node")
                .about("Start findora node.")
                .arg_from_usage("-c, --config=[FILE] 'Path to $TMHOM/config/config.toml'")
                .arg_from_usage("-H, --tendermint-host=[Tendermint Node IP]")
                .arg_from_usage("-P, --tendermint-port=[Tendermint Node Port]")
                .arg_from_usage("--submission-service-port=[Submission Service Port]")
                .arg_from_usage("--ledger-service-port=[Ledger Service Port]")
                .arg_from_usage("-l, --enable-ledger-service")
                .arg_from_usage("-q, --enable-query-service")
                .arg_from_usage("--enable-eth-empty-blocks 'whether to generate empty ethereum blocks when there is no ethereum transaction'")
                .arg_from_usage("--enable-eth-api-service")
                .arg_from_usage("--evm-api-port=[EVM Web3 Provider Port]")
                .arg_from_usage("--tendermint-node-self-addr=[Address] 'the address of your tendermint node, in upper-hex format'")
                .arg_from_usage("--tendermint-node-key-config-path=[Path] 'such as: ${HOME}/.tendermint/config/priv_validator_key.json'")
                .arg_from_usage("-d, --ledger-dir=[Path]")
                .arg_from_usage(
                    "-b, --base-dir=[DIR] 'Base directory for tendermint config, aka $TMHOME'",
                );

            let init = SubCommand::with_name("init")
                .about("Init findora node config file and tendermint config file")
                .arg_from_usage("--dev-net 'Inital findora development net configuration.'")
                .arg_from_usage("--test-net 'Inital findora testnet configuration.'")
                .arg_from_usage("--main-net 'Inital findora mainnet configuration.'")
                .arg_from_usage("--qa01-net 'Inital findora qa01 configuration.'")
                .group(ArgGroup::with_name("environment").args(&["dev-net", "test-net", "main-net", "qa01-net"]))
                .arg_from_usage(
                    "-b, --base-dir=[DIR] 'Base directory for tendermint config, aka $TMHOME'",
                );

            App::new("findorad")
                .version(env!("VERGEN_SHA"))
                .author(crate_authors!())
                .about("An ABCI node implementation of FindoraNetwork.")
                .subcommand(node)
                .subcommand(init)
                .arg(Arg::with_name("_a").long("ignored").hidden(true))
                .arg(Arg::with_name("_b").long("nocapture").hidden(true))
                .arg(Arg::with_name("_c").long("test-threads").hidden(true))
                .arg(Arg::with_name("INPUT").multiple(true).hidden(true))
                .get_matches()
        };
    }

    #[derive(Default)]
    pub struct Config {
        pub tendermint_host: &'static str,
        pub tendermint_port: u16,
        pub submission_service_port: u16,
        pub ledger_service_port: u16,
        pub enable_ledger_service: bool,
        pub enable_query_service: bool,
        pub enable_eth_empty_blocks: bool,
        pub enable_eth_api_service: bool,
        pub evm_api_port: u16,
        pub tendermint_node_self_addr: Option<&'static str>,
        pub tendermint_node_key_config_path: Option<&'static str>,
        pub ledger_dir: Option<&'static str>,
        pub tendermint_home: Option<&'static str>,
        pub tendermint_config: Option<&'static str>,
        pub command: &'static str,
        pub init_mode: Option<InitMode>,
    }

    #[cfg(test)]
    fn get_config() -> Result<Config> {
        Ok(Config::default())
    }

    #[cfg(not(test))]
    fn get_config() -> Result<Config> {
        let (command, matches) = M.subcommand();

        if matches.is_none() {
            print_version();
            return Err(eg!("no this command"));
        }

        let m = if let Some(m) = matches {
            m
        } else {
            print_version();
            return Err(eg!("no this command"));
        };

        let th = m
            .value_of("tendermint-host")
            .or_else(|| TENDERMINT_HOST.as_deref())
            .unwrap_or("localhost");
        let tp = m
            .value_of("tendermint-port")
            .or_else(|| TENDERMINT_PORT.as_deref())
            .unwrap_or("26657")
            .parse::<u16>()
            .c(d!())?;
        let ssp = m
            .value_of("submission-service-port")
            .or_else(|| SUBMISSION_PORT.as_deref())
            .unwrap_or("8669")
            .parse::<u16>()
            .c(d!())?;
        let lsp = m
            .value_of("ledger-service-port")
            .or_else(|| LEDGER_PORT.as_deref())
            .unwrap_or("8668")
            .parse::<u16>()
            .c(d!())?;
        let els =
            m.is_present("enable-ledger-service") || ENABLE_LEDGER_SERVICE.is_some();
        let eqs = m.is_present("enable-query-service") || ENABLE_QUERY_SERVICE.is_some();
        let tnsa = m
            .value_of("tendermint-node-self-addr")
            .or_else(|| TD_NODE_SELF_ADDR.as_deref());
        let tnkcp = m
            .value_of("tendermint-node-key-config-path")
            .or_else(|| TENDERMINT_NODE_KEY_CONFIG_PATH.as_deref());
        let ld = m.value_of("ledger-dir").or_else(|| LEDGER_DIR.as_deref());

        let tendermint_config = m
            .value_of("config")
            .or_else(|| TENDERMINT_CONFIG.as_deref());

        let tendermint_home = m
            .value_of("base-dir")
            .or_else(|| TENDERMINT_HOME.as_deref());

        let eeb =
            M.is_present("enable-eth-empty-blocks") || ENABLE_ETH_EMPTY_BLOCKS.is_some();
        let eas =
            M.is_present("enable-eth-api-service") || ENABLE_ETH_API_SERVICE.is_some();
        let eap = M
            .value_of("evm-api-port")
            .or_else(|| EVM_API_PORT.as_deref())
            .unwrap_or("8545")
            .parse::<u16>()
            .c(d!())?;

        let init_mode = if m.is_present("dev-net") {
            InitMode::Dev
        } else if m.is_present("test-net") {
            InitMode::Testnet
        } else if m.is_present("main-net") {
            InitMode::Mainnet
        } else if m.is_present("qa01-net") {
            InitMode::Qa01
        } else {
            InitMode::Dev
        };

        let res = Config {
            tendermint_host: th,
            tendermint_port: tp,
            submission_service_port: ssp,
            ledger_service_port: lsp,
            enable_ledger_service: els,
            enable_query_service: eqs,
            enable_eth_empty_blocks: eeb,
            enable_eth_api_service: eas,
            evm_api_port: eap,
            tendermint_node_self_addr: tnsa,
            tendermint_node_key_config_path: tnkcp,
            ledger_dir: ld,
            command,
            tendermint_config,
            tendermint_home,
            init_mode: Some(init_mode),
        };
        Ok(res)
    }

    #[cfg(not(test))]
    fn print_version() {
        if M.is_present("version") {
            println!("{}", env!("VERGEN_SHA"));
            std::process::exit(0);
        }
    }
}
