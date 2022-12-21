#![cfg(feature = "test-bpf")]

mod helpers;

use helpers::*;
use port_finance_variable_rate_lending::math::Decimal;
use port_finance_variable_rate_lending::{
    error::LendingError,
    instruction::init_reserve,
    processor::process_instruction,
    state::{ReserveFees, INITIAL_COLLATERAL_RATIO},
};
use solana_program_test::*;
use solana_sdk::{
    instruction::InstructionError,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::{Transaction, TransactionError},
};
use spl_token::solana_program::program_option::COption;

#[tokio::test]
async fn test_success() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(100_000);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);
    let sol_oracle = add_sol_pyth_oracle(&mut test);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;

    const RESERVE_AMOUNT: u64 = 42;

    let sol_user_liquidity_account = create_and_mint_to_token_account(
        &mut banks_client,
        spl_token::native_mint::id(),
        None,
        &payer,
        user_accounts_owner.pubkey(),
        RESERVE_AMOUNT,
    )
    .await;

    let sol_reserve = TestReserve::init(
        "sol".to_owned(),
        &mut banks_client,
        &lending_market,
        &COption::Some(sol_oracle),
        RESERVE_AMOUNT,
        COption::None,
        TEST_RESERVE_CONFIG,
        spl_token::native_mint::id(),
        sol_user_liquidity_account,
        &payer,
        &user_accounts_owner,
    )
    .await
    .unwrap();

    sol_reserve.validate_state(&mut banks_client).await;

    let sol_liquidity_supply =
        get_token_balance(&mut banks_client, sol_reserve.liquidity_supply_pubkey).await;
    assert_eq!(sol_liquidity_supply, RESERVE_AMOUNT);
    let user_sol_balance =
        get_token_balance(&mut banks_client, sol_reserve.user_liquidity_pubkey).await;
    assert_eq!(user_sol_balance, 0);
    let user_sol_collateral_balance =
        get_token_balance(&mut banks_client, sol_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_sol_collateral_balance,
        RESERVE_AMOUNT * INITIAL_COLLATERAL_RATIO
    );
}

#[tokio::test]
async fn test_success_switchboard() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(200_000);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);
    let (slot, sol_oracle) = add_sol_switchboard_oracle(&mut test, false);

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(slot.unwrap_or(1) + 2).unwrap(); // clock.slot = 3

    let ProgramTestContext {
        mut banks_client,
        payer,
        ..
    } = test_context;

    const RESERVE_AMOUNT: u64 = 42;

    let sol_user_liquidity_account = create_and_mint_to_token_account(
        &mut banks_client,
        spl_token::native_mint::id(),
        None,
        &payer,
        user_accounts_owner.pubkey(),
        RESERVE_AMOUNT,
    )
    .await;

    let sol_reserve = TestReserve::init(
        "sol".to_owned(),
        &mut banks_client,
        &lending_market,
        &COption::Some(sol_oracle),
        RESERVE_AMOUNT,
        COption::None,
        TEST_RESERVE_CONFIG,
        spl_token::native_mint::id(),
        sol_user_liquidity_account,
        &payer,
        &user_accounts_owner,
    )
    .await
    .unwrap();

    sol_reserve.validate_state(&mut banks_client).await;

    let sol_liquidity_supply =
        get_token_balance(&mut banks_client, sol_reserve.liquidity_supply_pubkey).await;
    assert_eq!(sol_liquidity_supply, RESERVE_AMOUNT);
    let user_sol_balance =
        get_token_balance(&mut banks_client, sol_reserve.user_liquidity_pubkey).await;
    assert_eq!(user_sol_balance, 0);
    let user_sol_collateral_balance =
        get_token_balance(&mut banks_client, sol_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_sol_collateral_balance,
        RESERVE_AMOUNT * INITIAL_COLLATERAL_RATIO
    );
}

