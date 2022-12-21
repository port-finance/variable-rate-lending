#![cfg(feature = "test-bpf")]

use solana_program::pubkey::Pubkey;
use solana_program_test::*;
use solana_sdk::transaction::TransactionError;

use helpers::*;
use port_finance_staking::error::StakingError;
use port_finance_staking::math::Decimal;
use port_finance_staking::solana_program::instruction::InstructionError;
use port_finance_staking::state::staking_pool::RatePerSlot;

mod helpers;

#[tokio::test]
async fn test_extend_duration() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool = add_staking_pool(
        &mut test,
        spl_token::native_mint::id(),
        1000,
        100,
        Some(200),
        0,
    );
    let stake_account = add_stake_account(&mut test, staking_pool.pubkey);
    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    staking_pool
        .deposit(&mut banks_client, 10, 1, &payer, None, stake_account.pubkey)
        .await
        .unwrap();
    staking_pool
        .change_duration(&mut banks_client, 1000, &payer, 1, true)
        .await
        .unwrap();

    staking_pool.validate_state(&mut banks_client).await;
    assert_eq!(
        staking_pool.staking_pool.rate_per_slot,
        RatePerSlot {
            reward: Decimal::from_percent(5),
            sub_reward: Some(Decimal::from_percent(10))
        }
    );
}

#[tokio::test]
async fn test_extend_duration_not_start() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool = add_staking_pool(
        &mut test,
        spl_token::native_mint::id(),
        1000,
        100,
        Some(200),
        0,
    );
    let (mut banks_client, payer, _recent_blockhash) = test.start().await;

    staking_pool
        .change_duration(&mut banks_client, 1000, &payer, 1, true)
        .await
        .unwrap();

    staking_pool.validate_state(&mut banks_client).await;
    assert_eq!(
        staking_pool.staking_pool.rate_per_slot,
        RatePerSlot {
            reward: Decimal::from_percent(5),
            sub_reward: Some(Decimal::from_percent(10))
        }
    );

    assert_eq!(staking_pool.staking_pool.end_time, 0);
    assert_eq!(staking_pool.staking_pool.duration, 2000);
}

