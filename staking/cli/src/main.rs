use std::convert::TryInto;
use std::fmt::Display;

use solana_clap_utils::input_validators::is_slot;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_sdk::commitment_config::CommitmentLevel::Finalized;
use spl_token::instruction::approve;

use port_finance_staking::instruction::{
    add_sub_reward_pool, change_admin, change_duration, change_owner, change_reward_supply,
    init_staking_pool,
};
use port_finance_staking::solana_program::clock::Slot;
use port_finance_staking::state::staking_pool::StakingPool;
use {
    clap::{
        crate_description, crate_name, crate_version, value_t, App, AppSettings, Arg, SubCommand,
    },
    solana_clap_utils::{
        fee_payer::fee_payer_arg,
        input_parsers::{keypair_of, pubkey_of, value_of},
        input_validators::{is_keypair, is_pubkey, is_url},
        keypair::signer_from_path,
    },
    solana_client::rpc_client::RpcClient,
    solana_program::{program_pack::Pack, pubkey::Pubkey},
    solana_sdk::{
        commitment_config::CommitmentConfig,
        signature::{Keypair, Signer},
        system_instruction,
        transaction::Transaction,
    },
    spl_token::state::Account as Token,
    std::process::exit,
    system_instruction::create_account,
};

struct Config {
    rpc_client: RpcClient,
    fee_payer: Box<dyn Signer>,
    staking_program_id: Pubkey,
    verbose: bool,
    dry_run: bool,
}

type Error = Box<dyn std::error::Error>;
type CommandResult = Result<(), Error>;

pub fn is_i64<T>(amount: T) -> Result<(), String>
where
    T: AsRef<str> + Display,
{
    if amount.as_ref().parse::<i64>().is_ok() {
        Ok(())
    } else {
        Err(format!(
            "Unable to parse input amount as integer or float, provided: {}",
            amount
        ))
    }
}

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

