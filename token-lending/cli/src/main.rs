use std::fmt::Display;

use solana_clap_utils::input_parsers::pubkeys_of;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_sdk::commitment_config::CommitmentLevel::Finalized;
use solana_sdk::signature::read_keypair_file;

use port_finance_variable_rate_lending::instruction::{
    refresh_obligation, update_oracle, update_reserve,
};
use port_finance_variable_rate_lending::instruction::{
    refresh_reserve, repay_obligation_liquidity,
};
use {
    clap::{
        crate_description, crate_name, crate_version, value_t, App, AppSettings, Arg, ArgMatches,
        SubCommand,
    },
    port_finance_variable_rate_lending::{
        self,
        instruction::{init_lending_market, init_reserve},
        math::{Decimal, WAD},
        state::{LendingMarket, Reserve, ReserveConfig, ReserveFees},
    },
    solana_clap_utils::{
        fee_payer::fee_payer_arg,
        input_parsers::{keypair_of, pubkey_of, value_of},
        input_validators::{is_amount, is_keypair, is_parsable, is_pubkey, is_url},
        keypair::signer_from_path,
    },
    solana_client::rpc_client::RpcClient,
    solana_program::{program_option::COption, program_pack::Pack, pubkey::Pubkey},
    solana_sdk::{
        commitment_config::CommitmentConfig,
        signature::{Keypair, Signer},
        system_instruction,
        transaction::Transaction,
    },
    spl_token::{
        instruction::{approve, revoke},
        state::{Account as Token, Mint},
        ui_amount_to_amount,
    },
    std::{borrow::Borrow, process::exit, str::FromStr},
    system_instruction::create_account,
};

struct Config {
    rpc_client: RpcClient,
    fee_payer: Box<dyn Signer>,
    lending_program_id: Pubkey,
    verbose: bool,
    dry_run: bool,
}

type Error = Box<dyn std::error::Error>;
type CommandResult = Result<(), Error>;

pub fn is_u64<T>(amount: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    if amount.as_ref().parse::<u64>().is_ok() {
        Ok(())
    } else {
        Err(format!(
            "Unable to parse input amount as u64 integer, provided: {}",
            amount
        ))
    }
}

pub fn is_pubkey_or_none<T>(pubkey: T) -> Result<(), String>
where
    T: AsRef<str> + Display + Clone,
{
    if pubkey.as_ref() == "NONE" || is_pubkey(pubkey.clone()).is_ok() {
        Ok(())
    } else {
        Err(format!(
            "Unable to parse input amount as a pubkey or None, provided: {}",
            pubkey
        ))
    }
}

pub fn pubkeys_or_none_of(matches: &ArgMatches<'_>, name: &str) -> Option<Vec<COption<Pubkey>>> {
    matches.values_of(name).map(|values| {
        values
            .map(|value| {
                if value == "NONE" {
                    COption::None
                } else {
                    COption::Some(value.parse::<Pubkey>().unwrap_or_else(|_| {
                        read_keypair_file(value)
                            .expect("read_keypair_file failed")
                            .pubkey()
                    }))
                }
            })
            .collect()
    })
}

pub fn pubkey_or_none_of(matches: &ArgMatches<'_>, name: &str) -> Option<COption<Pubkey>> {
    let value: Option<String> = value_of(matches, name);
    value.map(|v| {
        if v == "NONE" {
            COption::None
        } else {
            COption::Some(v.parse::<Pubkey>().unwrap_or_else(|_| {
                read_keypair_file(v)
                    .expect("read_keypair_file failed")
                    .pubkey()
            }))
        }
    })
}