#[tokio::test]
async fn test_extend_duration_when_end() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool = add_staking_pool(
        &mut test,
        spl_token::native_mint::id(),
        1000,
        100,
        Some(200),
        0,
    );
    let mut stake_account = add_stake_account(&mut test, staking_pool.pubkey);
    let mut stake_account2 = add_stake_account(&mut test, staking_pool.pubkey);
    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(10).unwrap(); // clock.slot = 3
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;
        let rate = staking_pool
            .deposit(banks_client, 10, 10, payer, None, stake_account.pubkey)
            .await
            .unwrap();

        stake_account.deposit(10, rate).unwrap();
    }

    test_context.warp_to_slot(1500).unwrap();
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;

        staking_pool
            .change_duration(banks_client, 1000, payer, 1500, true)
            .await
            .unwrap();
        let rate = staking_pool
            .deposit(banks_client, 10, 1500, payer, None, stake_account2.pubkey)
            .await
            .unwrap();
        stake_account2.deposit(10, rate).unwrap();
        staking_pool.validate_state(banks_client).await;
        assert_eq!(
            staking_pool.staking_pool.rate_per_slot,
            RatePerSlot {
                reward: Decimal::from_percent(0),
                sub_reward: Some(Decimal::from_percent(0))
            }
        );
        assert_eq!(staking_pool.staking_pool.end_time, 2010);
        assert_eq!(staking_pool.staking_pool.duration, 2000);
    }

    test_context.warp_to_slot(1600).unwrap();
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;
        let dest = create_token_account(
            banks_client,
            spl_token::native_mint::id(),
            payer,
            None,
            None,
        )
        .await;
        let rate = staking_pool
            .claim_reward(
                banks_client,
                1600,
                &payer,
                &stake_account.owner,
                stake_account.pubkey,
                dest,
                Some(dest),
            )
            .await
            .unwrap();

        stake_account.claim_reward(rate).unwrap();
        staking_pool.validate_state(banks_client).await;
        stake_account.validate_state(banks_client).await;
        assert_eq!(
            staking_pool.staking_pool.rate_per_slot,
            RatePerSlot {
                reward: Decimal::from_percent(0),
                sub_reward: Some(Decimal::from_percent(0))
            }
        );
        assert_eq!(staking_pool.staking_pool.end_time, 2010);
        assert_eq!(staking_pool.staking_pool.duration, 2000);
        assert_eq!(get_token_balance(banks_client, dest).await, 100 + 100 * 2);

        staking_pool
            .claim_reward(
                banks_client,
                1600,
                &payer,
                &stake_account2.owner,
                stake_account2.pubkey,
                dest,
                Some(dest),
            )
            .await
            .unwrap();
        assert_eq!(get_token_balance(banks_client, dest).await, 100 + 100 * 2);
        stake_account2.validate_state(banks_client).await;
    }

    test_context.warp_to_slot(1810).unwrap();
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;
        staking_pool
            .change_reward_supply(
                banks_client,
                100,
                Some(200),
                1810,
                spl_token::native_mint::id(),
                Some(spl_token::native_mint::id()),
                payer,
            )
            .await
            .unwrap();
    }
    test_context.warp_to_slot(1910).unwrap();
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;
        let dest = create_token_account(
            banks_client,
            spl_token::native_mint::id(),
            payer,
            None,
            None,
        )
        .await;
        staking_pool
            .claim_reward(
                banks_client,
                1910,
                &payer,
                &stake_account.owner,
                stake_account.pubkey,
                dest,
                Some(dest),
            )
            .await
            .unwrap();
        assert_eq!(get_token_balance(banks_client, dest).await, 25 + 25 * 2);
    }

    test_context.warp_to_slot(1914).unwrap();
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;
        let dest = create_token_account(
            banks_client,
            spl_token::native_mint::id(),
            payer,
            None,
            None,
        )
        .await;
        staking_pool
            .claim_reward(
                banks_client,
                1914,
                &payer,
                &stake_account2.owner,
                stake_account2.pubkey,
                dest,
                Some(dest),
            )
            .await
            .unwrap();

        assert_eq!(get_token_balance(banks_client, dest).await, 26 + 26 * 2);
        assert_eq!(
            staking_pool.staking_pool.rate_per_slot,
            RatePerSlot {
                reward: Decimal::from_percent(50),
                sub_reward: Some(Decimal::from_percent(100))
            }
        );
    }
}

#[tokio::test]
async fn test_reduce_duration() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);
    let stake_account = add_stake_account(&mut test, staking_pool.pubkey);
    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    staking_pool
        .deposit(&mut banks_client, 10, 1, &payer, None, stake_account.pubkey)
        .await
        .unwrap();
    staking_pool
        .change_duration(&mut banks_client, -500, &payer, 1, true)
        .await
        .unwrap();

    staking_pool.validate_state(&mut banks_client).await;
    assert_eq!(
        staking_pool.staking_pool.rate_per_slot,
        RatePerSlot {
            reward: Decimal::from_percent(20),
            sub_reward: None
        }
    );
}

#[tokio::test]
async fn test_reduce_duration_fail() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);
    let stake_account = add_stake_account(&mut test, staking_pool.pubkey);
    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    staking_pool
        .deposit(&mut banks_client, 10, 1, &payer, None, stake_account.pubkey)
        .await
        .unwrap();
    let err = staking_pool
        .change_duration(&mut banks_client, -1000, &payer, 1, true)
        .await
        .unwrap_err();

    assert_eq!(
        err,
        TransactionError::InstructionError(
            0,
            InstructionError::Custom(StakingError::InvalidArgumentError as u32)
        )
    );
    staking_pool.validate_state(&mut banks_client).await;
}

#[tokio::test]
async fn test_wrong_admin_fail() {
    let mut test = staking_test!();

    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);
    let stake_account = add_stake_account(&mut test, staking_pool.pubkey);
    let (mut banks_client, payer, _recent_blockhash) = test.start().await;
    staking_pool
        .deposit(&mut banks_client, 10, 1, &payer, None, stake_account.pubkey)
        .await
        .unwrap();
    let err = staking_pool
        .change_duration(&mut banks_client, 1000, &payer, 1, false)
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
