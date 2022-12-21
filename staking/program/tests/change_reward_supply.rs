#![cfg(feature = "test-bpf")]

use solana_program_test::*;
use solana_sdk::clock::Slot;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::transaction::TransactionError;

use helpers::*;
use port_finance_staking::error::StakingError;
use port_finance_staking::math::Decimal;
use port_finance_staking::solana_program::instruction::InstructionError;
use port_finance_staking::state::staking_pool::RatePerSlot;

mod helpers;

#[tokio::test]
async fn test_change_reward_supply() {
    let mut test = staking_test!();
    const START_SLOT: Slot = 100;
    const ELAPSED_SLOT: Slot = 500;
    const ELAPSED_SLOT2: Slot = 100;
    // limit to track compute unit increase
    test.set_compute_max_units(50_000);
    let mut staking_pool =
        add_staking_pool(&mut test, spl_token::native_mint::id(), 1000, 100, None, 0);
    let stake_account = add_stake_account(&mut test, staking_pool.pubkey);
    let mut test_context = test.start_with_context().await;

    {
        test_context.warp_to_slot(START_SLOT).unwrap(); // clock.slot = 500
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;
        staking_pool
            .deposit(banks_client, 10, 100, payer, None, stake_account.pubkey)
            .await
            .unwrap();
        staking_pool.validate_state(banks_client).await;
    }
    {
        test_context
            .warp_to_slot(START_SLOT + ELAPSED_SLOT)
            .unwrap(); // clock.slot = 500
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            ..
        } = test_context;
        staking_pool
            .change_reward_supply(
                banks_client,
                200,
                None,
                START_SLOT + ELAPSED_SLOT,
                spl_token::native_mint::id(),
                None,
                payer,
            )
            .await
            .unwrap();
        staking_pool.validate_state(banks_client).await;
        assert_eq!(
            staking_pool.staking_pool.rate_per_slot,
            RatePerSlot {
                reward: Decimal::from_percent(50),
                sub_reward: None
            }
        );
        let reward_pool_balance =
            get_token_balance(banks_client, staking_pool.staking_pool.reward_token_pool).await;
        assert_eq!(reward_pool_balance, 300);
    }
    {
        test_context
            .warp_to_slot(START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2)
            .unwrap();
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            ..
        } = test_context;
        staking_pool
            .change_reward_supply(
                banks_client,
                -160,
                None,
                START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2,
                spl_token::native_mint::id(),
                None,
                payer,
            )
            .await
            .unwrap();
        staking_pool.validate_state(banks_client).await;
        assert_eq!(
            staking_pool.staking_pool.rate_per_slot,
            RatePerSlot {
                reward: Decimal::from_percent(10),
                sub_reward: None
            }
        );
        let reward_pool_balance =
            get_token_balance(banks_client, staking_pool.staking_pool.reward_token_pool).await;
        assert_eq!(reward_pool_balance, 140);
    }
    {
        test_context
            .warp_to_slot(START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2 + ELAPSED_SLOT2)
            .unwrap();
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            ..
        } = test_context;
        let err = staking_pool
            .change_reward_supply(
                banks_client,
                -160,
                None,
                START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2 + ELAPSED_SLOT2,
                spl_token::native_mint::id(),
                None,
                payer,
            )
            .await
            .unwrap_err();

        assert_eq!(
            err,
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(StakingError::ReduceRewardTooMuch as u32)
            )
        );
        let reward_pool_balance =
            get_token_balance(banks_client, staking_pool.staking_pool.reward_token_pool).await;
        assert_eq!(reward_pool_balance, 140);
    }
    {
        test_context
            .warp_to_slot(START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2 + ELAPSED_SLOT2 + ELAPSED_SLOT2)
            .unwrap();
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            ..
        } = test_context;

        staking_pool.staking_pool_admin = Keypair::new();

        let err = staking_pool
            .change_reward_supply(
                banks_client,
                -1,
                None,
                START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2 + ELAPSED_SLOT2 + ELAPSED_SLOT2,
                spl_token::native_mint::id(),
                None,
                payer,
            )
            .await
            .unwrap_err();
        assert_eq!(
            err,
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(StakingError::InvalidSigner as u32)
            )
        )
    }
}

