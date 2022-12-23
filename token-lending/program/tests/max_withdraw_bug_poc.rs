#![cfg(feature = "test-bpf")]

use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction::create_account,
    transaction::Transaction,
};
use spl_token::solana_program::program_option::COption;
use spl_token::{instruction::approve, solana_program::program_pack::Pack};

use helpers::*;
use port_finance_variable_rate_lending::{
    instruction::{
        borrow_obligation_liquidity, deposit_obligation_collateral, init_obligation,
        liquidate_obligation, refresh_obligation, refresh_reserve, withdraw_obligation_collateral,
    },
    processor::process_instruction,
    state::{Obligation, ReserveConfig, ReserveFees, INITIAL_COLLATERAL_RATIO},
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
    test.set_bpf_compute_max_units(2_000_000);

    // note for ease of computation we are using SOL to be valued at $1
    const SOL_DEPOSIT_AMOUNT_LAMPORTS: u64 = 120 * LAMPORTS_TO_SOL * INITIAL_COLLATERAL_RATIO;
    const SOL_RESERVE_COLLATERAL_LAMPORTS: u64 = SOL_DEPOSIT_AMOUNT_LAMPORTS;

    const FEE_AMOUNT: u64 = 100;
    const USDC_DEPOSIT_AMOUNT_LAMPORTS: u64 = 100_000 * FRACTIONAL_TO_USDC;
    const USDC_BORROW_AMOUNT_FRACTIONAL: u64 = 100 * FRACTIONAL_TO_USDC - FEE_AMOUNT;
    const USDC_RESERVE_LIQUIDITY_FRACTIONAL: u64 = 1_000_000 * FRACTIONAL_TO_USDC;

    let user_accounts_owner = Keypair::new();
    let user_accounts_owner_pubkey = user_accounts_owner.pubkey();

    let user_transfer_authority = Keypair::new();
    let user_transfer_authority_pubkey = user_transfer_authority.pubkey();

    let obligation_keypair = Keypair::new();
    let obligation_pubkey = obligation_keypair.pubkey();

    let lending_market = add_lending_market(&mut test);

    let sol_reserve_config: ReserveConfig = ReserveConfig {
        optimal_utilization_rate: 80,
        // note these mirror production SBR liq configs
        loan_to_value_ratio: 35,
        liquidation_bonus: 30,
        liquidation_threshold: 40,
        min_borrow_rate: 0,
        optimal_borrow_rate: 4,
        max_borrow_rate: 30,
        fees: ReserveFees {
            borrow_fee_wad: 100_000_000_000,
            /// 0.00001% (Aave borrow fee)
            flash_loan_fee_wad: 3_000_000_000_000_000,
            /// 0.3% (Aave flash loan fee)
            host_fee_percentage: 20,
        },
        deposit_staking_pool: COption::None,
        deposit_limit: 18446744073709551615,
        borrow_limit: 18446744073709551615,
    };

    // oracle price doesn't matter so using usdc oracle for ease of computation
    let sol_oracle = add_usdc_pyth_oracle(&mut test);
    let sol_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &sol_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_amount: SOL_RESERVE_COLLATERAL_LAMPORTS,
            liquidity_mint_pubkey: spl_token::native_mint::id(),
            liquidity_mint_decimals: 9,
            config: sol_reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let usdc_reserve_config: ReserveConfig = ReserveConfig {
        optimal_utilization_rate: 80,
        // note these mirror production USDC liq configs
        loan_to_value_ratio: 85,
        liquidation_bonus: 3,
        liquidation_threshold: 90,
        min_borrow_rate: 0,
        optimal_borrow_rate: 4,
        max_borrow_rate: 30,
        fees: ReserveFees {
            borrow_fee_wad: 100_000_000_000,
            /// 0.00001% (Aave borrow fee)
            flash_loan_fee_wad: 3_000_000_000_000_000,
            /// 0.3% (Aave flash loan fee)
            host_fee_percentage: 20,
        },
        deposit_staking_pool: COption::None,
        deposit_limit: 18446744073709551615,
        borrow_limit: 18446744073709551615,
    };
    let usdc_mint = add_usdc_mint(&mut test);
    let usdc_oracle = add_usdc_pyth_oracle(&mut test);
    let usdc_test_reserve = add_reserve(
        &mut test,
        &lending_market,
        &usdc_oracle,
        &user_accounts_owner,
        AddReserveArgs {
            user_liquidity_amount: USDC_DEPOSIT_AMOUNT_LAMPORTS,
            liquidity_amount: USDC_RESERVE_LIQUIDITY_FRACTIONAL,
            liquidity_mint_pubkey: usdc_mint.pubkey,
            liquidity_mint_decimals: usdc_mint.decimals,
            config: usdc_reserve_config,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;
    let payer_pubkey = payer.pubkey();

    let rent = banks_client.get_rent().await.unwrap();

    // this works by depositing SOL and USDC. borrowing USDC then withdrawing USDC
    // at which time the supplys will still be greater than the borrows but the obligation will be
    // liquidatable and when you factor in the liquidation bonus we are able to withdraw
    // all the supplys without repaying all the debt.

    let mut transaction = Transaction::new_with_payer(
        &[
            // 0
            create_account(
                &payer.pubkey(),
                &obligation_keypair.pubkey(),
                rent.minimum_balance(Obligation::LEN),
                Obligation::LEN as u64,
                &port_finance_variable_rate_lending::id(),
            ),
            // 1
            init_obligation(
                port_finance_variable_rate_lending::id(),
                obligation_pubkey,
                lending_market.pubkey,
                user_accounts_owner_pubkey,
            ),
            // 2
            refresh_reserve(
                port_finance_variable_rate_lending::id(),
                sol_test_reserve.pubkey,
                COption::Some(sol_oracle.price_pubkey),
            ),
            // 3
            approve(
                &spl_token::id(),
                &sol_test_reserve.user_collateral_pubkey,
                &user_transfer_authority_pubkey,
                &user_accounts_owner_pubkey,
                &[],
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            // 4
            deposit_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                SOL_DEPOSIT_AMOUNT_LAMPORTS,
                sol_test_reserve.user_collateral_pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                sol_test_reserve.pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_accounts_owner_pubkey,
                user_transfer_authority_pubkey,
                None,
                None,
            ),
            // 5
            refresh_reserve(
                port_finance_variable_rate_lending::id(),
                usdc_test_reserve.pubkey,
                COption::Some(usdc_oracle.price_pubkey),
            ),
            // 6
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_collateral_pubkey,
                &user_transfer_authority_pubkey,
                &user_accounts_owner_pubkey,
                &[],
                USDC_DEPOSIT_AMOUNT_LAMPORTS,
            )
            .unwrap(),
            // 7
            deposit_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                USDC_DEPOSIT_AMOUNT_LAMPORTS,
                usdc_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.collateral_supply_pubkey,
                usdc_test_reserve.pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_accounts_owner_pubkey,
                user_transfer_authority_pubkey,
                None,
                None,
            ),
            // 8
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                obligation_pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            // 9
            refresh_reserve(
                port_finance_variable_rate_lending::id(),
                usdc_test_reserve.pubkey,
                COption::Some(usdc_oracle.price_pubkey),
            ),
            // 10
            borrow_obligation_liquidity(
                port_finance_variable_rate_lending::id(),
                USDC_BORROW_AMOUNT_FRACTIONAL,
                usdc_test_reserve.liquidity_supply_pubkey,
                usdc_test_reserve.user_liquidity_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_fee_receiver_pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_accounts_owner_pubkey,
            ),
            // 11
            refresh_reserve(
                port_finance_variable_rate_lending::id(),
                usdc_test_reserve.pubkey,
                COption::Some(usdc_oracle.price_pubkey),
            ),
            // 12
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                obligation_pubkey,
                vec![
                    sol_test_reserve.pubkey,
                    usdc_test_reserve.pubkey,
                    usdc_test_reserve.pubkey,
                ],
            ),
            // 13
            withdraw_obligation_collateral(
                port_finance_variable_rate_lending::id(),
                USDC_DEPOSIT_AMOUNT_LAMPORTS,
                usdc_test_reserve.collateral_supply_pubkey,
                usdc_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_accounts_owner_pubkey,
                None,
                None,
            ),
            // need to liquidate several times to clean obligation out
            refresh_reserve(
                port_finance_variable_rate_lending::id(),
                usdc_test_reserve.pubkey,
                COption::Some(usdc_oracle.price_pubkey),
            ),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                obligation_pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority_pubkey,
                &user_accounts_owner_pubkey,
                &[],
                USDC_BORROW_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                u64::MAX,
                usdc_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_transfer_authority_pubkey,
                None,
                None,
            ),
            refresh_reserve(
                port_finance_variable_rate_lending::id(),
                usdc_test_reserve.pubkey,
                COption::Some(usdc_oracle.price_pubkey),
            ),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                obligation_pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority_pubkey,
                &user_accounts_owner_pubkey,
                &[],
                USDC_BORROW_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                u64::MAX,
                usdc_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_transfer_authority_pubkey,
                None,
                None,
            ),
            refresh_reserve(
                port_finance_variable_rate_lending::id(),
                usdc_test_reserve.pubkey,
                COption::Some(usdc_oracle.price_pubkey),
            ),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                obligation_pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority_pubkey,
                &user_accounts_owner_pubkey,
                &[],
                USDC_BORROW_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                u64::MAX,
                usdc_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_transfer_authority_pubkey,
                None,
                None,
            ),
            refresh_reserve(
                port_finance_variable_rate_lending::id(),
                usdc_test_reserve.pubkey,
                COption::Some(usdc_oracle.price_pubkey),
            ),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                obligation_pubkey,
                vec![sol_test_reserve.pubkey, usdc_test_reserve.pubkey],
            ),
            approve(
                &spl_token::id(),
                &usdc_test_reserve.user_liquidity_pubkey,
                &user_transfer_authority_pubkey,
                &user_accounts_owner_pubkey,
                &[],
                USDC_BORROW_AMOUNT_FRACTIONAL,
            )
            .unwrap(),
            liquidate_obligation(
                port_finance_variable_rate_lending::id(),
                u64::MAX,
                usdc_test_reserve.user_liquidity_pubkey,
                sol_test_reserve.user_collateral_pubkey,
                usdc_test_reserve.pubkey,
                usdc_test_reserve.liquidity_supply_pubkey,
                sol_test_reserve.pubkey,
                sol_test_reserve.collateral_supply_pubkey,
                obligation_pubkey,
                lending_market.pubkey,
                user_transfer_authority_pubkey,
                None,
                None,
            ),
            // just refreshing here so we can see the value amounts on the obligation after
            refresh_reserve(
                port_finance_variable_rate_lending::id(),
                usdc_test_reserve.pubkey,
                COption::Some(usdc_oracle.price_pubkey),
            ),
            refresh_obligation(
                port_finance_variable_rate_lending::id(),
                obligation_pubkey,
                vec![usdc_test_reserve.pubkey],
            ),
        ],
        Some(&payer_pubkey),
    );

    transaction.sign(
        &vec![
            &payer,
            &obligation_keypair,
            &user_accounts_owner,
            &user_transfer_authority,
        ],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(transaction).await.is_err());
}
