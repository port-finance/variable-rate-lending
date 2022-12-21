#![cfg(feature = "test-bpf")]
mod helpers;

use helpers::*;
use num_traits::abs;
use port_finance_staking::error::StakingError;
use port_finance_staking::math::{Decimal, TryAdd, TryDiv, TryMul, TrySub};
use port_finance_staking::solana_program::clock::Slot;
use port_finance_staking::solana_program::instruction::InstructionError;
use serde_yaml::from_str;
use solana_program_test::*;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::TransactionError;
use spl_token::state::Account as Token;
use spl_token::state::AccountState;
use std::process::Command;
use std::str::FromStr;
use std::{
    fs::File,
    io::{prelude::*, BufReader},
    path::Path,
};

#[tokio::test]
async fn claim_reward() {
    let mut test = staking_test!();
    test.set_compute_max_units(200000);

    const AMOUNT: u64 = 10;
    const SLOT: Slot = 10;
    const ELAPSED: Slot = 100;
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
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;

        let rate = staking_pool
            .deposit(
                banks_client,
                AMOUNT,
                SLOT,
                payer,
                None,
                stake_account.pubkey,
            )
            .await
            .unwrap();

        stake_account.deposit(AMOUNT, rate).unwrap();
        staking_pool.validate_state(banks_client).await;
        stake_account.validate_state(banks_client).await;
    }

    test_context.warp_to_slot(SLOT + ELAPSED).unwrap();
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
            SLOT + ELAPSED,
            payer,
            &stake_account.owner,
            stake_account.pubkey,
            dest,
            Some(dest),
        )
        .await
        .unwrap();

    let claim_amount = stake_account.claim_reward(rate).unwrap();
    staking_pool.validate_state(banks_client).await;
    stake_account.validate_state(banks_client).await;
    assert_eq!(
        claim_amount,
        (
            SUPPLY * ELAPSED / DURATION,
            Some(SUPPLY * 2 * ELAPSED / DURATION)
        )
    );
    assert_eq!(
        get_token_balance(banks_client, dest).await,
        claim_amount.0 + claim_amount.1.unwrap()
    );
}

#[tokio::test]
async fn claim_reward_and_add_sub_reward() {
    let mut test = staking_test!();
    test.set_compute_max_units(200000);

    const AMOUNT: u64 = 10;
    const SLOT: Slot = 10;
    const ELAPSED: Slot = 100;
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
    let mut stake_account: TestStakeAccount = add_stake_account(&mut test, staking_pool.pubkey);

    let sub_reward_token_account = Pubkey::new_unique();
    test.add_packable_account(
        sub_reward_token_account,
        u32::MAX as u64,
        &Token {
            mint: spl_token::native_mint::id(),
            owner: staking_pool.staking_pool_admin.pubkey(),
            amount: SUPPLY * 2,
            state: AccountState::Initialized,
            ..Token::default()
        },
        &spl_token::id(),
    );

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(SLOT).unwrap(); // clock.slot = 3
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;

        let rate = staking_pool
            .deposit(
                banks_client,
                AMOUNT,
                SLOT,
                payer,
                None,
                stake_account.pubkey,
            )
            .await
            .unwrap();

        stake_account.deposit(AMOUNT, rate).unwrap();
        staking_pool.validate_state(banks_client).await;
        stake_account.validate_state(banks_client).await;
    }

    test_context.warp_to_slot(SLOT + ELAPSED).unwrap();
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
                SLOT + ELAPSED,
                payer,
                &stake_account.owner,
                stake_account.pubkey,
                dest,
                None,
            )
            .await
            .unwrap();

        let claim_amount = stake_account.claim_reward(rate).unwrap();

        staking_pool.validate_state(banks_client).await;
        stake_account.validate_state(banks_client).await;
        assert_eq!(claim_amount, (SUPPLY * ELAPSED / DURATION, None));
        assert_eq!(get_token_balance(banks_client, dest).await, claim_amount.0);
    }
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;

        staking_pool
            .add_sub_reward(
                banks_client,
                SUPPLY * 2,
                SLOT + ELAPSED,
                sub_reward_token_account,
                payer,
            )
            .await
            .unwrap();
        staking_pool.validate_state(banks_client).await;
        staking_pool.staking_pool.sub_reward_token_pool.unwrap();
        assert_eq!(
            staking_pool.staking_pool.cumulative_rate.sub_reward,
            Some(Decimal::zero())
        );
        let duration = staking_pool.staking_pool.duration - ELAPSED;
        assert_eq!(
            staking_pool.staking_pool.rate_per_slot.sub_reward,
            Some(Decimal::from(SUPPLY * 2).try_div(duration).unwrap())
        );
    }
    test_context.warp_to_slot(SLOT + ELAPSED + ELAPSED).unwrap();
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

        let err = staking_pool
            .claim_reward(
                banks_client,
                SLOT + ELAPSED,
                payer,
                &stake_account.owner,
                stake_account.pubkey,
                dest,
                None,
            )
            .await
            .unwrap_err();

        assert_eq!(
            err,
            TransactionError::InstructionError(0, InstructionError::NotEnoughAccountKeys)
        );
        let rate = staking_pool
            .claim_reward(
                banks_client,
                SLOT + ELAPSED + ELAPSED,
                payer,
                &stake_account.owner,
                stake_account.pubkey,
                dest,
                Some(dest),
            )
            .await
            .unwrap();

        let claim_amount = stake_account.claim_reward(rate).unwrap();

        staking_pool.validate_state(banks_client).await;
        stake_account.validate_state(banks_client).await;
        assert_eq!(
            claim_amount,
            (
                SUPPLY * ELAPSED / DURATION,
                Some(SUPPLY * 2 * ELAPSED / (DURATION - SLOT - ELAPSED)),
            )
        );
        assert_eq!(
            get_token_balance(banks_client, dest).await,
            claim_amount.0 + claim_amount.1.unwrap()
        );
    }
}

