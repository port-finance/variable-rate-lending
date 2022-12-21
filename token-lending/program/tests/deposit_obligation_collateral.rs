#![cfg(feature = "test-bpf")]

use solana_program::instruction::InstructionError;
use solana_program::program_option::COption;
use solana_program::pubkey::PUBKEY_BYTES;
use solana_program_test::*;
use solana_sdk::transaction::TransactionError;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use spl_token::instruction::approve;

use helpers::*;
use port_finance_variable_rate_lending::error::LendingError;
use port_finance_variable_rate_lending::instruction::deposit_reserve_liquidity_and_obligation_collateral;
use port_finance_variable_rate_lending::state::ReserveConfig;
use port_finance_variable_rate_lending::{
    instruction::deposit_obligation_collateral, processor::process_instruction,
    state::INITIAL_COLLATERAL_RATIO,
};

mod helpers;

#[tokio::test]
async fn test_success() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(88_000);

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: TEST_RESERVE_CONFIG,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;
    sol_test_reserve.validate_state(&mut banks_client).await;
    test_obligation.validate_state(&mut banks_client).await;

    let initial_collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    let initial_user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    // check that collateral tokens were transferred
    let collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    assert_eq!(
        collateral_supply_balance,
        initial_collateral_supply_balance + SOL_DEPOSIT_AMOUNT_LAMPORTS
    );
    sol_test_reserve.validate_state(&mut banks_client).await;
    let user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_collateral_balance,
        initial_user_collateral_balance - SOL_DEPOSIT_AMOUNT_LAMPORTS
    );
}

#[tokio::test]
async fn test_deposit_and_collateral_success() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(88_000);

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: TEST_RESERVE_CONFIG,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    sol_test_reserve.validate_state(&mut banks_client).await;
    let init_liquidity = sol_test_reserve
        .get_state(&mut banks_client)
        .await
        .liquidity
        .available_amount;
    test_obligation.validate_state(&mut banks_client).await;

    let initial_collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;

    assert_eq!(0, initial_collateral_supply_balance);
    let initial_user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    let initial_user_liquidity_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_liquidity_pubkey).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_reserve_liquidity_and_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.collateral_mint_pubkey,
                sol_test_reserve.lending_market_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    banks_client.process_transaction(transaction).await.unwrap();

    // check that collateral tokens were transferred
    let collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    assert_eq!(
        collateral_supply_balance,
        initial_collateral_supply_balance + SOL_DEPOSIT_AMOUNT_LAMPORTS
    );

    assert_eq!(
        sol_test_reserve
            .get_state(&mut banks_client)
            .await
            .liquidity
            .available_amount,
        init_liquidity + SOL_DEPOSIT_AMOUNT_LAMPORTS
    );

    let user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    assert_eq!(user_collateral_balance, initial_user_collateral_balance);
    let user_liquidity_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(
        user_liquidity_balance,
        initial_user_liquidity_balance - SOL_DEPOSIT_AMOUNT_LAMPORTS
    );
}

#[tokio::test]
async fn test_deposit_and_collateral_success_with_liquidity_reward() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );
    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );
    // limit to track compute unit increase
    test.set_compute_max_units(80_000);

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(&mut test, lending_market_authority_pubkey, 0);
    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: ReserveConfig {
                deposit_staking_pool: COption::Some(staking_pool.staking_pool_pubkey),
                ..TEST_RESERVE_CONFIG
            },
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let stake_account: TestStakeAccount = add_stake_account(
        &mut test,
        staking_pool.staking_pool_pubkey,
        &user_accounts_owner,
        0,
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;

    let initial_collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    let initial_user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    let initial_user_liquidity_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_liquidity_pubkey).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_reserve_liquidity_and_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.collateral_mint_pubkey,
                sol_test_reserve.lending_market_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    banks_client.process_transaction(transaction).await.unwrap();

    // check that collateral tokens were transferred
    let collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    assert_eq!(
        collateral_supply_balance,
        initial_collateral_supply_balance + SOL_DEPOSIT_AMOUNT_LAMPORTS
    );
    let user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    assert_eq!(user_collateral_balance, initial_user_collateral_balance);
    let user_liquidity_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(
        user_liquidity_balance,
        initial_user_liquidity_balance - SOL_DEPOSIT_AMOUNT_LAMPORTS
    );
}

#[tokio::test]
async fn test_deposit_and_collateral_fail_because_wrong_owner_with_liquidity_reward() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );
    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );
    // limit to track compute unit increase
    test.set_compute_max_units(60_000);

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_acc_owner = Keypair::new();
    let staking_pool = add_staking_pool(&mut test, lending_market_authority_pubkey, 0);
    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: ReserveConfig {
                deposit_staking_pool: COption::Some(staking_pool.staking_pool_pubkey),
                ..TEST_RESERVE_CONFIG
            },
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let stake_account: TestStakeAccount = add_stake_account(
        &mut test,
        staking_pool.staking_pool_pubkey,
        &staking_acc_owner,
        0,
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_reserve_liquidity_and_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.collateral_mint_pubkey,
                sol_test_reserve.lending_market_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(LendingError::InvalidStakeAccount as u32),
        )
    );
}