fn main() {
    solana_logger::setup_with_default("solana=info");

    let default_lending_program_id: &str = &port_finance_variable_rate_lending::id().to_string();

    let build_u64_arg = |name: &'static str| {
        Arg::with_name(name)
            .long(name)
            .validator(is_u64)
            .value_name("U64")
            .takes_value(true)
            .help(name)
    };

    let update_reserve_args: Vec<_> = vec![
        "optimal_utilization_rate",
        "loan_to_value_ratio",
        "liquidation_bonus",
        "liquidation_threshold",
        "min_borrow_rate",
        "optimal_borrow_rate",
        "max_borrow_rate",
        "borrow_fee_wad",
        "flash_loan_fee_wad",
        "host_fee_percentage",
    ]
    .into_iter()
    .map(build_u64_arg)
    .collect();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(crate_version!())
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .arg({
            let arg = Arg::with_name("config_file")
                .short("C")
                .long("config")
                .value_name("PATH")
                .takes_value(true)
                .global(true)
                .help("Configuration file to use");
            if let Some(ref config_file) = *solana_cli_config::CONFIG_FILE {
                arg.default_value(config_file)
            } else {
                arg
            }
        })
        .arg(
            Arg::with_name("json_rpc_url")
                .long("url")
                .value_name("URL")
                .takes_value(true)
                .validator(is_url)
                .help("JSON RPC URL for the cluster.  Default from the configuration file."),
        )
        .arg(
            fee_payer_arg()
                .short("p")
                .global(true)
        )
        .arg(
            Arg::with_name("lending_program_id")
                .long("program")
                .validator(is_pubkey)
                .value_name("PUBKEY")
                .takes_value(true)
                .required(true)
                .default_value(default_lending_program_id)
                .help("Lending program ID"),
        )
        .arg(
            Arg::with_name("verbose")
                .long("verbose")
                .short("v")
                .takes_value(false)
                .global(true)
                .help("Show additional information"),
        )
        .arg(
            Arg::with_name("dry_run")
                .long("dry-run")
                .takes_value(false)
                .global(true)
                .help("Simulate transaction instead of executing"),
        )
        .subcommand(
            SubCommand::with_name("update-reserve")
                .about("Update the config of the reserve")
                .arg(
                    Arg::with_name("reserve")
                        .long("reserve")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Reserve to update")
                )
                .arg(
                    Arg::with_name("lending_market")
                        .long("market")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("lending market")
                )
                .arg(
                    Arg::with_name("lending_market_owner")
                        .long("market-owner")
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .help("Owner of the lending market"),
                )
                .arg(
                    Arg::with_name("deposit_staking_pool")
                        .long("deposit_staking_pool")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .help("deposit staking pool")
                )
                .args(&update_reserve_args)
        )
        .subcommand(
            SubCommand::with_name("update-oracle")
                .about("Update the oracle of the reserve")
                .arg(
                    Arg::with_name("reserve")
                        .long("reserve")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Reserve to update")
                )
                .arg(
                    Arg::with_name("lending_market")
                        .long("market")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("lending market")
                )
                .arg(
                    Arg::with_name("lending_market_owner")
                        .long("market-owner")
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .help("Owner of the lending market"),
                )
                .arg(
                    Arg::with_name("oracle")
                        .long("oracle")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .help("price oracle")
                )
        )
        .subcommand(
            SubCommand::with_name("repay-loan")
                .about("repay loan to a specific obligation")
                .arg(
                    Arg::with_name("amount_to_repay")
                        .long("amount")
                        .validator(is_u64)
                        .value_name("U64")
                        .takes_value(true)
                        .required(true)
                        .help("Amount to repay")
                ).arg(Arg::with_name
                ("token_account_to_repay")
                .long("source-token")
                .validator(is_pubkey)
                .value_name("PUBKEY")
                .takes_value(true)
                .required(true)
                .help("Source token account to repay the loan")
            )
                .arg(
                    Arg::with_name("wallet_to_repay")
                        .long("source-wallet")
                        .validator(is_keypair)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .help("keypair of the wallet to repay the loan")
                )
                .arg(
                    Arg::with_name("destination_token_account")
                        .long("dest-token")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Pubkey of the wallet to repay token to")
                )
                .arg(
                    Arg::with_name("repay_reserve")
                        .long("repay-reserve")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Reserve to repay to")
                )
                .arg(
                    Arg::with_name("all_reserves")
                        .long("reserve")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .multiple(true)
                        .help("All reserves to refresh should be in same order as oracles")
                )
                .arg(
                    Arg::with_name("all_oracles")
                        .long("oracle")
                        .validator(is_pubkey_or_none)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .multiple(true)
                        .help("All oracle associated with reserves should be in same order as reserves")
                )
                .arg(
                    Arg::with_name("repay_obligation")
                        .long("obligation")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Obligation to repay")
                )
                .arg(
                    Arg::with_name("lending_market")
                        .long("market")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Lending market repays to")
                )
        )
        .subcommand(
            SubCommand::with_name("create-market")
                .about("Create a new lending market")
                .arg(
                    Arg::with_name("lending_market_owner")
                        .long("market-owner")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Owner that can add reserves to the market"),
                )
                .arg(
                    Arg::with_name("quote_currency")
                        .long("quote")
                        .value_name("STRING")
                        .takes_value(true)
                        .required(true)
                        .default_value("USD")
                        .help("Currency market prices are quoted in"),
                ),
        )
        .subcommand(
            SubCommand::with_name("add-reserve")
                .about("Add a reserve to a lending market")
                // @TODO: use is_valid_signer
                .arg(
                    Arg::with_name("lending_market_owner")
                        .long("market-owner")
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .help("Owner of the lending market"),
                )
                // @TODO: use is_valid_signer
                .arg(
                    Arg::with_name("source_liquidity_owner")
                        .long("source-owner")
                        .validator(is_keypair)
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .help("Owner of the SPL Token account to deposit initial liquidity from"),
                )
                .arg(
                    Arg::with_name("lending_market")
                        .long("market")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Lending market address"),
                )
                .arg(
                    Arg::with_name("source_liquidity")
                        .long("source")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("SPL Token account to deposit initial liquidity from"),
                )
                // @TODO: use is_amount_or_all
                .arg(
                    Arg::with_name("liquidity_amount")
                        .long("amount")
                        .validator(is_amount)
                        .value_name("DECIMAL_AMOUNT")
                        .takes_value(true)
                        .required(true)
                        .help("Initial amount of liquidity to deposit into the new reserve"),
                )
                .arg(
                    Arg::with_name("fixed_price")
                        .long("fixed-price")
                        .validator(is_amount)
                        .value_name("DECIMAL_AMOUNT")
                        .takes_value(true)
                        .help("Initial price for the given asset"),
                )
                .arg(
                    Arg::with_name("pyth_price")
                        .long("pyth-price")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(false)
                        .help("Pyth price account: https://pyth.network/developers/consumers/accounts"),
                )
                .arg(
                    Arg::with_name("optimal_utilization_rate")
                        .long("optimal-utilization-rate")
                        .validator(is_parsable::<u8>)
                        .value_name("INTEGER_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("80")
                        .help("Optimal utilization rate: [0, 100]"),
                )
                .arg(
                    Arg::with_name("loan_to_value_ratio")
                        .long("loan-to-value-ratio")
                        .validator(is_parsable::<u8>)
                        .value_name("INTEGER_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("50")
                        .help("Target ratio of the value of borrows to deposits: [0, 100)"),
                )
                .arg(
                    Arg::with_name("liquidation_bonus")
                        .long("liquidation-bonus")
                        .validator(is_parsable::<u8>)
                        .value_name("INTEGER_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("5")
                        .help("Bonus a liquidator gets when repaying part of an unhealthy obligation: [0, 100]"),
                )
                .arg(
                    Arg::with_name("liquidation_threshold")
                        .long("liquidation-threshold")
                        .validator(is_parsable::<u8>)
                        .value_name("INTEGER_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("55")
                        .help("Loan to value ratio at which an obligation can be liquidated: (LTV, 100]"),
                )
                .arg(
                    Arg::with_name("min_borrow_rate")
                        .long("min-borrow-rate")
                        .validator(is_parsable::<u8>)
                        .value_name("INTEGER_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("0")
                        .help("Min borrow APY: min <= optimal <= max"),
                )
                .arg(
                    Arg::with_name("optimal_borrow_rate")
                        .long("optimal-borrow-rate")
                        .validator(is_parsable::<u8>)
                        .value_name("INTEGER_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("4")
                        .help("Optimal (utilization) borrow APY: min <= optimal <= max"),
                )
                .arg(
                    Arg::with_name("max_borrow_rate")
                        .long("max-borrow-rate")
                        .validator(is_parsable::<u8>)
                        .value_name("INTEGER_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("30")
                        .help("Max borrow APY: min <= optimal <= max"),
                )
                .arg(
                    Arg::with_name("borrow_fee")
                        .long("borrow-fee")
                        .validator(is_parsable::<f64>)
                        .value_name("DECIMAL_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("0.0001")
                        .help("Fee assessed on borrow, expressed as a percentage: [0, 1)"),
                )
                .arg(
                    Arg::with_name("flash_loan_fee")
                        .long("flash-loan-fee")
                        .validator(is_parsable::<f64>)
                        .value_name("DECIMAL_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("0.0009")
                        .help("Fee assessed for flash loans, expressed as a percentage: [0, 1)"),
                )
                .arg(
                    Arg::with_name("host_fee_percentage")
                        .long("host-fee-percentage")
                        .validator(is_parsable::<u8>)
                        .value_name("INTEGER_PERCENT")
                        .takes_value(true)
                        .required(true)
                        .default_value("20")
                        .help("Amount of fee going to host account: [0, 100]"),
                )
        )
        .get_matches();

    let mut wallet_manager = None;
    let config = {
        let cli_config = if let Some(config_file) = matches.value_of("config_file") {
            solana_cli_config::Config::load(config_file).unwrap_or_default()
        } else {
            solana_cli_config::Config::default()
        };
        let json_rpc_url = value_t!(matches, "json_rpc_url", String)
            .unwrap_or_else(|_| cli_config.json_rpc_url.clone());

        let fee_payer = signer_from_path(
            &matches,
            matches
                .value_of("fee_payer")
                .unwrap_or(&cli_config.keypair_path),
            "fee_payer",
            &mut wallet_manager,
        )
        .unwrap_or_else(|e| {
            eprintln!("error: {}", e);
            exit(1);
        });

        let lending_program_id = pubkey_of(&matches, "lending_program_id").unwrap();
        let verbose = matches.is_present("verbose");
        let dry_run = matches.is_present("dry_run");

        Config {
            rpc_client: RpcClient::new_with_commitment(json_rpc_url, CommitmentConfig::confirmed()),
            fee_payer,
            lending_program_id,
            verbose,
            dry_run,
        }
    };

    let _ = match matches.subcommand() {
        ("create-market", Some(arg_matches)) => {
            let lending_market_owner = pubkey_of(arg_matches, "lending_market_owner").unwrap();
            let quote_currency = quote_currency_of(arg_matches, "quote_currency").unwrap();
            command_create_lending_market(&config, lending_market_owner, quote_currency)
        }
        ("update-reserve", Some(arg_matches)) => {
            let reserve = pubkey_of(arg_matches, "reserve").unwrap();
            let lending_market = pubkey_of(arg_matches, "lending_market").unwrap();
            let mut wallet_manager = None;
            let lending_market_owner = signer_from_path(
                arg_matches,
                arg_matches.value_of("lending_market_owner").unwrap(),
                "lending_market_owner",
                &mut wallet_manager,
            )
            .unwrap();
            let optimal_utilization_rate = value_of(arg_matches, "optimal_utilization_rate");
            let loan_to_value_ratio = value_of(arg_matches, "loan_to_value_ratio");
            let liquidation_bonus = value_of(arg_matches, "liquidation_bonus");
            let liquidation_threshold = value_of(arg_matches, "liquidation_threshold");
            let min_borrow_rate = value_of(arg_matches, "min_borrow_rate");
            let optimal_borrow_rate = value_of(arg_matches, "optimal_borrow_rate");
            let max_borrow_rate = value_of(arg_matches, "max_borrow_rate");
            let borrow_fee_wad = value_of(arg_matches, "borrow_fee_wad");
            let flash_loan_fee_wad = value_of(arg_matches, "flash_loan_fee_wad");
            let host_fee_percentage = value_of(arg_matches, "host_fee_percentage");
            let deposit_staking_pool = pubkey_or_none_of(arg_matches, "deposit_staking_pool");
            let mut old_config =
                Reserve::unpack(&config.rpc_client.get_account(&reserve).unwrap().data)
                    .unwrap()
                    .config;

            old_config.optimal_utilization_rate =
                optimal_utilization_rate.unwrap_or(old_config.optimal_utilization_rate);
            old_config.loan_to_value_ratio =
                loan_to_value_ratio.unwrap_or(old_config.loan_to_value_ratio);
            old_config.liquidation_bonus =
                liquidation_bonus.unwrap_or(old_config.liquidation_bonus);
            old_config.liquidation_threshold =
                liquidation_threshold.unwrap_or(old_config.liquidation_threshold);
            old_config.min_borrow_rate = min_borrow_rate.unwrap_or(old_config.min_borrow_rate);
            old_config.max_borrow_rate = max_borrow_rate.unwrap_or(old_config.max_borrow_rate);
            old_config.optimal_borrow_rate =
                optimal_borrow_rate.unwrap_or(old_config.optimal_borrow_rate);
            old_config.fees.borrow_fee_wad =
                borrow_fee_wad.unwrap_or(old_config.fees.borrow_fee_wad);
            old_config.fees.host_fee_percentage =
                host_fee_percentage.unwrap_or(old_config.fees.host_fee_percentage);
            old_config.fees.flash_loan_fee_wad =
                flash_loan_fee_wad.unwrap_or(old_config.fees.flash_loan_fee_wad);
            old_config.deposit_staking_pool =
                deposit_staking_pool.unwrap_or(old_config.deposit_staking_pool);
            command_update_reserve(
                &config,
                reserve,
                lending_market,
                lending_market_owner,
                old_config,
            )
        }
        ("update-oracle", Some(arg_matches)) => {
            let reserve = pubkey_of(arg_matches, "reserve").unwrap();
            let lending_market = pubkey_of(arg_matches, "lending_market").unwrap();
            let mut wallet_manager = None;
            let lending_market_owner = signer_from_path(
                arg_matches,
                arg_matches.value_of("lending_market_owner").unwrap(),
                "lending_market_owner",
                &mut wallet_manager,
            )
            .unwrap();
            let oracle = pubkey_of(arg_matches, "oracle");
            command_update_oracle(
                &config,
                reserve,
                lending_market,
                lending_market_owner,
                oracle,
            )
        }
        ("add-reserve", Some(arg_matches)) => {
            let mut wallet_manager = None;
            let lending_market_owner = signer_from_path(
                arg_matches,
                arg_matches.value_of("lending_market_owner").unwrap(),
                "lending_market_owner",
                &mut wallet_manager,
            )
            .unwrap();
            let source_liquidity_owner_keypair =
                keypair_of(arg_matches, "source_liquidity_owner").unwrap();
            let lending_market_pubkey = pubkey_of(arg_matches, "lending_market").unwrap();
            let source_liquidity_pubkey = pubkey_of(arg_matches, "source_liquidity").unwrap();
            let ui_amount = value_of(arg_matches, "liquidity_amount").unwrap();
            let fixed_price = if arg_matches.is_present("fixed_price") {
                let price: u64 = value_of(arg_matches, "fixed_price").unwrap();
                COption::Some(Decimal::from(price))
            } else {
                COption::None
            };
            let pyth_price_pubkey = if arg_matches.is_present("pyth_price") {
                COption::Some(pubkey_of(arg_matches, "pyth_price").unwrap())
            } else {
                COption::None
            };
            let optimal_utilization_rate =
                value_of(arg_matches, "optimal_utilization_rate").unwrap();
            let loan_to_value_ratio = value_of(arg_matches, "loan_to_value_ratio").unwrap();
            let liquidation_bonus = value_of(arg_matches, "liquidation_bonus").unwrap();
            let liquidation_threshold = value_of(arg_matches, "liquidation_threshold").unwrap();
            let min_borrow_rate = value_of(arg_matches, "min_borrow_rate").unwrap();
            let optimal_borrow_rate = value_of(arg_matches, "optimal_borrow_rate").unwrap();
            let max_borrow_rate = value_of(arg_matches, "max_borrow_rate").unwrap();
            let borrow_fee = value_of::<f64>(arg_matches, "borrow_fee").unwrap();
            let flash_loan_fee = value_of::<f64>(arg_matches, "flash_loan_fee").unwrap();
            let host_fee_percentage = value_of(arg_matches, "host_fee_percentage").unwrap();

            let borrow_fee_wad = (borrow_fee * WAD as f64) as u64;
            let flash_loan_fee_wad = (flash_loan_fee * WAD as f64) as u64;

            if fixed_price.is_none() && pyth_price_pubkey.is_none() {
                eprintln!("Supply at least one of `fixed_price` or `pyth_price_pubkey`");
                exit(1);
            }

            if fixed_price.is_some() && pyth_price_pubkey.is_some() {
                eprintln!("Supply both `fixed_price` and `pyth_price_pubkey`");
                exit(1);
            }
            command_add_reserve(
                &config,
                ui_amount,
                fixed_price,
                ReserveConfig {
                    optimal_utilization_rate,
                    loan_to_value_ratio,
                    liquidation_bonus,
                    liquidation_threshold,
                    min_borrow_rate,
                    optimal_borrow_rate,
                    max_borrow_rate,
                    fees: ReserveFees {
                        borrow_fee_wad,
                        flash_loan_fee_wad,
                        host_fee_percentage,
                    },
                    deposit_staking_pool: COption::None,
                },
                source_liquidity_pubkey,
                source_liquidity_owner_keypair,
                lending_market_pubkey,
                lending_market_owner,
                pyth_price_pubkey,
            )
        }
        ("repay-loan", Some(arg_matches)) => {
            let amount: u64 = value_of(arg_matches, "amount_to_repay").unwrap();
            let source_wallet = keypair_of(arg_matches, "wallet_to_repay").unwrap();
            let source_token = pubkey_of(arg_matches, "token_account_to_repay").unwrap();
            let dest_token = pubkey_of(arg_matches, "destination_token_account").unwrap();
            let repay_reserve = pubkey_of(arg_matches, "repay_reserve").unwrap();
            let repay_obligation = pubkey_of(arg_matches, "repay_obligation").unwrap();
            let lending_market = pubkey_of(arg_matches, "lending_market").unwrap();
            let reserves = pubkeys_of(arg_matches, "all_reserves").unwrap();
            let oracles = pubkeys_or_none_of(arg_matches, "all_oracles").unwrap();
            if reserves.len() != oracles.len() {
                eprintln!(
                    "Number of reserves should equal with the number of oracles, {} != {}",
                    reserves.len(),
                    oracles.len()
                );
                exit(1);
            }
            command_repay_loan(
                &config,
                amount,
                source_token,
                source_wallet,
                dest_token,
                repay_reserve,
                repay_obligation,
                reserves.into_iter().zip(oracles).collect(),
                lending_market,
            )
        }
        _ => unreachable!(),
    }
    .map_err(|err| {
        eprintln!("{}", err);
        exit(1);
    });
}

// COMMANDS
fn command_update_reserve(
    config: &Config,
    reserve: Pubkey,
    lending_market: Pubkey,
    lending_market_owner: Box<dyn Signer>,
    reserve_config: ReserveConfig,
) -> CommandResult {
    println!(
        "update reserve {} with the config {:?}",
        reserve, reserve_config
    );
    let mut transaction = Transaction::new_with_payer(
        &[update_reserve(
            config.lending_program_id,
            reserve_config,
            reserve,
            lending_market,
            lending_market_owner.pubkey(),
        )],
        Some(&config.fee_payer.pubkey()),
    );
    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;
    transaction.sign(
        &vec![config.fee_payer.as_ref(), lending_market_owner.as_ref()],
        recent_blockhash,
    );
    send_transaction(config, transaction)?;
    Ok(())
}

fn command_update_oracle(
    config: &Config,
    reserve: Pubkey,
    lending_market: Pubkey,
    lending_market_owner: Box<dyn Signer>,
    oracle: Option<Pubkey>,
) -> CommandResult {
    println!("update reserve {} with the oracle {:?}", reserve, oracle);
    let mut transaction = Transaction::new_with_payer(
        &[update_oracle(
            config.lending_program_id,
            oracle,
            reserve,
            lending_market,
            lending_market_owner.pubkey(),
        )],
        Some(&config.fee_payer.pubkey()),
    );
    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;
    transaction.sign(
        &vec![config.fee_payer.as_ref(), lending_market_owner.as_ref()],
        recent_blockhash,
    );
    send_transaction(config, transaction)?;
    Ok(())
}

fn command_create_lending_market(
    config: &Config,
    lending_market_owner: Pubkey,
    quote_currency: [u8; 32],
) -> CommandResult {
    let lending_market_keypair = Keypair::new();
    println!(
        "Creating lending market {}",
        lending_market_keypair.pubkey()
    );

    let lending_market_balance = config
        .rpc_client
        .get_minimum_balance_for_rent_exemption(LendingMarket::LEN)?;

    let mut transaction = Transaction::new_with_payer(
        &[
            // Account for the lending market
            create_account(
                &config.fee_payer.pubkey(),
                &lending_market_keypair.pubkey(),
                lending_market_balance,
                LendingMarket::LEN as u64,
                &config.lending_program_id,
            ),
            // Initialize lending market account
            init_lending_market(
                config.lending_program_id,
                lending_market_owner,
                quote_currency,
                lending_market_keypair.pubkey(),
            ),
        ],
        Some(&config.fee_payer.pubkey()),
    );

    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;
    transaction.sign(
        &vec![config.fee_payer.as_ref(), &lending_market_keypair],
        recent_blockhash,
    );
    send_transaction(config, transaction)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn command_add_reserve(
    config: &Config,
    ui_amount: f64,
    fixed_price: COption<Decimal>,
    reserve_config: ReserveConfig,
    source_liquidity_pubkey: Pubkey,
    source_liquidity_owner_keypair: Keypair,
    lending_market_pubkey: Pubkey,
    lending_market_owner_keypair: Box<dyn Signer>,
    pyth_price_pubkey: COption<Pubkey>,
) -> CommandResult {
    let source_liquidity_account = config.rpc_client.get_account(&source_liquidity_pubkey)?;
    let source_liquidity = Token::unpack_from_slice(source_liquidity_account.data.borrow())?;

    let source_liquidity_mint_account = config.rpc_client.get_account(&source_liquidity.mint)?;
    let source_liquidity_mint =
        Mint::unpack_from_slice(source_liquidity_mint_account.data.borrow())?;
    let liquidity_amount = ui_amount_to_amount(ui_amount, source_liquidity_mint.decimals);

    let reserve_keypair = Keypair::new();
    let collateral_mint_keypair = Keypair::new();
    let collateral_supply_keypair = Keypair::new();
    let liquidity_supply_keypair = Keypair::new();
    let liquidity_fee_receiver_keypair = Keypair::new();
    let user_collateral_keypair = Keypair::new();
    let user_transfer_authority_keypair = Keypair::new();

    if config.verbose {
        println!(
            "lending market owner: {}",
            lending_market_owner_keypair.try_pubkey()?
        );
        println!(
            "Adding reserve {}, with config: {} {} with oracle: {}",
            reserve_keypair.pubkey(),
            reserve_config.liquidation_threshold,
            reserve_config.loan_to_value_ratio,
            pyth_price_pubkey.is_some()
        );

        println!(
            "Adding collateral mint {}",
            collateral_mint_keypair.pubkey()
        );
        println!(
            "Adding collateral supply {}",
            collateral_supply_keypair.pubkey()
        );
        println!(
            "Adding liquidity supply {}",
            liquidity_supply_keypair.pubkey()
        );
        println!(
            "Adding liquidity fee receiver {}",
            liquidity_fee_receiver_keypair.pubkey()
        );
        println!(
            "Adding user collateral {}",
            user_collateral_keypair.pubkey()
        );
        println!(
            "Adding user transfer authority {}",
            user_transfer_authority_keypair.pubkey()
        );
    }

    let reserve_balance = config
        .rpc_client
        .get_minimum_balance_for_rent_exemption(Reserve::LEN)?;
    let collateral_mint_balance = config
        .rpc_client
        .get_minimum_balance_for_rent_exemption(Mint::LEN)?;
    let token_account_balance = config
        .rpc_client
        .get_minimum_balance_for_rent_exemption(Token::LEN)?;
    let collateral_supply_balance = token_account_balance;
    let user_collateral_balance = token_account_balance;
    let liquidity_supply_balance = token_account_balance;
    let liquidity_fee_receiver_balance = token_account_balance;

    let mut transaction_1 = Transaction::new_with_payer(
        &[
            create_account(
                &config.fee_payer.pubkey(),
                &reserve_keypair.pubkey(),
                reserve_balance,
                Reserve::LEN as u64,
                &config.lending_program_id,
            ),
            create_account(
                &config.fee_payer.pubkey(),
                &collateral_mint_keypair.pubkey(),
                collateral_mint_balance,
                Mint::LEN as u64,
                &spl_token::id(),
            ),
            create_account(
                &config.fee_payer.pubkey(),
                &collateral_supply_keypair.pubkey(),
                collateral_supply_balance,
                Token::LEN as u64,
                &spl_token::id(),
            ),
            create_account(
                &config.fee_payer.pubkey(),
                &user_collateral_keypair.pubkey(),
                user_collateral_balance,
                Token::LEN as u64,
                &spl_token::id(),
            ),
        ],
        Some(&config.fee_payer.pubkey()),
    );

    let mut transaction_2 = Transaction::new_with_payer(
        &[
            create_account(
                &config.fee_payer.pubkey(),
                &liquidity_supply_keypair.pubkey(),
                liquidity_supply_balance,
                Token::LEN as u64,
                &spl_token::id(),
            ),
            create_account(
                &config.fee_payer.pubkey(),
                &liquidity_fee_receiver_keypair.pubkey(),
                liquidity_fee_receiver_balance,
                Token::LEN as u64,
                &spl_token::id(),
            ),
            approve(
                &spl_token::id(),
                &source_liquidity_pubkey,
                &user_transfer_authority_keypair.pubkey(),
                &source_liquidity_owner_keypair.pubkey(),
                &[],
                liquidity_amount,
            )
            .unwrap(),
            init_reserve(
                config.lending_program_id,
                liquidity_amount,
                fixed_price,
                reserve_config,
                source_liquidity_pubkey,
                user_collateral_keypair.pubkey(),
                reserve_keypair.pubkey(),
                source_liquidity.mint,
                liquidity_supply_keypair.pubkey(),
                liquidity_fee_receiver_keypair.pubkey(),
                collateral_mint_keypair.pubkey(),
                collateral_supply_keypair.pubkey(),
                lending_market_pubkey,
                lending_market_owner_keypair.pubkey(),
                user_transfer_authority_keypair.pubkey(),
                pyth_price_pubkey,
            ),
            revoke(
                &spl_token::id(),
                &source_liquidity_pubkey,
                &source_liquidity_owner_keypair.pubkey(),
                &[],
            )
            .unwrap(),
        ],
        Some(&config.fee_payer.pubkey()),
    );

    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;
    transaction_1.sign(
        &vec![
            config.fee_payer.as_ref(),
            &reserve_keypair,
            &collateral_mint_keypair,
            &collateral_supply_keypair,
            &user_collateral_keypair,
        ],
        recent_blockhash,
    );
    transaction_2.sign(
        &vec![
            config.fee_payer.as_ref(),
            &liquidity_supply_keypair,
            &liquidity_fee_receiver_keypair,
            &source_liquidity_owner_keypair,
            lending_market_owner_keypair.as_ref(),
            &user_transfer_authority_keypair,
        ],
        recent_blockhash,
    );

    println!("Newly added reserve {}", reserve_keypair.pubkey());
    send_transaction(config, transaction_1)?;
    send_transaction(config, transaction_2)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn command_repay_loan(
    config: &Config,
    amount: u64,
    source_token: Pubkey,
    source_wallet: Keypair,
    dest_token: Pubkey,
    repay_reserve: Pubkey,
    repay_obligation: Pubkey,
    all_reserves_with_oracle: Vec<(Pubkey, COption<Pubkey>)>,
    lending_market: Pubkey,
) -> CommandResult {
    println!(
        "Repay Loan {}, from {} to {}",
        amount,
        source_wallet.pubkey(),
        dest_token
    );
    if config.verbose {
        println!(
            "Reserve {}\n\
            Obligation {}\n\
            Lending Market {}\n\
            ",
            repay_reserve, repay_obligation, lending_market
        );
    }
    let source_balance = config
        .rpc_client
        .get_balance(&source_wallet.pubkey())
        .unwrap();

    assert!(
        source_balance > amount,
        "source wallet has {}, not enough, need {}",
        source_balance,
        amount
    );

    let mut instructions: Vec<_> = all_reserves_with_oracle
        .iter()
        .map(|(r, o)| refresh_reserve(config.lending_program_id, *r, *o))
        .collect();
    instructions.push(refresh_obligation(
        config.lending_program_id,
        repay_obligation,
        all_reserves_with_oracle.iter().map(|(r, _)| *r).collect(),
    ));
    instructions.push(repay_obligation_liquidity(
        config.lending_program_id,
        amount,
        source_token,
        dest_token,
        repay_reserve,
        repay_obligation,
        lending_market,
        source_wallet.pubkey(),
    ));
    let mut transaction =
        Transaction::new_with_payer(&instructions, Some(&config.fee_payer.pubkey()));
    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;
    transaction.sign(
        &vec![config.fee_payer.as_ref(), &source_wallet],
        recent_blockhash,
    );
    send_transaction(config, transaction)?;
    Ok(())
}

fn send_transaction(
    config: &Config,
    transaction: Transaction,
) -> solana_client::client_error::Result<()> {
    if config.dry_run {
        let result = config.rpc_client.simulate_transaction(&transaction)?;
        println!("Simulate result: {:?}", result);
    } else {
        let signature = config
            .rpc_client
            .send_and_confirm_transaction_with_spinner_and_config(
                &transaction,
                CommitmentConfig {
                    commitment: Finalized,
                },
                RpcSendTransactionConfig {
                    skip_preflight: true,
                    ..RpcSendTransactionConfig::default()
                },
            )?;
        println!("Signature: {}", signature);
    }
    Ok(())
}

fn quote_currency_of(matches: &ArgMatches<'_>, name: &str) -> Option<[u8; 32]> {
    if let Some(value) = matches.value_of(name) {
        if value == "USD" {
            Some(*b"USD\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0")
        } else if value.len() <= 32 {
            let mut bytes32 = [0u8; 32];
            bytes32[0..value.len()].clone_from_slice(value.as_bytes());
            Some(bytes32)
        } else {
            Some(Pubkey::from_str(value).unwrap().to_bytes())
        }
    } else {
        None
    }
}
