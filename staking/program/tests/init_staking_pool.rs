#![cfg(feature = "test-bpf")]

use solana_program::clock::Slot;
use solana_program::pubkey::Pubkey;
use solana_program_test::tokio;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::{Transaction, TransactionError};
use spl_token::state::Account as Token;
use spl_token::state::AccountState;

use port_finance_staking::error::StakingError;
use port_finance_staking::instruction::init_staking_pool;
use port_finance_staking::math::TryMul;
use port_finance_staking::solana_program::instruction::InstructionError;

use crate::helpers::*;

mod helpers;

#[tokio::test]
async fn test_success() {
    let mut test = staking_test!();

    test.set_compute_max_units(38200);

    let supply_accounts_owner = Keypair::new();
    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    const SUPPLY: u64 = 100;
    const DURATION: Slot = 1000;
    const EARLIEST_REWARD_CLAIM_TIME: Slot = 0;

    let sol_reward_supplier = create_and_mint_to_token_account(
        &mut banks_client,
        spl_token::native_mint::id(),
        None,
        &payer,
        supply_accounts_owner.pubkey(),
        SUPPLY,
    )
    .await;

    let sol_staking_pool = TestStakingPool::init(
        "sol_staking_pool".to_owned(),
        &mut banks_client,
        sol_reward_supplier,
        spl_token::native_mint::id(),
        SUPPLY,
        DURATION,
        EARLIEST_REWARD_CLAIM_TIME,
        &payer,
        &supply_accounts_owner,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    sol_staking_pool.validate_state(&mut banks_client).await;

    let staking_pool = sol_staking_pool.get_state(&mut banks_client).await;
    let sol_reward_supply =
        get_token_balance(&mut banks_client, staking_pool.reward_token_pool).await;
    assert_eq!(sol_reward_supply, SUPPLY);

    assert_eq!(
        staking_pool
            .rate_per_slot
            .try_mul(staking_pool.duration)
            .and_then(|n| n.reward.try_round_u64())
            .unwrap(),
        SUPPLY
    );
}

#[tokio::test]
async fn test_success_with_dual_reward() {
    let mut test = staking_test!();

    test.set_compute_max_units(82200);

    let supply_accounts_owner = Keypair::new();
    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    const SUPPLY: u64 = 100;
    const DURATION: Slot = 1000;
    const EARLIEST_REWARD_CLAIM_TIME: Slot = 0;

    let sol_reward_supplier = create_and_mint_to_token_account(
        &mut banks_client,
        spl_token::native_mint::id(),
        None,
        &payer,
        supply_accounts_owner.pubkey(),
        SUPPLY * 3,
    )
    .await;

    let sol_staking_pool = TestStakingPool::init(
        "sol_staking_pool".to_owned(),
        &mut banks_client,
        sol_reward_supplier,
        spl_token::native_mint::id(),
        SUPPLY,
        DURATION,
        EARLIEST_REWARD_CLAIM_TIME,
        &payer,
        &supply_accounts_owner,
        Some(SUPPLY * 2),
        Some(sol_reward_supplier),
        Some(spl_token::native_mint::id()),
    )
    .await
    .unwrap();

    sol_staking_pool.validate_state(&mut banks_client).await;

    let staking_pool = sol_staking_pool.get_state(&mut banks_client).await;
    let sol_reward_supply =
        get_token_balance(&mut banks_client, staking_pool.reward_token_pool).await;
    assert_eq!(sol_reward_supply, SUPPLY);

    assert_eq!(
        staking_pool
            .rate_per_slot
            .try_mul(staking_pool.duration)
            .and_then(|n| n.reward.try_round_u64())
            .unwrap(),
        SUPPLY
    );

    let sub_sol_reward_supply = get_token_balance(
        &mut banks_client,
        staking_pool.sub_reward_token_pool.unwrap(),
    )
    .await;

    assert_eq!(sub_sol_reward_supply, SUPPLY * 2);

    assert_eq!(
        staking_pool
            .rate_per_slot
            .try_mul(staking_pool.duration)
            .and_then(|n| n.sub_reward.unwrap().try_round_u64())
            .unwrap(),
        SUPPLY * 2
    );
}

#[tokio::test]
async fn test_already_initialized() {
    let mut test = staking_test!();

    test.set_compute_max_units(50000);

    let transfer_reward_token_authority = Keypair::new();
    const SUPPLY: u64 = 100;
    let reward_token_supply_pubkey = Pubkey::new_unique();
    test.add_packable_account(
        reward_token_supply_pubkey,
        u32::MAX as u64,
        &Token {
            mint: spl_token::native_mint::id(),
            amount: SUPPLY,
            owner: transfer_reward_token_authority.pubkey(),
            state: AccountState::Initialized,
            ..Token::default()
        },
        &spl_token::id(),
    );
    let reward_pool_keypair = Keypair::new();
    test.add_packable_account(
        reward_pool_keypair.pubkey(),
        u32::MAX as u64,
        &Token::default(),
        &spl_token::id(),
    );
    let test_staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);

    let (mut banks_client, payer, recent_blockhash) = test.start().await;

    let mut transaction = Transaction::new_with_payer(
        &[init_staking_pool(
            port_finance_staking::id(),
            SUPPLY,
            None,
            1000,
            0,
            transfer_reward_token_authority.pubkey(),
            reward_token_supply_pubkey,
            reward_pool_keypair.pubkey(),
            None,
            None,
            test_staking_pool.pubkey,
            spl_token::native_mint::id(),
            None,
            test_staking_pool.staking_pool_owner.pubkey(),
            test_staking_pool.staking_pool_admin.pubkey(),
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(
        &[&payer, &transfer_reward_token_authority],
        recent_blockhash,
    );

    let err = banks_client
        .process_transaction(transaction)
        .await
        .unwrap_err()
        .unwrap();

    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(StakingError::AlreadyInitialized as u32)
        )
    );
}

#[tokio::test]
async fn test_zero_supply() {
    let mut test = staking_test!();

    test.set_compute_max_units(58200);

    let supply_accounts_owner = Keypair::new();
    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    const SUPPLY: u64 = 0;
    const DURATION: Slot = 1000;
    const EARLIEST_REWARD_CLAIM_TIME: Slot = 0;

    let sol_reward_supplier = create_and_mint_to_token_account(
        &mut banks_client,
        spl_token::native_mint::id(),
        None,
        &payer,
        supply_accounts_owner.pubkey(),
        SUPPLY,
    )
    .await;

    let err = TestStakingPool::init(
        "sol_staking_pool".to_owned(),
        &mut banks_client,
        sol_reward_supplier,
        spl_token::native_mint::id(),
        SUPPLY,
        DURATION,
        EARLIEST_REWARD_CLAIM_TIME,
        &payer,
        &supply_accounts_owner,
        None,
        None,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(
        err,
        TransactionError::InstructionError(
            3,
            InstructionError::Custom(StakingError::InvalidSupplyError as u32)
        )
    );
}

#[tokio::test]
async fn test_zero_duration() {
    let mut test = staking_test!();

    test.set_compute_max_units(8200);

    let supply_accounts_owner = Keypair::new();
    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    const SUPPLY: u64 = 10;
    const DURATION: Slot = 0;
    const EARLIEST_REWARD_CLAIM_TIME: Slot = 0;

    let sol_reward_supplier = create_and_mint_to_token_account(
        &mut banks_client,
        spl_token::native_mint::id(),
        None,
        &payer,
        supply_accounts_owner.pubkey(),
        SUPPLY,
    )
    .await;

    let err = TestStakingPool::init(
        "sol_staking_pool".to_owned(),
        &mut banks_client,
        sol_reward_supplier,
        spl_token::native_mint::id(),
        SUPPLY,
        DURATION,
        EARLIEST_REWARD_CLAIM_TIME,
        &payer,
        &supply_accounts_owner,
        None,
        None,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(
        err,
        TransactionError::InstructionError(
            3,
            InstructionError::Custom(StakingError::InvalidDurationError as u32)
        )
    );
}