#[tokio::test]
async fn test_deposit_and_collateral_success_with_liquidity_reward_fail() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );
    // limit to track compute unit increase
    test.set_compute_max_units(80_000);
    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(&mut test, lending_market_authority_pubkey, 0);
    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: ReserveConfig {
                deposit_staking_pool: COption::Some(staking_pool.staking_pool_pubkey),
                ..TEST_RESERVE_CONFIG
            },
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_reserve_liquidity_and_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.collateral_mint_pubkey,
                sol_test_reserve.lending_market_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );

    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(LendingError::InvalidStakingPool as u32),
        )
    );
}

#[tokio::test]
async fn test_deposit_and_collateral_success_with_liquidity_reward_fail2() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );
    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );
    // limit to track compute unit increase
    test.set_compute_max_units(60_000);

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(&mut test, lending_market_authority_pubkey, 0);
    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: TEST_RESERVE_CONFIG,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let stake_account: TestStakeAccount = add_stake_account(
        &mut test,
        staking_pool.staking_pool_pubkey,
        &user_accounts_owner,
        0,
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_reserve_liquidity_and_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.collateral_mint_pubkey,
                sol_test_reserve.lending_market_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            2,
            InstructionError::Custom(LendingError::InvalidStakingPool as u32),
        )
    );

    // check that collateral tokens were transferred
}

#[tokio::test]
async fn test_success_with_liquidity_reward() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(118_000);

    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(&mut test, lending_market_authority_pubkey, 0);
    let stake_account: TestStakeAccount = add_stake_account(
        &mut test,
        staking_pool.staking_pool_pubkey,
        &user_accounts_owner,
        0,
    );

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: ReserveConfig {
                deposit_staking_pool: COption::Some(staking_pool.staking_pool_pubkey),
                ..TEST_RESERVE_CONFIG
            },
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    let before_staking_pool = staking_pool.get_state(&mut banks_client).await;
    let before_staking_account = stake_account.get_state(&mut banks_client).await;

    assert_eq!(0, before_staking_pool.pool_size);

    assert_eq!(0, before_staking_account.deposited_amount);

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let after_staking_pool = staking_pool.get_state(&mut banks_client).await;
    let after_staking_account = stake_account.get_state(&mut banks_client).await;

    assert_eq!(SOL_DEPOSIT_AMOUNT_LAMPORTS, after_staking_pool.pool_size);

    assert_eq!(
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
        after_staking_account.deposited_amount
    );
}

#[tokio::test]
async fn test_fail_wrong_owner_with_liquidity_reward() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(118_000);

    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let stake_account_owner = Keypair::new();
    let staking_pool = add_staking_pool(&mut test, lending_market_authority_pubkey, 0);
    let stake_account: TestStakeAccount = add_stake_account(
        &mut test,
        staking_pool.staking_pool_pubkey,
        &stake_account_owner,
        0,
    );

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: ReserveConfig {
                deposit_staking_pool: COption::Some(staking_pool.staking_pool_pubkey),
                ..TEST_RESERVE_CONFIG
            },
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    let before_staking_pool = staking_pool.get_state(&mut banks_client).await;
    let before_staking_account = stake_account.get_state(&mut banks_client).await;

    assert_eq!(0, before_staking_pool.pool_size);

    assert_eq!(0, before_staking_account.deposited_amount);

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );

    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(LendingError::InvalidStakeAccount as u32),
        )
    );
}

#[tokio::test]
async fn test_fail_with_liquidity_reward() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(118_000);

    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(&mut test, lending_market_authority_pubkey, 0);
    let stake_account: TestStakeAccount = add_stake_account(
        &mut test,
        staking_pool.staking_pool_pubkey,
        &user_accounts_owner,
        0,
    );

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: TEST_RESERVE_CONFIG,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(LendingError::InvalidStakingPool as u32),
        )
    );
}

#[tokio::test]
async fn test_fail_with_liquidity_reward2() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(118_000);

    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 10 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();

    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(&mut test, lending_market_authority_pubkey, 0);

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_decimals: 9,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            config: ReserveConfig {
                deposit_staking_pool: COption::Some(staking_pool.staking_pool_pubkey),
                ..TEST_RESERVE_CONFIG
            },
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs::default(),
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            deposit_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                user_transfer_authority.pubkey(),
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &vec![&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(LendingError::InvalidStakingPool as u32),
        )
    );
}
