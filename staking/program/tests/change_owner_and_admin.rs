#![cfg(feature = "test-bpf")]

mod helpers;
use helpers::*;
use port_finance_staking::error::StakingError;
use port_finance_staking::solana_program::instruction::InstructionError;
use solana_program::pubkey::Pubkey;
use solana_program_test::*;
use solana_sdk::transaction::TransactionError;

#[tokio::test]
async fn test_change_owner() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    let new_owner = Pubkey::new_unique();
    staking_pool
        .change_owner(&mut banks_client, new_owner, &payer, true)
        .await
        .unwrap();
    assert_eq!(staking_pool.staking_pool.owner_authority, new_owner);
    staking_pool.validate_state(&mut banks_client).await;
}

#[tokio::test]
async fn test_change_owner_fail() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    let new_owner = Pubkey::new_unique();
    let err = staking_pool
        .change_owner(&mut banks_client, new_owner, &payer, false)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(StakingError::InvalidSigner as u32)
        )
    );
    staking_pool.validate_state(&mut banks_client).await;
}

#[tokio::test]
async fn test_change_admin() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    let new_admin = Pubkey::new_unique();
    staking_pool
        .change_admin(&mut banks_client, new_admin, &payer, true)
        .await
        .unwrap();
    assert_eq!(staking_pool.staking_pool.admin_authority, new_admin);
    staking_pool.validate_state(&mut banks_client).await;
}

#[tokio::test]
async fn test_change_admin_fail() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);

    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    let new_admin = Pubkey::new_unique();
    let err = staking_pool
        .change_admin(&mut banks_client, new_admin, &payer, false)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(StakingError::InvalidSigner as u32)
        )
    );
    staking_pool.validate_state(&mut banks_client).await;
}
