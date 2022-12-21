#![cfg(feature = "test-bpf")]

use std::u64;

use solana_program_test::*;
use solana_sdk::{
    instruction::InstructionError,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::{Transaction, TransactionError},
};

use helpers::*;
use port_finance_staking::solana_program::program_option::COption;
use port_finance_staking::solana_program::pubkey::PUBKEY_BYTES;
use port_finance_variable_rate_lending::instruction::update_reserve;
use port_finance_variable_rate_lending::state::ReserveConfig;
use port_finance_variable_rate_lending::{
    error::LendingError,
    instruction::{refresh_obligation, withdraw_obligation_collateral},
    processor::process_instruction,
    state::INITIAL_COLLATERAL_RATIO,
};

mod helpers;

#[tokio::test]
async fn test_withdraw_fixed_amount() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(100_000);

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 200 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const WITHDRAW_AMOUNT: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = TEST_RESERVE_CONFIG;
    reserve_config.loan_to_value_ratio = 50;

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            collateral_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            liquidity_mint_decimals: 9,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_pyth_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            borrow_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs {
            deposits: &[(&sol_test_reserve, SOL_DEPOSIT_AMOUNT_LAMPORTS)],
            borrows: &[(&usdc_test_reserve, USDC_BORROW_AMOUNT_FRACTIONAL)],
            ..AddObligationArgs::default()
        },
    );

    let test_collateral = &test_obligation.deposits[0];
    let test_liquidity = &test_obligation.borrows[0];

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;
    test_collateral.validate_state(&mut banks_client).await;
    test_liquidity.validate_state(&mut banks_client).await;

    let initial_collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    let initial_user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            withdraw_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                WITHDRAW_AMOUNT,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    // check that collateral tokens were transferred
    let collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    assert_eq!(
        collateral_supply_balance,
        initial_collateral_supply_balance - WITHDRAW_AMOUNT
    );
    let user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_collateral_balance,
        initial_user_collateral_balance + WITHDRAW_AMOUNT
    );

    let obligation = test_obligation.get_state(&mut banks_client).await;
    let collateral = &obligation.deposits[0];
    assert_eq!(
        collateral.deposited_amount,
        SOL_DEPOSIT_AMOUNT_LAMPORTS - WITHDRAW_AMOUNT
    );
}

#[tokio::test]
async fn test_withdraw_max_amount() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(100_000);

    const USDC_DEPOSIT_AMOUNT_FRACTIONAL: u64 =
        1_000 * FRACTIONAL_TO_USDC * INITIAL_COLLATERAL_RATIO;
    const USDC_RESERVE_COLLATERAL_FRACTIONAL: u64 = 2 * USDC_DEPOSIT_AMOUNT_FRACTIONAL;
    const WITHDRAW_AMOUNT: u64 = u64::MAX;

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = TEST_RESERVE_CONFIG;
    reserve_config.loan_to_value_ratio = 50;

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_pyth_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            collateral_amount: USDC_RESERVE_COLLATERAL_FRACTIONAL,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs {
            deposits: &[(&usdc_test_reserve, USDC_DEPOSIT_AMOUNT_FRACTIONAL)],
            ..AddObligationArgs::default()
        },
    );

    let test_collateral = &test_obligation.deposits[0];

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;
    test_collateral.validate_state(&mut banks_client).await;

    let initial_collateral_supply_balance = get_token_balance(
        &mut banks_client,
        usdc_test_reserve.collateral_supply_pubkey,
    )
    .await;
    let initial_user_collateral_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_collateral_pubkey).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![usdc_test_reserve.pubkey],
            ),
            withdraw_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                WITHDRAW_AMOUNT,
                usdc_test_reserve.collateral_supply_pubkey,
                usdc_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    // check that collateral tokens were transferred
    let collateral_supply_balance = get_token_balance(
        &mut banks_client,
        usdc_test_reserve.collateral_supply_pubkey,
    )
    .await;
    assert_eq!(
        collateral_supply_balance,
        initial_collateral_supply_balance - USDC_DEPOSIT_AMOUNT_FRACTIONAL
    );
    let user_collateral_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_collateral_balance,
        initial_user_collateral_balance + USDC_DEPOSIT_AMOUNT_FRACTIONAL
    );

    let obligation = test_obligation.get_state(&mut banks_client).await;
    assert_eq!(obligation.deposits.len(), 0);
}

