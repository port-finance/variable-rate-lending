#![cfg(feature = "test-bpf")]

mod helpers;

use helpers::*;
use port_finance_variable_rate_lending::{
    error::LendingError, instruction::init_lending_market, processor::process_instruction,
};
use solana_program_test::*;
use solana_sdk::{
    instruction::InstructionError,
    pubkey::Pubkey,
    signature::Signer,
    transaction::{Transaction, TransactionError},
};

#[tokio::test]
async fn test_success() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    // limit to track compute unit increase
    test.set_compute_max_units(80_000);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;

    let test_lending_market = TestLendingMarket::init(&mut banks_client, &payer).await;

    test_lending_market.validate_state(&mut banks_client).await;
}

#[tokio::test]
async fn test_already_initialized() {
    let mut test = ProgramTest::new(
        "port_finance_variable_rate_lending",
        port_finance_variable_rate_lending::id(),
        processor!(process_instruction),
    );

    let existing_market = add_lending_market(&mut test);
    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[init_lending_market(
            port_finance_variable_rate_lending::id(),
            existing_market.owner.pubkey(),
            existing_market.quote_currency,
            existing_market.pubkey,
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer], recent_blockhash);
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