#[tokio::test]
async fn test_change_sub_reward_supply() {
    let mut test = staking_test!();
    const START_SLOT: Slot = 100;
    const ELAPSED_SLOT: Slot = 500;
    const ELAPSED_SLOT2: Slot = 100;
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
    let mut test_context = test.start_with_context().await;

    {
        test_context.warp_to_slot(START_SLOT).unwrap(); // clock.slot = 500
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;
        staking_pool
            .deposit(banks_client, 10, 100, payer, None, stake_account.pubkey)
            .await
            .unwrap();
        staking_pool.validate_state(banks_client).await;
    }
    {
        test_context
            .warp_to_slot(START_SLOT + ELAPSED_SLOT)
            .unwrap(); // clock.slot = 500
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            ..
        } = test_context;
        staking_pool
            .change_reward_supply(
                banks_client,
                100,
                Some(400),
                START_SLOT + ELAPSED_SLOT,
                spl_token::native_mint::id(),
                Some(spl_token::native_mint::id()),
                payer,
            )
            .await
            .unwrap();
        staking_pool.validate_state(banks_client).await;

        staking_pool
            .change_reward_supply(
                banks_client,
                100,
                None,
                START_SLOT + ELAPSED_SLOT,
                spl_token::native_mint::id(),
                Some(spl_token::native_mint::id()),
                payer,
            )
            .await
            .unwrap();
        staking_pool.validate_state(banks_client).await;

        assert_eq!(
            staking_pool.staking_pool.rate_per_slot,
            RatePerSlot {
                reward: Decimal::from_percent(50),
                sub_reward: Some(Decimal::from_percent(100))
            }
        );
        let reward_pool_balance =
            get_token_balance(banks_client, staking_pool.staking_pool.reward_token_pool).await;
        assert_eq!(reward_pool_balance, 300);

        let sub_reward_pool_balance = get_token_balance(
            banks_client,
            staking_pool.staking_pool.sub_reward_token_pool.unwrap(),
        )
        .await;
        assert_eq!(sub_reward_pool_balance, 300 * 2);
    }
    {
        test_context
            .warp_to_slot(START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2)
            .unwrap();
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            ..
        } = test_context;
        staking_pool
            .change_reward_supply(
                banks_client,
                -160,
                Some(-320),
                START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2,
                spl_token::native_mint::id(),
                Some(spl_token::native_mint::id()),
                payer,
            )
            .await
            .unwrap();
        staking_pool.validate_state(banks_client).await;
        assert_eq!(
            staking_pool.staking_pool.rate_per_slot,
            RatePerSlot {
                reward: Decimal::from_percent(10),
                sub_reward: Some(Decimal::from_percent(20))
            }
        );
        let reward_pool_balance =
            get_token_balance(banks_client, staking_pool.staking_pool.reward_token_pool).await;
        assert_eq!(reward_pool_balance, 140);

        let sub_reward_pool_balance = get_token_balance(
            banks_client,
            staking_pool.staking_pool.sub_reward_token_pool.unwrap(),
        )
        .await;
        assert_eq!(sub_reward_pool_balance, 280);
    }
    {
        test_context
            .warp_to_slot(START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2 + ELAPSED_SLOT2)
            .unwrap();
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            ..
        } = test_context;
        let err = staking_pool
            .change_reward_supply(
                banks_client,
                -150,
                Some(-320),
                START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2 + ELAPSED_SLOT2,
                spl_token::native_mint::id(),
                Some(spl_token::native_mint::id()),
                payer,
            )
            .await
            .unwrap_err();

        assert_eq!(
            err,
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(StakingError::ReduceRewardTooMuch as u32)
            )
        );
        let reward_pool_balance =
            get_token_balance(banks_client, staking_pool.staking_pool.reward_token_pool).await;
        assert_eq!(reward_pool_balance, 140);
        let sub_reward_pool_balance = get_token_balance(
            banks_client,
            staking_pool.staking_pool.sub_reward_token_pool.unwrap(),
        )
        .await;
        assert_eq!(sub_reward_pool_balance, 280);
    }
    {
        test_context
            .warp_to_slot(START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2 + ELAPSED_SLOT2 + ELAPSED_SLOT2)
            .unwrap();
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            ..
        } = test_context;

        staking_pool.staking_pool_admin = Keypair::new();

        let err = staking_pool
            .change_reward_supply(
                banks_client,
                -1,
                Some(-2),
                START_SLOT + ELAPSED_SLOT + ELAPSED_SLOT2 + ELAPSED_SLOT2 + ELAPSED_SLOT2,
                spl_token::native_mint::id(),
                Some(spl_token::native_mint::id()),
                payer,
            )
            .await
            .unwrap_err();
        assert_eq!(
            err,
            TransactionError::InstructionError(
                0,
                InstructionError::Custom(StakingError::InvalidSigner as u32)
            )
        )
    }
}