#[tokio::test]
async fn claim_reward_no_authority() {
    let mut test = staking_test!();
    test.set_compute_max_units(20000);

    const AMOUNT: u64 = 10;
    const SLOT: Slot = 10;
    const ELAPSED: Slot = 100;
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
    let mut stake_account: TestStakeAccount = add_stake_account(&mut test, staking_pool.pubkey);

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(SLOT).unwrap(); // clock.slot = 3
    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;

        let rate = staking_pool
            .deposit(
                banks_client,
                AMOUNT,
                SLOT,
                payer,
                None,
                stake_account.pubkey,
            )
            .await
            .unwrap();

        stake_account.deposit(AMOUNT, rate).unwrap();
        staking_pool.validate_state(banks_client).await;
        stake_account.validate_state(banks_client).await;
    }

    test_context.warp_to_slot(SLOT + ELAPSED).unwrap();
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

    let fake_authority = Keypair::new();
    let err = staking_pool
        .claim_reward(
            banks_client,
            SLOT + ELAPSED,
            payer,
            &fake_authority,
            stake_account.pubkey,
            dest,
            None,
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
async fn claim_reward_before_available_time() {
    let mut test = staking_test!();
    test.set_compute_max_units(22200);

    const AMOUNT: u64 = 10;
    const SLOT: Slot = 10;
    const ELAPSED: Slot = 100;
    const EARLIEST_CLAIM_SLOT: Slot = 200;
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
    let mut stake_account: TestStakeAccount = add_stake_account(&mut test, staking_pool.pubkey);

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(SLOT).unwrap(); // clock.slot = 3

    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;

        let rate = staking_pool
            .deposit(
                banks_client,
                AMOUNT,
                SLOT,
                payer,
                None,
                stake_account.pubkey,
            )
            .await
            .unwrap();

        stake_account.deposit(AMOUNT, rate).unwrap();
        staking_pool.validate_state(banks_client).await;
        stake_account.validate_state(banks_client).await;
    }

    test_context.warp_to_slot(SLOT + ELAPSED).unwrap();

    let (claim_amount, dest) = {
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

        let unchanged_staking_pool = staking_pool.staking_pool.clone();
        let unchanged_stake_account = stake_account.stake_account.clone();

        let rate = staking_pool
            .claim_reward(
                banks_client,
                SLOT + ELAPSED,
                payer,
                &stake_account.owner,
                stake_account.pubkey,
                dest,
                None,
            )
            .await
            .unwrap();

        let claim_amount = stake_account.claim_reward(rate).unwrap();
        assert_eq!(
            stake_account.get_state(banks_client).await,
            unchanged_stake_account
        );
        assert_eq!(
            staking_pool.get_state(banks_client).await,
            unchanged_staking_pool
        );
        (claim_amount, dest)
    };

    test_context.warp_to_slot(EARLIEST_CLAIM_SLOT + 1).unwrap();
    let ProgramTestContext {
        ref mut banks_client,
        ref payer,
        last_blockhash: _recent_blockhash,
        ..
    } = test_context;

    let rate = staking_pool
        .claim_reward(
            banks_client,
            EARLIEST_CLAIM_SLOT + 1,
            payer,
            &stake_account.owner,
            stake_account.pubkey,
            dest,
            None,
        )
        .await
        .unwrap();

    let claim_amount = stake_account.claim_reward(rate).unwrap().0 + claim_amount.0;

    staking_pool.validate_state(banks_client).await;
    stake_account.validate_state(banks_client).await;
    assert_eq!(
        claim_amount,
        SUPPLY * (EARLIEST_CLAIM_SLOT + 1 - SLOT) / DURATION
    );
    assert_eq!(get_token_balance(banks_client, dest).await, claim_amount);
}

