#![cfg(feature = "test-bpf")]

mod helpers;

use helpers::*;
use port_finance_staking::error::StakingError;
use port_finance_staking::instruction::create_stake_account;
use port_finance_staking::solana_program::instruction::InstructionError;
use solana_program_test::*;
use solana_sdk::{
    pubkey::Pubkey,
    signature::Signer,
    transaction::{Transaction, TransactionError},
};

#[tokio::test]
async fn test_success() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(10_000);
    let staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;

    let stake_account = TestStakeAccount::create(
        "Test Init Stake Account".to_owned(),
        &mut banks_client,
        staking_pool.pubkey,
        &payer,
    )
    .await
    .unwrap();

    stake_account.validate_state(&mut banks_client).await;
}

#[tokio::test]
async fn test_already_initialized() {
    let mut test = staking_test!();
    // limit to track compute unit increase
    test.set_compute_max_units(20_000);

    let staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);
    let stake_account: TestStakeAccount = add_stake_account(&mut test, staking_pool.pubkey);

    let (mut banks_client, payer, recent_blockhash) = test.start().await;
    let mut transaction = Transaction::new_with_payer(
        &[create_stake_account(
            port_finance_staking::id(),
            stake_account.pubkey,
            staking_pool.pubkey,
            stake_account.owner.pubkey(),
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
            InstructionError::Custom(StakingError::AlreadyInitialized as u32)
        )
    );
}
