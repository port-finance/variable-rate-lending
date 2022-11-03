#![cfg(feature = "test-bpf")]

use solana_program::instruction::InstructionError;
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
use port_finance_staking::solana_program::program_option::COption;
use port_finance_variable_rate_lending::error::LendingError;
use port_finance_variable_rate_lending::{
    instruction::{liquidate_obligation, refresh_obligation},
    processor::process_instruction,
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
    test.set_bpf_compute_max_units(90_000);

    // 100 SOL collateral
    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    // 100 SOL * 80% LTV -> 80 SOL * 20 USDC -> 1600 USDC borrow
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_600 * FRACTIONAL_TO_USDC;
    // 1600 USDC * 50% -> 800 USDC liquidation
    const USDC_LIQUIDATION_AMOUNT_FRACTIONAL: u64 = USDC_BORROW_AMOUNT_FRACTIONAL / 2;
    // 800 USDC / 20 USDC per SOL -> 40 SOL + 10% bonus -> 44 SOL
    const SOL_LIQUIDATION_AMOUNT_LAMPORTS: u64 = 44 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;

    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const USDC_RESERVE_LIQUIDITY_FRACTIONAL: u64 = 2 * USDC_BORROW_AMOUNT_FRACTIONAL;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = TEST_RESERVE_CONFIG;
    reserve_config.loan_to_value_ratio = 50;
    reserve_config.liquidation_threshold = 80;
    reserve_config.liquidation_bonus = 10;

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
            user_liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_RESERVE_LIQUIDITY_FRACTIONAL,
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

    let initial_user_liquidity_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    let initial_liquidity_supply_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    let initial_user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    let initial_collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
                usdc_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                user_transfer_authority.pubkey(),
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let user_liquidity_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(
        user_liquidity_balance,
        initial_user_liquidity_balance - USDC_LIQUIDATION_AMOUNT_FRACTIONAL
    );

    let liquidity_supply_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(
        liquidity_supply_balance,
        initial_liquidity_supply_balance + USDC_LIQUIDATION_AMOUNT_FRACTIONAL
    );

    let user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_collateral_balance,
        initial_user_collateral_balance + SOL_LIQUIDATION_AMOUNT_LAMPORTS
    );

    let collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    assert_eq!(
        collateral_supply_balance,
        initial_collateral_supply_balance - SOL_LIQUIDATION_AMOUNT_LAMPORTS
    );

    let obligation = test_obligation.get_state(&mut banks_client).await;
    assert_eq!(
        obligation.deposits[0].deposited_amount,
        SOL_DEPOSIT_AMOUNT_LAMPORTS - SOL_LIQUIDATION_AMOUNT_LAMPORTS
    );
    assert_eq!(
        obligation.borrows[0].borrowed_amount_wads,
        (USDC_BORROW_AMOUNT_FRACTIONAL - USDC_LIQUIDATION_AMOUNT_FRACTIONAL).into()
    )
}

#[tokio::test]
async fn test_success_one_lamport() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(90_000);

    // 1 USDC Collateral
    const USDC_DEPOSIT_AMOUNT_LAMPORTS: u64 = 1;
    // 40 SOL = 0.8 USDC borrow
    const SOL_BORROW_AMOUNT_FRACTIONAL: u64 = 40;
    // \
    const SOL_LIQUIDATION_AMOUNT_FRACTIONAL: u64 = 20;
    //
    const USDC_LIQUIDATION_AMOUNT_LAMPORTS: u64 = 1;

    const USDC_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * USDC_DEPOSIT_AMOUNT_LAMPORTS;
    const SOL_RESERVE_LIQUIDITY_FRACTIONAL: u64 = 2 * SOL_BORROW_AMOUNT_FRACTIONAL;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = TEST_RESERVE_CONFIG;
    reserve_config.loan_to_value_ratio = 50;
    reserve_config.liquidation_threshold = 80;
    reserve_config.liquidation_bonus = 10;

    let sol_oracle = add_sol_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            borrow_amount: SOL_BORROW_AMOUNT_FRACTIONAL,
            user_liquidity_amount: SOL_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: SOL_RESERVE_LIQUIDITY_FRACTIONAL,
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
            collateral_amount: USDC_RESERVE_COLLATERAL_LAMPORTS,
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
            deposits: &[(&usdc_test_reserve, USDC_DEPOSIT_AMOUNT_LAMPORTS)],
            borrows: &[(&sol_test_reserve, SOL_BORROW_AMOUNT_FRACTIONAL)],
            ..AddObligationArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let initial_user_liquidity_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_liquidity_pubkey).await;
    let initial_liquidity_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.liquidity_supply_pubkey).await;
    let initial_user_collateral_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_collateral_pubkey).await;
    let initial_collateral_supply_balance = get_token_balance(
        &mut banks_client,
        usdc_test_reserve.collateral_supply_pubkey,
    )
    .await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                SOL_LIQUIDATION_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![usdc_test_reserve.pubkey, sol_test_reserve.pubkey],
            ),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                SOL_LIQUIDATION_AMOUNT_FRACTIONAL,
                sol_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.user_collateral_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                user_transfer_authority.pubkey(),
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    banks_client.process_transaction(transaction).await.unwrap();

    let user_liquidity_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(
        user_liquidity_balance,
        initial_user_liquidity_balance - SOL_LIQUIDATION_AMOUNT_FRACTIONAL
    );

    let liquidity_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(
        liquidity_supply_balance,
        initial_liquidity_supply_balance + SOL_LIQUIDATION_AMOUNT_FRACTIONAL
    );

    let user_collateral_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_collateral_balance,
        initial_user_collateral_balance + USDC_LIQUIDATION_AMOUNT_LAMPORTS
    );

    let collateral_supply_balance = get_token_balance(
        &mut banks_client,
        usdc_test_reserve.collateral_supply_pubkey,
    )
    .await;
    assert_eq!(
        collateral_supply_balance,
        initial_collateral_supply_balance - USDC_LIQUIDATION_AMOUNT_LAMPORTS
    );

    let obligation = test_obligation.get_state(&mut banks_client).await;
    assert_eq!(obligation.deposits.len(), 0);
    assert_eq!(obligation.deposits.len(), 0)
}