#[tokio::test]
async fn test_success_switchboard_parsed() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(200_000);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);
    let (_, sol_oracle) = add_sol_switchboard_oracle(&mut test, true);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;

    const RESERVE_AMOUNT: u64 = 42;

    let sol_user_liquidity_account = create_and_mint_to_token_account(
        &mut banks_client,
        spl_token::native_mint::id(),
        None,
        &payer,
        user_accounts_owner.pubkey(),
        RESERVE_AMOUNT,
    )
    .await;

    let sol_reserve = TestReserve::init(
        "sol".to_owned(),
        &mut banks_client,
        &lending_market,
        &COption::Some(sol_oracle),
        RESERVE_AMOUNT,
        COption::None,
        TEST_RESERVE_CONFIG,
        spl_token::native_mint::id(),
        sol_user_liquidity_account,
        &payer,
        &user_accounts_owner,
    )
    .await
    .unwrap();

    sol_reserve.validate_state(&mut banks_client).await;

    let sol_liquidity_supply =
        get_token_balance(&mut banks_client, sol_reserve.liquidity_supply_pubkey).await;
    assert_eq!(sol_liquidity_supply, RESERVE_AMOUNT);
    let user_sol_balance =
        get_token_balance(&mut banks_client, sol_reserve.user_liquidity_pubkey).await;
    assert_eq!(user_sol_balance, 0);
    let user_sol_collateral_balance =
        get_token_balance(&mut banks_client, sol_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_sol_collateral_balance,
        RESERVE_AMOUNT * INITIAL_COLLATERAL_RATIO
    );
}

#[tokio::test]
async fn test_success_switchboard_v2() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(200_000);

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);
    let (_, gst_oracle) = add_gst_switchboard_oracle_v2(&mut test);

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(134883675).unwrap(); // oracle open slot is at 134883674.

    let ProgramTestContext {
        mut banks_client,
        payer,
        ..
    } = test_context;

    const RESERVE_AMOUNT: u64 = 42;

    let sol_user_liquidity_account = create_and_mint_to_token_account(
        &mut banks_client,
        spl_token::native_mint::id(),
        None,
        &payer,
        user_accounts_owner.pubkey(),
        RESERVE_AMOUNT,
    )
    .await;

    let gst_reserve = TestReserve::init(
        "gst".to_owned(),
        &mut banks_client,
        &lending_market,
        &COption::Some(gst_oracle),
        RESERVE_AMOUNT,
        COption::None,
        TEST_RESERVE_CONFIG,
        spl_token::native_mint::id(),
        sol_user_liquidity_account,
        &payer,
        &user_accounts_owner,
    )
    .await
    .unwrap();

    gst_reserve.validate_state(&mut banks_client).await;

    let sol_liquidity_supply =
        get_token_balance(&mut banks_client, gst_reserve.liquidity_supply_pubkey).await;
    assert_eq!(sol_liquidity_supply, RESERVE_AMOUNT);
    let user_sol_balance =
        get_token_balance(&mut banks_client, gst_reserve.user_liquidity_pubkey).await;
    assert_eq!(user_sol_balance, 0);
    let user_sol_collateral_balance =
        get_token_balance(&mut banks_client, gst_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_sol_collateral_balance,
        RESERVE_AMOUNT * INITIAL_COLLATERAL_RATIO
    );
}

#[tokio::test]
async fn test_already_initialized() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_pyth_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            liquidity_amount: 42,
            liquidity_mint_decimals: usdc_mint.decimals,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            config: TEST_RESERVE_CONFIG,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[init_reserve(
            port_finance_variable_rate_lending::id(),
            42,
            COption::None,
            usdc_test_reserve.config,
            usdc_test_reserve.user_liquidity_pubkey,
            usdc_test_reserve.user_collateral_pubkey,
            usdc_test_reserve.pubkey,
            usdc_test_reserve.liquidity_mint_pubkey,
            usdc_test_reserve.liquidity_supply_pubkey,
            usdc_test_reserve.liquidity_fee_receiver_pubkey,
            usdc_test_reserve.collateral_mint_pubkey,
            usdc_test_reserve.collateral_supply_pubkey,
            lending_market.pubkey,
            lending_market.owner.pubkey(),
            user_transfer_authority.pubkey(),
            COption::Some(usdc_oracle.price_pubkey),
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(
        &[&payer, &lending_market.owner, &user_transfer_authority],
        recent_blockhash,
    );
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::AlreadyInitialized as u32)
        )
    );
}