#[tokio::test]
async fn test_withdraw_too_large() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 200 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const WITHDRAW_AMOUNT: u64 = (100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO) + 1;

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = TEST_RESERVE_CONFIG;
    reserve_config.loan_to_value_ratio = 50;

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            collateral_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            liquidity_mint_decimals: 9,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_pyth_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            borrow_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs {
            deposits: &[(&sol_test_reserve, SOL_DEPOSIT_AMOUNT_LAMPORTS)],
            borrows: &[(&usdc_test_reserve, USDC_BORROW_AMOUNT_FRACTIONAL)],
            ..AddObligationArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            withdraw_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                WITHDRAW_AMOUNT,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);

    // check that transaction fails
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            1,
            InstructionError::Custom(LendingError::WithdrawTooLarge as u32)
        )
    );
}

#[tokio::test]
async fn test_withdraw_fixed_amount_liquidity_mining() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);

    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 200 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const WITHDRAW_AMOUNT: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(
        &mut test,
        lending_market_authority_pubkey,
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
    );
    let stake_account: TestStakeAccount = add_stake_account(
        &mut test,
        staking_pool.staking_pool_pubkey,
        &user_accounts_owner,
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
    );

    let mut reserve_config = ReserveConfig {
        deposit_staking_pool: COption::Some(staking_pool.staking_pool_pubkey),
        ..TEST_RESERVE_CONFIG
    };
    reserve_config.loan_to_value_ratio = 50;

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            collateral_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            liquidity_mint_decimals: 9,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_pyth_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            borrow_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs {
            deposits: &[(&sol_test_reserve, SOL_DEPOSIT_AMOUNT_LAMPORTS)],
            borrows: &[(&usdc_test_reserve, USDC_BORROW_AMOUNT_FRACTIONAL)],
            ..AddObligationArgs::default()
        },
    );

    let test_collateral = &test_obligation.deposits[0];
    let test_liquidity = &test_obligation.borrows[0];

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;
    test_collateral.validate_state(&mut banks_client).await;
    test_liquidity.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            withdraw_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                WITHDRAW_AMOUNT,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    let before_staking_pool = staking_pool.get_state(&mut banks_client).await;
    let before_staking_account = stake_account.get_state(&mut banks_client).await;

    assert_eq!(SOL_DEPOSIT_AMOUNT_LAMPORTS, before_staking_pool.pool_size);

    assert_eq!(
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
        before_staking_account.deposited_amount
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);
    banks_client.process_transaction(transaction).await.unwrap();
    let after_staking_pool = staking_pool.get_state(&mut banks_client).await;
    let after_staking_account = stake_account.get_state(&mut banks_client).await;

    assert_eq!(
        SOL_DEPOSIT_AMOUNT_LAMPORTS - WITHDRAW_AMOUNT,
        after_staking_pool.pool_size
    );

    assert_eq!(
        SOL_DEPOSIT_AMOUNT_LAMPORTS - WITHDRAW_AMOUNT,
        after_staking_account.deposited_amount
    );
}