#[tokio::test]
async fn test_success_with_staking() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(150_000);

    // 100 SOL collateral
    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    // 100 SOL * 80% LTV -> 80 SOL * 20 USDC -> 1600 USDC borrow
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_600 * FRACTIONAL_TO_USDC;
    // 1600 USDC * 50% -> 800 USDC liquidation
    const USDC_LIQUIDATION_AMOUNT_FRACTIONAL: u64 = USDC_BORROW_AMOUNT_FRACTIONAL / 2;
    // 800 USDC / 20 USDC per SOL -> 40 SOL + 10% bonus -> 44 SOL
    const SOL_LIQUIDATION_AMOUNT_LAMPORTS: u64 = 44 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;

    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const USDC_RESERVE_LIQUIDITY_FRACTIONAL: u64 = 2 * USDC_BORROW_AMOUNT_FRACTIONAL;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = TEST_RESERVE_CONFIG;
    reserve_config.loan_to_value_ratio = 50;
    reserve_config.liquidation_threshold = 80;
    reserve_config.liquidation_bonus = 10;

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

    reserve_config.deposit_staking_pool = COption::Some(staking_pool.staking_pool_pubkey);
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
            user_liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_RESERVE_LIQUIDITY_FRACTIONAL,
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

    let initial_user_liquidity_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    let initial_liquidity_supply_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    let initial_user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    let initial_collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;

    let mut transaction = Transaction::new_with_payer(
        &[
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
                usdc_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                user_transfer_authority.pubkey(),
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer, &user_accounts_owner, &user_transfer_authority],
        recent_blockhash,
    );
    banks_client.process_transaction(transaction).await.unwrap();

    let user_liquidity_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.user_liquidity_pubkey).await;
    assert_eq!(
        user_liquidity_balance,
        initial_user_liquidity_balance - USDC_LIQUIDATION_AMOUNT_FRACTIONAL
    );

    let liquidity_supply_balance =
        get_token_balance(&mut banks_client, usdc_test_reserve.liquidity_supply_pubkey).await;
    assert_eq!(
        liquidity_supply_balance,
        initial_liquidity_supply_balance + USDC_LIQUIDATION_AMOUNT_FRACTIONAL
    );

    let user_collateral_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.user_collateral_pubkey).await;
    assert_eq!(
        user_collateral_balance,
        initial_user_collateral_balance + SOL_LIQUIDATION_AMOUNT_LAMPORTS
    );

    let collateral_supply_balance =
        get_token_balance(&mut banks_client, sol_test_reserve.collateral_supply_pubkey).await;
    assert_eq!(
        collateral_supply_balance,
        initial_collateral_supply_balance - SOL_LIQUIDATION_AMOUNT_LAMPORTS
    );

    let obligation = test_obligation.get_state(&mut banks_client).await;
    assert_eq!(
        obligation.deposits[0].deposited_amount,
        SOL_DEPOSIT_AMOUNT_LAMPORTS - SOL_LIQUIDATION_AMOUNT_LAMPORTS
    );
    assert_eq!(
        obligation.borrows[0].borrowed_amount_wads,
        (USDC_BORROW_AMOUNT_FRACTIONAL - USDC_LIQUIDATION_AMOUNT_FRACTIONAL).into()
    );

    let account = stake_account.get_state(&mut banks_client).await;
    assert_eq!(
        account.deposited_amount,
        SOL_DEPOSIT_AMOUNT_LAMPORTS - SOL_LIQUIDATION_AMOUNT_LAMPORTS
    );
    let pool = staking_pool.get_state(&mut banks_client).await;
    assert_eq!(
        pool.pool_size,
        SOL_DEPOSIT_AMOUNT_LAMPORTS - SOL_LIQUIDATION_AMOUNT_LAMPORTS
    );
}