#[tokio::test]
async fn claim_reward_update_avaiable_time() {
    let mut test = staking_test!();
    test.set_compute_max_units(22200);

    const AMOUNT: u64 = 10;
    const SLOT: Slot = 10;
    const ELAPSED: Slot = 100;
    const EARLIEST_CLAIM_SLOT: Slot = 500;
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
    let mut stake_account: TestStakeAccount = add_stake_account(&mut test, staking_pool.pubkey);

    let mut test_context = test.start_with_context().await;
    test_context.warp_to_slot(SLOT).unwrap(); // clock.slot = 3

    {
        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;

        let rate = staking_pool
            .deposit(
                banks_client,
                AMOUNT,
                SLOT,
                payer,
                None,
                stake_account.pubkey,
            )
            .await
            .unwrap();

        stake_account.deposit(AMOUNT, rate).unwrap();
        staking_pool.validate_state(banks_client).await;
        stake_account.validate_state(banks_client).await;
    }

    test_context.warp_to_slot(SLOT + ELAPSED).unwrap();

    let (claim_amount, dest) = {
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

        let unchanged_staking_pool = staking_pool.staking_pool.clone();
        let unchanged_stake_account = stake_account.stake_account.clone();

        let rate = staking_pool
            .claim_reward(
                banks_client,
                SLOT + ELAPSED,
                payer,
                &stake_account.owner,
                stake_account.pubkey,
                dest,
                None,
            )
            .await
            .unwrap();

        let claim_amount = stake_account.claim_reward(rate).unwrap();
        assert_eq!(
            stake_account.get_state(banks_client).await,
            unchanged_stake_account
        );
        assert_eq!(
            staking_pool.get_state(banks_client).await,
            unchanged_staking_pool
        );
        (claim_amount, dest)
    };

    test_context.warp_to_slot(SLOT + ELAPSED + ELAPSED).unwrap();
    let ProgramTestContext {
        ref mut banks_client,
        ref payer,
        last_blockhash: _recent_blockhash,
        ..
    } = test_context;

    staking_pool
        .update_earliest_claim_time(banks_client, 100, payer)
        .await
        .unwrap();
    let rate = staking_pool
        .claim_reward(
            banks_client,
            SLOT + ELAPSED + ELAPSED,
            payer,
            &stake_account.owner,
            stake_account.pubkey,
            dest,
            None,
        )
        .await
        .unwrap();

    let claim_amount = stake_account.claim_reward(rate).unwrap().0 + claim_amount.0;

    staking_pool.validate_state(banks_client).await;
    stake_account.validate_state(banks_client).await;
    assert_eq!(claim_amount, SUPPLY * (ELAPSED + ELAPSED) / DURATION);
    assert_eq!(get_token_balance(banks_client, dest).await, claim_amount);

    staking_pool.staking_pool_admin = Keypair::new();

    let err = staking_pool
        .update_earliest_claim_time(banks_client, 200, payer)
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

fn lines_from_file(filename: impl AsRef<Path>) -> Vec<String> {
    let file = File::open(filename).expect("no such file");
    let buf = BufReader::new(file);
    buf.lines()
        .map(|l| l.expect("Could not parse line"))
        .collect()
}

#[tokio::test]
async fn claim_reward_random_test() {
    let mut test = staking_test!();

    Command::new("python3")
        .arg("tests/specification.py")
        .status()
        .expect("Specification script failed to run");
    let params = lines_from_file("tests/randoms");
    let duration: Slot = u64::from_str(&params[0]).unwrap() * 2;
    let num_of_user: usize = usize::from_str(&params[1]).unwrap();
    let supply: u64 = u64::from_str(&params[2]).unwrap() * 2;

    let mut users = vec![(0, Decimal::zero()); num_of_user];

    let mut staking_pool = add_staking_pool(
        &mut test,
        spl_token::native_mint::id(),
        duration,
        supply,
        Some(supply * 2),
        EARLIEST_CLAIM_SLOT,
    );

    let mut stake_accounts: Vec<TestStakeAccount> = (0..num_of_user)
        .map(|_| add_stake_account(&mut test, staking_pool.pubkey))
        .collect();

    const EARLIEST_CLAIM_SLOT: Slot = 0;

    let supply_per_slot = Decimal::from(supply).try_div(duration).unwrap();
    let mut pool_size = 0;
    let mut test_context = test.start_with_context().await;

    let mut i = 1;
    for para in &params[3..] {
        let para_vec: Vec<&str> = para.split_whitespace().collect();
        let do_something = para_vec[0] == "1";
        let j = if do_something {
            from_str::<usize>(para_vec[1]).unwrap()
        } else {
            0
        };

        let change = if do_something {
            from_str::<i64>(para_vec[2]).unwrap()
        } else {
            0
        };

        let stake_account = &mut stake_accounts[j];
        let to_deposit = change > 0;
        if i != 1 {
            test_context.warp_to_slot(i).unwrap(); // clock.slot = 3
        }

        let ProgramTestContext {
            ref mut banks_client,
            ref payer,
            last_blockhash: _recent_blockhash,
            ..
        } = test_context;

        for (balance, reward) in &mut users {
            if pool_size != 0 {
                *reward = (*reward)
                    .try_add(
                        (Decimal::from(*balance)
                            .try_div(pool_size)
                            .unwrap()
                            .try_mul(2)
                            .unwrap()
                            .try_mul(supply_per_slot))
                        .unwrap(),
                    )
                    .unwrap();
            }
        }

        if do_something {
            if to_deposit {
                let deposit_amount = abs(change) as u64;
                let rate = staking_pool
                    .deposit(
                        banks_client,
                        deposit_amount,
                        i,
                        payer,
                        None,
                        stake_account.pubkey,
                    )
                    .await
                    .unwrap();
                stake_account.deposit(deposit_amount, rate).unwrap();
                staking_pool.validate_state(banks_client).await;
                stake_account.validate_state(banks_client).await;
                pool_size += deposit_amount;
                users[j].0 += deposit_amount;
            } else {
                let withdraw_amount = abs(change) as u64;
                let rate = staking_pool
                    .withdraw(
                        banks_client,
                        withdraw_amount,
                        i,
                        payer,
                        None,
                        stake_account.pubkey,
                    )
                    .await
                    .unwrap();
                stake_account.withdraw(withdraw_amount, rate).unwrap();
                staking_pool.validate_state(banks_client).await;
                stake_account.validate_state(banks_client).await;
                pool_size -= withdraw_amount;
                users[j].0 -= withdraw_amount;
            }
        }
        i += 2;
    }

    for (balance, reward) in &mut users {
        if pool_size != 0 {
            *reward = (*reward)
                .try_add(
                    (Decimal::from(*balance)
                        .try_div(pool_size)
                        .unwrap()
                        .try_mul(2)
                        .unwrap()
                        .try_mul(supply_per_slot))
                    .unwrap(),
                )
                .unwrap();
        }
    }

    let rate = staking_pool.staking_pool.claim_reward(i).unwrap();
    for (mut stake_account, user) in (stake_accounts).into_iter().zip(users) {
        let reward = stake_account.stake_account.claim_reward(rate).unwrap();
        let total_reward = stake_account
            .stake_account
            .unclaimed_reward_wads
            .try_add(reward.into())
            .unwrap();
        let tol;
        let sub_tol;
        if total_reward.reward < user.1 {
            tol = user.1.try_sub(total_reward.reward).unwrap();
        } else {
            tol = total_reward.reward.try_sub(user.1).unwrap();
        }
        let sub_amount = user.1.try_mul(2).unwrap();
        if total_reward.sub_reward.unwrap() < sub_amount {
            sub_tol = sub_amount
                .try_sub(total_reward.sub_reward.unwrap())
                .unwrap();
        } else {
            sub_tol = total_reward
                .sub_reward
                .unwrap()
                .try_sub(sub_amount)
                .unwrap();
        }

        assert!(tol < Decimal::from(1u64));
        assert!(sub_tol < Decimal::from(1u64));
    }
}
