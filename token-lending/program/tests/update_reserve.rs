#![cfg(feature = "test-bpf")]

use solana_program::instruction::InstructionError;
use solana_program_test::*;
use solana_sdk::program_option::COption;
use solana_sdk::transaction::TransactionError;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};

use helpers::*;
use port_finance_variable_rate_lending::error::LendingError;
use port_finance_variable_rate_lending::instruction::update_reserve;
use port_finance_variable_rate_lending::state::ReserveConfig;
use port_finance_variable_rate_lending::{processor::process_instruction, state::ReserveFees};

mod helpers;

#[tokio::test]
async fn test_update_reserve() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    let lending_market2 = add_lending_market(&mut test);
    let user_accounts_owner = Keypair::new();
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
    let new_config = ReserveConfig {
        optimal_utilization_rate: 75,
        loan_to_value_ratio: 45,
        liquidation_bonus: 10,
        liquidation_threshold: 65,
        min_borrow_rate: 1,
        optimal_borrow_rate: 5,
        max_borrow_rate: 45,
        fees: ReserveFees {
            borrow_fee_wad: 200_000_000_000,
            flash_loan_fee_wad: 5_000_000_000_000_000,
            host_fee_percentage: 15,
        },
        deposit_staking_pool: COption::None,
    };
    let before_test_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    assert_ne!(before_test_reserve.config, new_config);

    let mut transaction = Transaction::new_with_payer(
        &[update_reserve(
            port_finance_variable_rate_lending::id(),
            new_config,
            usdc_test_reserve.pubkey,
            lending_market.pubkey,
            lending_market.owner.pubkey(),
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer, &lending_market.owner], recent_blockhash);
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let test_reserve = usdc_test_reserve.get_state(&mut banks_client).await;
    assert_eq!(test_reserve.config, new_config);

    let mut transaction = Transaction::new_with_payer(
        &[update_reserve(
            port_finance_variable_rate_lending::id(),
            new_config,
            usdc_test_reserve.pubkey,
            lending_market2.pubkey,
            lending_market2.owner.pubkey(),
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer, &lending_market2.owner], recent_blockhash);
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::InvalidAccountInput as u32),
        )
    );
}