#[tokio::test]
async fn test_success_with_staking_fail_wrong_owner() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    test.add_program(
        "port_finance_staking",
        port_finance_staking::id(),
        processor!(port_finance_staking::processor::process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(81_000);

    // 100 SOL collateral
    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    // 100 SOL * 80% LTV -> 80 SOL * 20 USDC -> 1600 USDC borrow
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_600 * FRACTIONAL_TO_USDC;
    // 1600 USDC * 50% -> 800 USDC liquidation
    const USDC_LIQUIDATION_AMOUNT_FRACTIONAL: u64 = USDC_BORROW_AMOUNT_FRACTIONAL / 2;

    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const USDC_RESERVE_LIQUIDITY_FRACTIONAL: u64 = 2 * USDC_BORROW_AMOUNT_FRACTIONAL;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let mut reserve_config = TEST_RESERVE_CONFIG;
    reserve_config.loan_to_value_ratio = 50;
    reserve_config.liquidation_threshold = 80;
    reserve_config.liquidation_bonus = 10;

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

    reserve_config.deposit_staking_pool = COption::Some(staking_pool.staking_pool_pubkey);
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
            user_liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_RESERVE_LIQUIDITY_FRACTIONAL,
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
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
                usdc_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                user_transfer_authority.pubkey(),
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer, &user_accounts_owner, &user_transfer_authority],
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
async fn test_fail() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(51_000);

    // 100 SOL collateral
    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    // 100 SOL * 80% LTV -> 80 SOL * 20 USDC -> 1600 USDC borrow
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_600 * FRACTIONAL_TO_USDC;
    // 1600 USDC * 50% -> 800 USDC liquidation
    const USDC_LIQUIDATION_AMOUNT_FRACTIONAL: u64 = USDC_BORROW_AMOUNT_FRACTIONAL / 2;
    // 800 USDC / 20 USDC per SOL -> 40 SOL + 10% bonus -> 44 SOL
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const USDC_RESERVE_LIQUIDITY_FRACTIONAL: u64 = 2 * USDC_BORROW_AMOUNT_FRACTIONAL;

    let user_accounts_owner = Keypair::new();
    let user_transfer_authority = Keypair::new();
    let lending_market = add_lending_market(&mut test);

    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market.pubkey.to_bytes()[..PUBKEY_BYTES]],
        &port_finance_variable_rate_lending::id(),
    );
    let staking_pool = add_staking_pool(&mut test, lending_market_authority_pubkey, 0);

    let mut reserve_config = TEST_RESERVE_CONFIG;
    reserve_config.loan_to_value_ratio = 50;
    reserve_config.liquidation_threshold = 80;
    reserve_config.liquidation_bonus = 10;
    reserve_config.deposit_staking_pool = COption::Some(staking_pool.staking_pool_pubkey);
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

    reserve_config.deposit_staking_pool = COption::None;
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            borrow_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            user_liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_RESERVE_LIQUIDITY_FRACTIONAL,
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
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
                usdc_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                user_transfer_authority.pubkey(),
                None,
                None,
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer, &user_accounts_owner, &user_transfer_authority],
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
async fn test_fail2() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_bpf_compute_max_units(51_000);

    // 100 SOL collateral
    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 100 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    // 100 SOL * 80% LTV -> 80 SOL * 20 USDC -> 1600 USDC borrow
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 1_600 * FRACTIONAL_TO_USDC;
    // 1600 USDC * 50% -> 800 USDC liquidation
    const USDC_LIQUIDATION_AMOUNT_FRACTIONAL: u64 = USDC_BORROW_AMOUNT_FRACTIONAL / 2;
    // 800 USDC / 20 USDC per SOL -> 40 SOL + 10% bonus -> 44 SOL
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = 2 * SOL_DEPOSIT_AMOUNT_LAMPORTS;
    const USDC_RESERVE_LIQUIDITY_FRACTIONAL: u64 = 2 * USDC_BORROW_AMOUNT_FRACTIONAL;

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

    let mut reserve_config = TEST_RESERVE_CONFIG;
    reserve_config.loan_to_value_ratio = 50;
    reserve_config.liquidation_threshold = 80;
    reserve_config.liquidation_bonus = 10;
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
            user_liquidity_amount: USDC_BORROW_AMOUNT_FRACTIONAL,
            liquidity_amount: USDC_RESERVE_LIQUIDITY_FRACTIONAL,
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
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority.pubkey(),
                &user_accounts_owner.pubkey(),
                &[],
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                test_obligation.pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                USDC_LIQUIDATION_AMOUNT_FRACTIONAL,
                usdc_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                test_obligation.pubkey,
                lending_market.pubkey,
                user_transfer_authority.pubkey(),
                Some(stake_account.pubkey),
                Some(staking_pool.staking_pool_pubkey),
            ),
        ],
        Some(&payer.pubkey()),
    );

    transaction.sign(
        &[&payer, &user_accounts_owner, &user_transfer_authority],
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