fn main() {
    solana_logger::setup_with_default("solana=info");

    let default_staking_program_id: &str = &port_finance_staking::id().to_string();

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
        .arg(fee_payer_arg().short("p").global(true))
        .arg(
            Arg::with_name("staking_program_id")
                .long("program")
                .validator(is_pubkey)
                .value_name("PUBKEY")
                .takes_value(true)
                .required(true)
                .default_value(default_staking_program_id)
                .help("staking program ID"),
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
            SubCommand::with_name("init-staking-pool")
                .about("Create a new staking pool")
                .arg(
                    Arg::with_name("transfer_authority")
                        .long("authority")
                        .validator(is_keypair)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Owner that can transfer reward into staking pool"),
                )
                .arg(
                    Arg::with_name("reward_supply_pubkey")
                        .long("supply_pubkey")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Token account that transfers reward from"),
                )
                .arg(
                    Arg::with_name("sub_reward_supply_pubkey")
                        .long("sub_supply_pubkey")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .help("Token account that transfers sub reward from"),
                )
                .arg(
                    Arg::with_name("reward_token_mint")
                        .long("mint")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Mint of rewarding token"),
                )
                .arg(
                    Arg::with_name("sub_reward_token_mint")
                        .long("sub-mint")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .help("Mint of sub rewarding token"),
                )
                .arg(
                    Arg::with_name("staking_program_owner_authority")
                        .long("owner_authority")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Staking pool owner derived authority"),
                )
                .arg(
                    Arg::with_name("staking_program_admin_authority")
                        .long("admin_authority")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Staking pool admin authority"),
                )
                .arg(
                    Arg::with_name("reward_supply_amount")
                        .long("supply")
                        .validator(is_u64)
                        .value_name("NUMBER")
                        .takes_value(true)
                        .required(true)
                        .help("Amount of reward given"),
                )
                .arg(
                    Arg::with_name("sub_reward_supply_amount")
                        .long("sub_supply")
                        .validator(is_u64)
                        .value_name("NUMBER")
                        .takes_value(true)
                        .help("Amount of sub reward given"),
                )
                .arg(
                    Arg::with_name("duration_of_rewarding")
                        .long("duration")
                        .validator(is_slot)
                        .value_name("SLOT")
                        .takes_value(true)
                        .required(true)
                        .help("Duration of rewarding"),
                )
                .arg(
                    Arg::with_name("earliest_reward_claim_time")
                        .long("claim-time")
                        .validator(is_slot)
                        .value_name("SLOT")
                        .takes_value(true)
                        .required(true)
                        .help("Earliest time to claim the reward"),
                ),
        ).subcommand(SubCommand::with_name("add-sub-reward")
        .about("Add sub reward")
        .arg(
            Arg::with_name("transfer_authority")
                .long("transfer_authority")
                .validator(is_keypair)
                .value_name("KEYPAIR")
                .takes_value(true)
                .required(true)
                .help("Owner that can transfer sub reward into staking pool"),
        )
        .arg(
            Arg::with_name("admin_authority")
                .long("admin_authority")
                .value_name("KEYPAIR")
                .takes_value(true)
                .required(true)
                .help("Admin authority of the staking pool"),
        )
        .arg(
            Arg::with_name("reward_supply_pubkey")
                .long("supply_pubkey")
                .validator(is_pubkey)
                .value_name("PUBKEY")
                .takes_value(true)
                .required(true)
                .help("Token account that transfers sub reward from"),
        )
        .arg(
            Arg::with_name("reward_token_mint")
                .long("mint")
                .validator(is_pubkey)
                .value_name("PUBKEY")
                .takes_value(true)
                .required(true)
                .help("Mint of rewarding token"),
        )
        .arg(
            Arg::with_name("reward_supply_amount")
                .long("supply")
                .validator(is_u64)
                .value_name("NUMBER")
                .takes_value(true)
                .required(true)
                .help("Amount of sub reward given"),
        )
        .arg(
            Arg::with_name("staking_pool")
                .long("pool")
                .validator(is_pubkey)
                .value_name("PUBKEY")
                .takes_value(true)
                .required(true)
                .help("Staking pool to change")
        )
        )
        .subcommand(
        SubCommand::with_name("change-duration")
            .about("increase or decrease the duration of rewards")
            .arg(
                Arg::with_name("admin_authority")
                    .long("admin")
                    .value_name("KEYPAIR")

                    .takes_value(true)
                    .required(true)
                    .help("Admin authority of the staking pool"),
            )
            .arg(
                Arg::with_name("staking_pool")
                    .long("pool")
                    .validator(is_pubkey)
                    .value_name("PUBKEY")
                    .takes_value(true)
                    .required(true)
                    .help("Staking pool to change")
            )
            .arg(
                Arg::with_name("amount")
                    .long("amount")
                    .validator(is_i64)
                    .value_name("i64")
                    .takes_value(true)
                    .required(true)
                    .help("num of slots to change")
            )
        )
        .subcommand(
            SubCommand::with_name("update-earliest-reward-claim-time")
                .about("update earliest reward claim time")
                .arg(
                    Arg::with_name("staking-pool")
                        .long("pool")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Staking pool to change"),
                )
                .arg(
                    Arg::with_name("admin authority")
                        .long("authority")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Admin authority of the staking pool"),
                )
                .arg(
                    Arg::with_name("updated time")
                        .long("time")
                        .validator(is_slot)
                        .value_name("SLOT")
                        .takes_value(true)
                        .required(true)
                        .help("New earliest reward claim time"),
                ),
        )
        .subcommand(
            SubCommand::with_name("change-reward-supply")
                .about("Change the amount of reward in the staking reward pool")
                .arg(
                    Arg::with_name("source_token_owner")
                        .long("source_token_owner")
                        .validator(is_keypair)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(false)
                        .help("In the case of adding reward to the lending pool, this should be supplied"),
                )
                .arg(
                    Arg::with_name("staking_pool_owner")
                        .long("staking_pool_owner")
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(false)
                        .help("The owner of the given staking pool"),
                )
                .arg(
                    Arg::with_name("reward_token_supply")
                        .long("supply")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Token account that supplies or receives the reward token."),
                )
                .arg(
                    Arg::with_name("sub_reward_token_supply")
                        .long("sub_supply")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .help("Token account that supplies or receives the sub reward token."),
                )
                .arg(
                    Arg::with_name("staking_pool")
                        .long("staking_pool")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Pubkey of the staking pool"),
                )
                .arg(
                    Arg::with_name("reward_token_mint")
                        .long("reward_token_mint")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Mint of rewarding token"),
                )
                .arg(
                    Arg::with_name("sub_reward_token_mint")
                        .long("sub_reward_token_mint")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .help("Mint of sub rewarding token"),
                )
                .arg(
                    Arg::with_name("reward_supply_diff")
                        .long("supply_change")
                        .validator(is_i64)
                        .value_name("i64")
                        .takes_value(true)
                        .required(true)
                        .allow_hyphen_values(true)
                        .help("Number of reward changes, positive for increase, negative for decrease."),
                ).arg(
                    Arg::with_name("sub_reward_supply_diff")
                        .long("sub_supply_change")
                        .validator(is_i64)
                        .value_name("i64")
                        .takes_value(true)
                        .allow_hyphen_values(true)
                        .help("Number of sub reward changes, positive for increase, negative for decrease."),
                ),
        )
        .subcommand(
            SubCommand::with_name("change-staking-pool-owner")
                .about("Change the owner of the new staking pool")
                .arg(
                    Arg::with_name("old_staking_pool_owner")
                        .long("old_staking_pool_owner")
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .help("The owner of the given staking pool"),
                )
                .arg(
                    Arg::with_name("new_staking_pool_owner")
                        .long("new_staking_pool_owner")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Public key for the new staking pool."),
                )
                .arg(
                    Arg::with_name("staking_pool")
                        .long("staking_pool")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Pubkey of the staking pool"),
                )
        )
        .subcommand(
            SubCommand::with_name("change-staking-pool-admin")
                .about("Change the admin of the new staking pool")
                .arg(
                    Arg::with_name("old_staking_pool_admin")
                        .long("old_staking_pool_admin")
                        .value_name("KEYPAIR")
                        .takes_value(true)
                        .required(true)
                        .help("The admin of the given staking pool"),
                )
                .arg(
                    Arg::with_name("new_staking_pool_admin")
                        .long("new_staking_pool_admin")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Public key for the new staking pool admin."),
                )
                .arg(
                    Arg::with_name("staking_pool")
                        .long("staking_pool")
                        .validator(is_pubkey)
                        .value_name("PUBKEY")
                        .takes_value(true)
                        .required(true)
                        .help("Pubkey of the staking pool"),
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

        let staking_program_id = pubkey_of(&matches, "staking_program_id").unwrap();
        let verbose = matches.is_present("verbose");
        let dry_run = matches.is_present("dry_run");

        Config {
            rpc_client: RpcClient::new_with_commitment(json_rpc_url, CommitmentConfig::confirmed()),
            fee_payer,
            staking_program_id,
            verbose,
            dry_run,
        }
    };

    let _ = match matches.subcommand() {
        ("init-staking-pool", Some(arg_matches)) => {
            let transfer_authority = keypair_of(arg_matches, "transfer_authority").unwrap();
            let reward_supply = pubkey_of(arg_matches, "reward_supply_pubkey").unwrap();
            let reward_token_mint = pubkey_of(arg_matches, "reward_token_mint").unwrap();
            let sub_reward_supply = pubkey_of(arg_matches, "sub_reward_supply_pubkey");
            let sub_reward_token_mint = pubkey_of(arg_matches, "sub_reward_token_mint");
            let staking_program_owner_authority =
                pubkey_of(arg_matches, "staking_program_owner_authority").unwrap();
            let staking_program_admin_authority =
                pubkey_of(arg_matches, "staking_program_admin_authority").unwrap();
            let supply = value_of(arg_matches, "reward_supply_amount").unwrap();
            let sub_supply = value_of(arg_matches, "sub_reward_supply_amount");
            let duration = value_of(arg_matches, "duration_of_rewarding").unwrap();
            let claim_time = value_of(arg_matches, "earliest_reward_claim_time").unwrap();
            command_init_staking_pool(
                &config,
                transfer_authority,
                reward_supply,
                sub_reward_supply,
                reward_token_mint,
                sub_reward_token_mint,
                staking_program_owner_authority,
                staking_program_admin_authority,
                supply,
                sub_supply,
                duration,
                claim_time,
            )
        }
        ("add-sub-reward", Some(arg_matches)) => {
            let transfer_authority = signer_from_path(
                arg_matches,
                arg_matches.value_of("transfer_authority").unwrap(),
                "transfer_authority",
                &mut wallet_manager,
            )
            .unwrap();
            let admin_authority = signer_from_path(
                arg_matches,
                arg_matches.value_of("admin_authority").unwrap(),
                "admin_authority",
                &mut wallet_manager,
            )
            .unwrap();
            let reward_supply = pubkey_of(arg_matches, "reward_supply_pubkey").unwrap();
            let reward_token_mint = pubkey_of(arg_matches, "reward_token_mint").unwrap();
            let staking_pool = pubkey_of(arg_matches, "staking_pool").unwrap();
            let supply = value_of(arg_matches, "reward_supply_amount").unwrap();

            command_add_sub_reward(
                &config,
                transfer_authority,
                admin_authority,
                staking_pool,
                reward_supply,
                reward_token_mint,
                supply,
            )
        }
        ("change-duration", Some(arg_matches)) => {
            let admin_authority = signer_from_path(
                arg_matches,
                arg_matches.value_of("admin_authority").unwrap(),
                "admin_authority",
                &mut wallet_manager,
            )
            .unwrap();
            let staking_pool = pubkey_of(arg_matches, "staking_pool").unwrap();
            let amount = value_of(arg_matches, "amount").unwrap();
            command_change_duration(&config, admin_authority, staking_pool, amount)
        }
        // TODO: implement update reward claim time
        ("update-earliest-reward-claim-time", Some(_arg_matches)) => Ok(()),
        // TODO: implement change reward supply
        ("change-reward-supply", Some(arg_matches)) => {
            let mut wallet_manager = None;
            let staking_pool_owner = if arg_matches.value_of("staking_pool_owner").is_none() {
                Option::None
            } else {
                Some(
                    signer_from_path(
                        arg_matches,
                        arg_matches.value_of("staking_pool_owner").unwrap(),
                        "staking_pool_owner",
                        &mut wallet_manager,
                    )
                    .unwrap(),
                )
            };
            let source_token_owner = keypair_of(arg_matches, "source_token_owner");
            let reward_token_supply = pubkey_of(arg_matches, "reward_token_supply").unwrap();
            let staking_pool = pubkey_of(arg_matches, "staking_pool").unwrap();
            let reward_token_mint = pubkey_of(arg_matches, "reward_token_mint").unwrap();
            let reward_supply_diff = value_of(arg_matches, "reward_supply_diff").unwrap();
            let sub_reward_token_supply = pubkey_of(arg_matches, "sub_reward_token_supply");
            let sub_reward_token_mint = pubkey_of(arg_matches, "sub_reward_token_mint");
            let sub_reward_supply_diff = value_of(arg_matches, "sub_reward_supply_diff");
            command_change_reward_supply(
                &config,
                staking_pool_owner,
                source_token_owner,
                staking_pool,
                reward_token_supply,
                reward_token_mint,
                reward_supply_diff,
                sub_reward_token_supply,
                sub_reward_token_mint,
                sub_reward_supply_diff,
            )
        }
        ("change-staking-pool-owner", Some(arg_matches)) => {
            let mut wallet_manager = None;
            let old_staking_pool_owner = signer_from_path(
                arg_matches,
                arg_matches.value_of("old_staking_pool_owner").unwrap(),
                "old_staking_pool_owner",
                &mut wallet_manager,
            )
            .unwrap();
            let new_staking_pool_owner = pubkey_of(arg_matches, "new_staking_pool_owner").unwrap();
            let staking_pool = pubkey_of(arg_matches, "staking_pool").unwrap();

            command_change_staking_pool_owner(
                &config,
                old_staking_pool_owner,
                new_staking_pool_owner,
                staking_pool,
            )
        }
        ("change-staking-pool-admin", Some(arg_matches)) => {
            let mut wallet_manager = None;
            let old_staking_pool_admin = signer_from_path(
                arg_matches,
                arg_matches.value_of("old_staking_pool_admin").unwrap(),
                "old_staking_pool_admin",
                &mut wallet_manager,
            )
            .unwrap();
            let new_staking_pool_admin = pubkey_of(arg_matches, "new_staking_pool_admin").unwrap();
            let staking_pool = pubkey_of(arg_matches, "staking_pool").unwrap();

            command_change_staking_pool_admin(
                &config,
                old_staking_pool_admin,
                new_staking_pool_admin,
                staking_pool,
            )
        }
        _ => unreachable!(),
    }
    .map_err(|err| {
        eprintln!("{}", err);
        exit(1);
    });
}

#[allow(clippy::too_many_arguments)]
fn command_change_staking_pool_admin(
    config: &Config,
    current_staking_pool_admin: Box<dyn Signer>,
    new_staking_pool_admin: Pubkey,
    staking_pool: Pubkey,
) -> CommandResult {
    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;

    let mut transaction = Transaction::new_with_payer(
        &[change_admin(
            config.staking_program_id,
            new_staking_pool_admin,
            current_staking_pool_admin.pubkey(),
            staking_pool,
        )],
        Some(&config.fee_payer.pubkey()),
    );
    transaction.sign(
        &vec![
            config.fee_payer.as_ref(),
            current_staking_pool_admin.as_ref(),
        ],
        recent_blockhash,
    );
    send_transaction(config, transaction)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn command_change_staking_pool_owner(
    config: &Config,
    current_staking_pool_owner: Box<dyn Signer>,
    new_staking_pool_owner: Pubkey,
    staking_pool: Pubkey,
) -> CommandResult {
    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;

    let mut transaction = Transaction::new_with_payer(
        &[change_owner(
            config.staking_program_id,
            new_staking_pool_owner,
            current_staking_pool_owner.pubkey(),
            staking_pool,
        )],
        Some(&config.fee_payer.pubkey()),
    );
    transaction.sign(
        &vec![
            config.fee_payer.as_ref(),
            current_staking_pool_owner.as_ref(),
        ],
        recent_blockhash,
    );
    send_transaction(config, transaction)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn command_change_duration(
    config: &Config,
    admin_authority: Box<dyn Signer>,
    staking_pool: Pubkey,
    amount: i64,
) -> CommandResult {
    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;

    let mut transaction = Transaction::new_with_payer(
        &[change_duration(
            config.staking_program_id,
            amount,
            admin_authority.pubkey(),
            staking_pool,
        )],
        Some(&config.fee_payer.pubkey()),
    );
    transaction.sign(
        &vec![config.fee_payer.as_ref(), admin_authority.as_ref()],
        recent_blockhash,
    );
    send_transaction(config, transaction)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn command_change_reward_supply(
    config: &Config,
    staking_pool_owner_authority: Option<Box<dyn Signer>>,
    source_owner: Option<Keypair>,
    staking_pool: Pubkey,
    reward_token_supply: Pubkey,
    reward_token_mint: Pubkey,
    reward_supply_amount: i64,
    sub_reward_token_supply: Option<Pubkey>,
    sub_reward_token_mint: Option<Pubkey>,
    sub_reward_supply_amount: Option<i64>,
) -> CommandResult {
    if config.verbose {
        println!(
            "staking pool {} supply {}, sub_supply {:?}",
            staking_pool, reward_supply_amount, sub_reward_supply_amount
        );
    }
    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;
    let reward_token_pool_pubkey =
        StakingPool::unpack(&config.rpc_client.get_account(&staking_pool).unwrap().data)
            .unwrap()
            .reward_token_pool;
    let sub_reward_token_pool_pubkey =
        StakingPool::unpack(&config.rpc_client.get_account(&staking_pool).unwrap().data)
            .unwrap()
            .sub_reward_token_pool;
    if source_owner.is_some() && reward_supply_amount > 0 {
        let transfer_authority = Keypair::new();
        let mut transaction = Transaction::new_with_payer(
            &[
                approve(
                    &spl_token::id(),
                    &reward_token_supply,
                    &transfer_authority.pubkey(),
                    &source_owner.as_ref().unwrap().pubkey(),
                    &[],
                    reward_supply_amount.try_into().unwrap(),
                )
                .unwrap(),
                approve(
                    &spl_token::id(),
                    &sub_reward_token_supply.unwrap(),
                    &transfer_authority.pubkey(),
                    &source_owner.as_ref().unwrap().pubkey(),
                    &[],
                    sub_reward_supply_amount.unwrap().try_into().unwrap(),
                )
                .unwrap(),
                change_reward_supply(
                    config.staking_program_id,
                    reward_supply_amount,
                    sub_reward_supply_amount,
                    transfer_authority.pubkey(),
                    reward_token_supply,
                    reward_token_mint,
                    staking_pool,
                    reward_token_pool_pubkey,
                    sub_reward_token_supply,
                    sub_reward_token_mint,
                    sub_reward_token_pool_pubkey,
                ),
            ],
            Some(&config.fee_payer.pubkey()),
        );
        transaction.sign(
            &vec![
                config.fee_payer.as_ref(),
                &source_owner.unwrap(),
                &transfer_authority,
            ],
            recent_blockhash,
        );
        send_transaction(config, transaction)?;
        Ok(())
    } else if staking_pool_owner_authority.is_some() && reward_supply_amount < 0 {
        let mut transaction = Transaction::new_with_payer(
            &[change_reward_supply(
                config.staking_program_id,
                reward_supply_amount,
                sub_reward_supply_amount,
                staking_pool_owner_authority.as_ref().unwrap().pubkey(),
                reward_token_supply,
                reward_token_mint,
                staking_pool,
                reward_token_pool_pubkey,
                sub_reward_token_supply,
                sub_reward_token_mint,
                sub_reward_token_pool_pubkey,
            )],
            Some(&config.fee_payer.pubkey()),
        );
        transaction.sign(
            &vec![
                config.fee_payer.as_ref(),
                staking_pool_owner_authority.unwrap().as_ref(),
            ],
            recent_blockhash,
        );
        send_transaction(config, transaction)?;
        Ok(())
    } else {
        unreachable!()
    }
}

#[allow(clippy::too_many_arguments)]
fn command_init_staking_pool(
    config: &Config,
    transfer_authority: Keypair,
    reward_supply: Pubkey,
    sub_reward_supply: Option<Pubkey>,
    reward_token_mint: Pubkey,
    sub_reward_token_mint: Option<Pubkey>,
    staking_program_owner_authority: Pubkey,
    staking_program_admin_authority: Pubkey,
    supply: u64,
    sub_supply: Option<u64>,
    duration: u64,
    claim_time: Slot,
) -> CommandResult {
    let staking_pool_keypair = Keypair::new();
    let reward_pool_keypair = Keypair::new();
    let sub_reward_pool_keypair = Keypair::new();

    let staking_pool_balance = config
        .rpc_client
        .get_minimum_balance_for_rent_exemption(StakingPool::LEN)?;
    let reward_pool_balance = config
        .rpc_client
        .get_minimum_balance_for_rent_exemption(Token::LEN)?;

    println!(
        "staking pool {} \n \
        reward pool {} \n \
        reward mint {} mount {} \n",
        staking_pool_keypair.pubkey(),
        reward_pool_keypair.pubkey(),
        reward_token_mint,
        supply
    );

    if config.verbose {
        println!("transfer_authority {}", transfer_authority.pubkey());
        println!(
            "staking_program_owner_authority {}",
            staking_program_owner_authority
        );
        println!(
            "staking_program_admin_authority {}",
            staking_program_admin_authority
        );
        println!("claim_time {}", claim_time);
    }
    let mut instructions = if sub_supply.is_some() {
        vec![create_account(
            &config.fee_payer.pubkey(),
            &sub_reward_pool_keypair.pubkey(),
            reward_pool_balance,
            Token::LEN as u64,
            &spl_token::id(),
        )]
    } else {
        vec![]
    };
    instructions.extend([
        create_account(
            &config.fee_payer.pubkey(),
            &reward_pool_keypair.pubkey(),
            reward_pool_balance,
            Token::LEN as u64,
            &spl_token::id(),
        ),
        create_account(
            &config.fee_payer.pubkey(),
            &staking_pool_keypair.pubkey(),
            staking_pool_balance,
            StakingPool::LEN as u64,
            &config.staking_program_id,
        ),
        init_staking_pool(
            config.staking_program_id,
            supply,
            sub_supply,
            duration,
            claim_time,
            transfer_authority.pubkey(),
            reward_supply,
            reward_pool_keypair.pubkey(),
            sub_reward_supply,
            sub_reward_token_mint.map(|_| sub_reward_pool_keypair.pubkey()),
            staking_pool_keypair.pubkey(),
            reward_token_mint,
            sub_reward_token_mint,
            staking_program_owner_authority,
            staking_program_admin_authority,
        ),
    ]);
    let mut transaction =
        Transaction::new_with_payer(&instructions, Some(&config.fee_payer.pubkey()));
    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;

    let mut signers: Vec<&dyn Signer> = if sub_supply.is_some() {
        vec![&sub_reward_pool_keypair]
    } else {
        vec![]
    };
    signers.extend([
        config.fee_payer.as_ref(),
        &reward_pool_keypair,
        &staking_pool_keypair,
        &transfer_authority,
    ]);
    transaction.sign(&signers, recent_blockhash);
    send_transaction(config, transaction)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn command_add_sub_reward(
    config: &Config,
    transfer_authority: Box<dyn Signer>,
    admin_authority: Box<dyn Signer>,
    staking_pool: Pubkey,
    reward_supply: Pubkey,
    reward_token_mint: Pubkey,
    supply: u64,
) -> CommandResult {
    let reward_pool_keypair = Keypair::new();

    let reward_pool_balance = config
        .rpc_client
        .get_minimum_balance_for_rent_exemption(Token::LEN)?;

    println!(
        "staking pool {} \n \
        reward pool {} \n \
        reward mint {} mount {} \n",
        staking_pool,
        reward_pool_keypair.pubkey(),
        reward_token_mint,
        supply
    );

    if config.verbose {
        println!("transfer_authority {}", transfer_authority.pubkey());
        println!("admin_authority {}", admin_authority.pubkey());
    }

    let mut transaction = Transaction::new_with_payer(
        &[
            create_account(
                &config.fee_payer.pubkey(),
                &reward_pool_keypair.pubkey(),
                reward_pool_balance,
                Token::LEN as u64,
                &spl_token::id(),
            ),
            add_sub_reward_pool(
                config.staking_program_id,
                supply,
                transfer_authority.pubkey(),
                admin_authority.pubkey(),
                reward_supply,
                reward_token_mint,
                staking_pool,
                reward_pool_keypair.pubkey(),
            ),
        ],
        Some(&config.fee_payer.pubkey()),
    );

    let recent_blockhash = config.rpc_client.get_latest_blockhash()?;
    transaction.sign(
        &vec![
            config.fee_payer.as_ref(),
            &reward_pool_keypair,
            admin_authority.as_ref(),
            transfer_authority.as_ref(),
        ],
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