#[tokio::test]
async fn test_invalid_fees() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);
    let sol_oracle = add_sol_pyth_oracle(&mut test);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;

    const RESERVE_AMOUNT: u64 = 42;

    let sol_user_liquidity_account = create_and_mint_to_token_account(
        &mut banks_client,
        spl_token::native_mint::id(),
        None,
        &payer,
        user_accounts_owner.pubkey(),
        RESERVE_AMOUNT,
    )
    .await;

    // fee above 100%
    {
        let mut config = TEST_RESERVE_CONFIG;
        config.fees = ReserveFees {
            borrow_fee_wad: 1_000_000_000_000_000_001,
            flash_loan_fee_wad: 1_000_000_000_000_000_001,
            host_fee_percentage: 0,
        };

        assert_eq!(
            TestReserve::init(
                "sol".to_owned(),
                &mut banks_client,
                &lending_market,
                &COption::Some(sol_oracle),
                RESERVE_AMOUNT,
                COption::None,
                config,
                spl_token::native_mint::id(),
                sol_user_liquidity_account,
                &payer,
                &user_accounts_owner,
            )
            .await
            .unwrap_err(),
            TransactionError::InstructionError(
                8,
                InstructionError::Custom(LendingError::InvalidConfig as u32)
            )
        );
    }

    // host fee above 100%
    {
        let mut config = TEST_RESERVE_CONFIG;
        config.fees = ReserveFees {
            borrow_fee_wad: 10_000_000_000_000_000,
            flash_loan_fee_wad: 10_000_000_000_000_000,
            host_fee_percentage: 101,
        };

        assert_eq!(
            TestReserve::init(
                "sol".to_owned(),
                &mut banks_client,
                &lending_market,
                &COption::Some(sol_oracle),
                RESERVE_AMOUNT,
                COption::None,
                config,
                spl_token::native_mint::id(),
                sol_user_liquidity_account,
                &payer,
                &user_accounts_owner,
            )
            .await
            .unwrap_err(),
            TransactionError::InstructionError(
                8,
                InstructionError::Custom(LendingError::InvalidConfig as u32)
            )
        );
    }
}

#[tokio::test]
async fn test_fixed_price() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(80_000);

    let user_accounts_owner = Keypair::new();
    let usdc_mint = add_usdc_mint(&mut test);
    let lending_market = add_lending_market(&mut test);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;

    const RESERVE_AMOUNT: u64 = 42;

    let usdc_user_liquidity_account = create_and_mint_to_token_account(
        &mut banks_client,
        usdc_mint.pubkey,
        Some(&usdc_mint.authority),
        &payer,
        user_accounts_owner.pubkey(),
        RESERVE_AMOUNT,
    )
    .await;

    let usdc_reserve = TestReserve::init(
        "usdc".to_owned(),
        &mut banks_client,
        &lending_market,
        &COption::None,
        RESERVE_AMOUNT,
        COption::Some(Decimal::one()),
        TEST_RESERVE_CONFIG,
        usdc_mint.pubkey,
        usdc_user_liquidity_account,
        &payer,
        &user_accounts_owner,
    )
    .await
    .unwrap();

    usdc_reserve.validate_state(&mut banks_client).await;

    let usdc_liquidity_supply =
        get_token_balance(&mut banks_client, usdc_reserve.liquidity_supply_pubkey).await;
    assert_eq!(usdc_liquidity_supply, RESERVE_AMOUNT);
    let user_usdc_balance =
        get_token_balance(&mut banks_client, usdc_reserve.user_liquidity_pubkey).await;
    assert_eq!(user_usdc_balance, 0);
    let user_usdc_collateral_balance =
        get_token_balance(&mut banks_client, usdc_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_usdc_collateral_balance,
        RESERVE_AMOUNT * INITIAL_COLLATERAL_RATIO
    );
}