#[tokio::test]
async fn test_withdraw_fixed_amount_liquidity_mining_fail_owner_not_match() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);

    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 200 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const WITHDRAW_AMOUNT: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(
        &mut test,
        lending_market_authority_pubkey,
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
    );
    let stake_acc_owner = Keypair::new();
    let stake_account: TestStakeAccount = add_stake_account(
        &mut test,
        staking_pool.staking_pool_pubkey,
        &stake_acc_owner,
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
    );

    let mut reserve_config = ReserveConfig {
        deposit_staking_pool: COption::Some(staking_pool.staking_pool_pubkey),
        ..TEST_RESERVE_CONFIG
    };
    reserve_config.loan_to_value_ratio = 50;

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            collateral_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            liquidity_mint_decimals: 9,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_pyth_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            borrow_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs {
            deposits: &[(&sol_test_reserve, SOL_DEPOSIT_AMOUNT_LAMPORTS)],
            borrows: &[(&usdc_test_reserve, USDC_BORROW_AMOUNT_FRACTIONAL)],
            ..AddObligationArgs::default()
        },
    );

    let test_collateral = &test_obligation.deposits[0];
    let test_liquidity = &test_obligation.borrows[0];

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;
    test_collateral.validate_state(&mut banks_client).await;
    test_liquidity.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            withdraw_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                WITHDRAW_AMOUNT,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    let before_staking_pool = staking_pool.get_state(&mut banks_client).await;
    let before_staking_account = stake_account.get_state(&mut banks_client).await;

    assert_eq!(SOL_DEPOSIT_AMOUNT_LAMPORTS, before_staking_pool.pool_size);

    assert_eq!(
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
        before_staking_account.deposited_amount
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);
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
async fn test_withdraw_fixed_amount_liquidity_mining_fail() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(33_000);

    test.prefer_bpf(false);
    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 200 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_000 * FRACTIONAL_TO_USDC;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const WITHDRAW_AMOUNT: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(
        &mut test,
        lending_market_authority_pubkey,
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
    );
    let stake_account: TestStakeAccount = add_stake_account(
        &mut test,
        staking_pool.staking_pool_pubkey,
        &user_accounts_owner,
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
    );

    let mut reserve_config = ReserveConfig {
        deposit_staking_pool: COption::Some(staking_pool.staking_pool_pubkey),
        ..TEST_RESERVE_CONFIG
    };
    reserve_config.loan_to_value_ratio = 50;

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            collateral_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            liquidity_mint_decimals: 9,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_pyth_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            borrow_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: reserve_config,
            mark_fresh: true,
            ..AddReserveArgs::default()
        },
    );

    let test_obligation = add_obligation(
        &mut test,
        &lending_market,
        &user_accounts_owner,
        AddObligationArgs {
            deposits: &[(&sol_test_reserve, SOL_DEPOSIT_AMOUNT_LAMPORTS)],
            borrows: &[(&usdc_test_reserve, USDC_BORROW_AMOUNT_FRACTIONAL)],
            ..AddObligationArgs::default()
        },
    );

    let test_collateral = &test_obligation.deposits[0];
    let test_liquidity = &test_obligation.borrows[0];

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    test_obligation.validate_state(&mut banks_client).await;
    test_collateral.validate_state(&mut banks_client).await;
    test_liquidity.validate_state(&mut banks_client).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            withdraw_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                WITHDRAW_AMOUNT,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    let before_staking_pool = staking_pool.get_state(&mut banks_client).await;
    let before_staking_account = stake_account.get_state(&mut banks_client).await;

    assert_eq!(SOL_DEPOSIT_AMOUNT_LAMPORTS, before_staking_pool.pool_size);

    assert_eq!(
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
        before_staking_account.deposited_amount
    );

    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);
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
    let after_staking_pool = staking_pool.get_state(&mut banks_client).await;
    let after_staking_account = stake_account.get_state(&mut banks_client).await;

    assert_eq!(SOL_DEPOSIT_AMOUNT_LAMPORTS, after_staking_pool.pool_size);

    assert_eq!(
        SOL_DEPOSIT_AMOUNT_LAMPORTS,
        after_staking_account.deposited_amount
    );

    let mut transaction = Transaction::new_with_payer(
        &[
            update_reserve(
                port_finance_variable_rate_lending::id(),
                TEST_RESERVE_CONFIG,
                sol_test_reserve.pubkey,
                lending_market.pubkey,
                lending_market.owner.pubkey(),
            ),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            withdraw_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                WITHDRAW_AMOUNT,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                test_obligation.owner,
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(
        &[&payer, &user_accounts_owner, &lending_market.owner],
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
