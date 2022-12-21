#![cfg(feature = "test-bpf")]

use solana_program_test::*;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::transaction::TransactionError;

use helpers::*;
use port_finance_staking::error::StakingError;
use port_finance_staking::solana_program::clock::Slot;
use port_finance_staking::solana_program::instruction::InstructionError;

mod helpers;

#[tokio::test]
async fn deposit() {
    let mut test = staking_test!();
    test.set_compute_max_units(15200);

    const AMOUNT: u64 = 10;
    const SLOT: Slot = 10;
    const EARLIEST_CLAIM_SLOT: Slot = 0;
    const SUPPLY: u64 = 100;
    const DURATION: Slot = 1000;
    let mut staking_pool = add_staking_pool(
        &mut test,
        spl_token::native_mint::id(),
        DURATION,
        SUPPLY,
        Some(SUPPLY * 2),
        EARLIEST_CLAIM_SLOT,
    );
    let mut stake_account: TestStakeAccount = add_stake_account(&mut test, staking_pool.pubkey);

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(SLOT).unwrap(); // clock.slot = 3

    let ProgramTestContext {
        mut banks_client,
        payer,
        last_blockhash: _recent_blockhash,
        ..
    } = test_context;

    let rate = staking_pool
        .deposit(
            &mut banks_client,
            AMOUNT,
            SLOT,
            &payer,
            None,
            stake_account.pubkey,
        )
        .await
        .unwrap();

    stake_account.deposit(AMOUNT, rate).unwrap();
    staking_pool.validate_state(&mut banks_client).await;
    stake_account.validate_state(&mut banks_client).await;

    let staking_pool_admin =
        Keypair::from_bytes(&staking_pool.staking_pool_admin.to_bytes()).unwrap();
    let rate = staking_pool
        .deposit(
            &mut banks_client,
            AMOUNT,
            SLOT,
            &payer,
            Some(&staking_pool_admin),
            stake_account.pubkey,
        )
        .await
        .unwrap();

    stake_account.deposit(AMOUNT, rate).unwrap();
    staking_pool.validate_state(&mut banks_client).await;
    stake_account.validate_state(&mut banks_client).await;
}

#[tokio::test]
async fn deposit_zero() {
    let mut test = staking_test!();
    test.set_compute_max_units(8200);

    const AMOUNT: u64 = 0;
    const SLOT: Slot = 10;
    const EARLIEST_CLAIM_SLOT: Slot = 0;
    const SUPPLY: u64 = 100;
    const DURATION: Slot = 1000;
    let mut staking_pool = add_staking_pool(
        &mut test,
        spl_token::native_mint::id(),
        DURATION,
        SUPPLY,
        None,
        EARLIEST_CLAIM_SLOT,
    );
    let stake_account: TestStakeAccount = add_stake_account(&mut test, staking_pool.pubkey);

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(SLOT).unwrap(); // clock.slot = 3

    let ProgramTestContext {
        mut banks_client,
        payer,
        last_blockhash: _recent_blockhash,
        ..
    } = test_context;

    let err = staking_pool
        .deposit(
            &mut banks_client,
            AMOUNT,
            SLOT,
            &payer,
            None,
            stake_account.pubkey,
        )
        .await
        .unwrap_err();

    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(StakingError::StakeDepositsZero as u32)
        )
    );
}

#[tokio::test]
async fn deposit_no_authority() {
    let mut test = staking_test!();
    test.set_compute_max_units(8200);

    const AMOUNT: u64 = 10;
    const SLOT: Slot = 10;
    const EARLIEST_CLAIM_SLOT: Slot = 0;
    const SUPPLY: u64 = 100;
    const DURATION: Slot = 1000;
    let mut staking_pool = add_staking_pool(
        &mut test,
        spl_token::native_mint::id(),
        DURATION,
        SUPPLY,
        None,
        EARLIEST_CLAIM_SLOT,
    );
    let stake_account: TestStakeAccount = add_stake_account(&mut test, staking_pool.pubkey);

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(SLOT).unwrap(); // clock.slot = 3

    let ProgramTestContext {
        mut banks_client,
        payer,
        last_blockhash: _recent_blockhash,
        ..
    } = test_context;
    let fake_owner = Keypair::new();
    let err = staking_pool
        .deposit(
            &mut banks_client,
            AMOUNT,
            SLOT,
            &payer,
            Some(&fake_owner),
            stake_account.pubkey,
        )
        .await
        .unwrap_err();

    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(StakingError::InvalidSigner as u32)
        )
    );
}

#[tokio::test]
async fn deposit_does_not_match() {
    let mut test = staking_test!();
    test.set_compute_max_units(8200);

    const AMOUNT: u64 = 10;
    const SLOT: Slot = 10;
    const EARLIEST_CLAIM_SLOT: Slot = 0;
    const SUPPLY: u64 = 100;
    const DURATION: Slot = 1000;
    let mut staking_pool = add_staking_pool(
        &mut test,
        spl_token::native_mint::id(),
        DURATION,
        SUPPLY,
        None,
        EARLIEST_CLAIM_SLOT,
    );
    let staking_pool2 = add_staking_pool(
        &mut test,
        spl_token::native_mint::id(),
        DURATION,
        SUPPLY,
        None,
        EARLIEST_CLAIM_SLOT,
    );
    let stake_account: TestStakeAccount = add_stake_account(&mut test, staking_pool2.pubkey);

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(SLOT).unwrap(); // clock.slot = 3

    let ProgramTestContext {
        mut banks_client,
        payer,
        last_blockhash: _recent_blockhash,
        ..
    } = test_context;
    let err = staking_pool
        .deposit(
            &mut banks_client,
            AMOUNT,
            SLOT,
            &payer,
            None,
            stake_account.pubkey,
        )
        .await
        .unwrap_err();

    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(StakingError::InvalidStakingPool as u32)
        )
    );
}
