#![cfg(feature = "test-bpf")]

use solana_program::instruction::InstructionError;
use solana_program_test::*;
use solana_sdk::transaction::TransactionError;
use solana_sdk::{
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use spl_token::instruction::transfer;

use helpers::*;
use port_finance_variable_rate_lending::error::LendingError;
use port_finance_variable_rate_lending::instruction::withdraw_fee;
use port_finance_variable_rate_lending::processor::process_instruction;

mod helpers;

#[tokio::test]
async fn test_withdraw_fee() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    let wrong_lending_market = add_lending_market(&mut test);
    let user_accounts_owner = Keypair::new();
    let lending_market = add_lending_market(&mut test);
    const USER_LIQUIDITY_AMOUNT: u64 = 200;
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
            user_liquidity_amount: USER_LIQUIDITY_AMOUNT,
            config: TEST_RESERVE_CONFIG,
            ..AddReserveArgs::default()
        },
    );

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[transfer(
            &spl_token::id(),
            &usdc_test_reserve.user_liquidity_pubkey,
            &usdc_test_reserve.liquidity_fee_receiver_pubkey,
            &user_accounts_owner.pubkey(),
            &[],
            USER_LIQUIDITY_AMOUNT,
        )
        .unwrap()],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);
    assert!(banks_client.process_transaction(transaction).await.is_ok());

    let mut transaction = Transaction::new_with_payer(
        &[withdraw_fee(
            port_finance_variable_rate_lending::id(),
            usdc_test_reserve.pubkey,
            lending_market.pubkey,
            lending_market.owner.pubkey(),
            usdc_test_reserve.liquidity_fee_receiver_pubkey,
            usdc_test_reserve.user_liquidity_pubkey,
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer, &lending_market.owner], recent_blockhash);
    banks_client.process_transaction(transaction).await.unwrap();

    let mut transaction = Transaction::new_with_payer(
        &[withdraw_fee(
            port_finance_variable_rate_lending::id(),
            usdc_test_reserve.pubkey,
            wrong_lending_market.pubkey,
            wrong_lending_market.owner.pubkey(),
            usdc_test_reserve.liquidity_fee_receiver_pubkey,
            usdc_test_reserve.user_liquidity_pubkey,
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer, &wrong_lending_market.owner], recent_blockhash);
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

    let mut transaction = Transaction::new_with_payer(
        &[withdraw_fee(
            port_finance_variable_rate_lending::id(),
            usdc_test_reserve.pubkey,
            lending_market.pubkey,
            user_accounts_owner.pubkey(),
            usdc_test_reserve.liquidity_fee_receiver_pubkey,
            usdc_test_reserve.user_liquidity_pubkey,
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer, &user_accounts_owner], recent_blockhash);
    assert_eq!(
        banks_client
            .process_transaction(transaction)
            .await
            .unwrap_err()
            .unwrap(),
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(LendingError::InvalidMarketOwner as u32),
        )
    );
}
