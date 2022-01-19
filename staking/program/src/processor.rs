use solana_program::account_info::next_account_info;
use spl_token::state::Account;

use crate::error::StakingError;
use crate::instruction::StakingInstruction;
use crate::solana_program::account_info::{next_account_infos, AccountInfo};
use crate::solana_program::clock::Slot;
use crate::solana_program::entrypoint::ProgramResult;
use crate::solana_program::msg;
use crate::solana_program::program::{invoke, invoke_signed};
use crate::solana_program::program_error::ProgramError;
use crate::solana_program::program_pack::{IsInitialized, Pack};
use crate::solana_program::pubkey::Pubkey;
use crate::solana_program::rent::Rent;
use crate::solana_program::sysvar::clock::Clock;
use crate::solana_program::sysvar::Sysvar;
use crate::state::{stake_account::StakeAccount, staking_pool::StakingPool};

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    let instruction = StakingInstruction::unpack(input)?;
    match instruction {
        StakingInstruction::InitStakingPool {
            supply,
            sub_supply,
            duration,
            earliest_reward_claim_time,
            bump_seed_staking_program,
            pool_owner_authority,
            admin_authority,
        } => {
            msg!("Instruction: Init staking pool");
            process_init_staking_pool(
                program_id,
                supply,
                sub_supply,
                duration,
                earliest_reward_claim_time,
                bump_seed_staking_program,
                pool_owner_authority,
                admin_authority,
                accounts,
            )
        }
        StakingInstruction::CreateStakeAccount => {
            msg!("Instruction: create stake account");
            process_create_stake_account(program_id, accounts)
        }
        StakingInstruction::Deposit(amount) => {
            msg!("Instruction: deposit");
            process_deposit(program_id, amount, accounts)
        }
        StakingInstruction::Withdraw(amount) => {
            msg!("Instruction: withdraw");
            process_withdraw(program_id, amount, accounts)
        }
        StakingInstruction::ClaimReward => {
            msg!("Instruction: claim reward");
            process_claim_reward(program_id, accounts)
        }
        StakingInstruction::UpdateEarliestRewardClaimTime(time) => {
            msg!("Instruction: update earliest reward claim time");
            process_update_earliest_reward_claim_time(program_id, time, accounts)
        }
        StakingInstruction::ChangeRewardSupply(amount, sub_amount) => {
            msg!("Instruction: add reward supply to current staking pool");
            process_change_reward_supply(program_id, amount, sub_amount, accounts)
        }
        StakingInstruction::ChangeOwner(new_owner) => {
            msg!("Instruction: Changing owner of staking pool");
            process_change_owner(program_id, new_owner, accounts)
        }
        StakingInstruction::ChangeDuration(amount) => {
            msg!("Instruction: extend duration");
            process_change_duration(program_id, amount, accounts)
        }
        StakingInstruction::AddSubRewardPool(amount) => {
            msg!("Instruction: Add Sub Reward Pool");
            process_add_sub_reward_pool(program_id, amount, accounts)
        }
        StakingInstruction::ChangeAdmin(new_admin) => {
            msg!("Instruction: Changing admin of staking pool");
            process_change_admin(program_id, new_admin, accounts)
        }
    }
}
fn process_add_sub_reward_pool(
    program_id: &Pubkey,
    amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    if let [admin_authority_info, transfer_reward_token_authority_info, staking_pool_info, sub_reward_token_supply_info, sub_reward_token_pool_info, sub_reward_token_mint_info, staking_program_derived_info, token_program_info, rent_info, clock_info] =
        next_account_infos(account_info_iter, 10)?
    {
        let mut staking_pool = StakingPool::unpack(&staking_pool_info.data.borrow())
            .map_err(|_| StakingError::InvalidStakingPool)?;

        if !transfer_reward_token_authority_info.is_signer {
            msg!("Transfer reward token authority must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        if !admin_authority_info.is_signer {
            msg!("Admin authority must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        if *admin_authority_info.key != staking_pool.admin_authority {
            msg!("Admin didn't sign for adding sub award");
            return Err(StakingError::InvalidSigner.into());
        }

        let sub_reward_supply_token_account =
            Account::unpack(&sub_reward_token_supply_info.data.borrow())
                .map_err(|_| StakingError::InvalidRewardTokenSupplyAccount)?;

        if sub_reward_supply_token_account.amount < amount as u64 {
            msg!(
                "Insufficient fund for rewarding token, {} < {}",
                sub_reward_supply_token_account.amount,
                amount
            );
            return Err(StakingError::InSufficientSupplyError.into());
        }

        if sub_reward_supply_token_account.mint != *sub_reward_token_mint_info.key {
            msg!("sub reward supply account mint is different from the reward token mint");
            return Err(StakingError::InvalidRewardSupplyAccountError.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        if sub_reward_token_mint_info.owner != token_program_info.key {
            msg!("Reward token mint is not owned by the token program provided");
            return Err(StakingError::InvalidTokenOwner.into());
        }

        if sub_reward_token_supply_info.owner != token_program_info.key
            || sub_reward_token_pool_info.owner != token_program_info.key
        {
            msg!("Reward token supply or reward token pool is not owned by the token program");
            return Err(StakingError::InvalidTokenOwner.into());
        }

        let reward_token_pool_owner_seeds = &[
            staking_pool_info.key.as_ref(),
            &[staking_pool.bump_seed_staking_program],
        ];

        let reward_token_pool_owner_derived_pubkey =
            Pubkey::create_program_address(reward_token_pool_owner_seeds, program_id)?;

        if reward_token_pool_owner_derived_pubkey != *staking_program_derived_info.key {
            msg!("sub reward token pool must be owned by the staking program");
            return Err(StakingError::InvalidRewardTokenPoolOwner.into());
        }
        let clock = &Clock::from_account_info(clock_info)?;

        staking_pool.add_sub_reward(amount, clock.slot, *sub_reward_token_pool_info.key)?;
        spl_token_init_account(TokenInitializeAccountParams {
            account: sub_reward_token_pool_info.clone(),
            mint: sub_reward_token_mint_info.clone(),
            owner: staking_program_derived_info.clone(),
            rent: rent_info.clone(),
            token_program: token_program_info.clone(),
        })?;
        spl_token_transfer(TokenTransferParams {
            source: sub_reward_token_supply_info.clone(),
            destination: sub_reward_token_pool_info.clone(),
            amount: amount as u64,
            authority: transfer_reward_token_authority_info.clone(),
            authority_signer_seeds: &[],
            token_program: token_program_info.clone(),
        })?;

        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;
        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}
fn assert_rent_exempt(rent: &Rent, account_info: &AccountInfo) -> ProgramResult {
    if !rent.is_exempt(account_info.lamports(), account_info.data_len()) {
        msg!(
            "required minimum lamports {}",
            &rent.minimum_balance(account_info.data_len()).to_string()
        );
        Err(StakingError::NotRentExempt.into())
    } else {
        Ok(())
    }
}

fn assert_uninitialized<T: Pack + IsInitialized>(
    account_info: &AccountInfo,
) -> Result<T, ProgramError> {
    let account: T = T::unpack_unchecked(&account_info.data.borrow())?;
    if account.is_initialized() {
        msg!("The account is already init");
        Err(StakingError::AlreadyInitialized.into())
    } else {
        Ok(account)
    }
}

fn process_change_owner(
    program_id: &Pubkey,
    new_owner: Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    if let [current_owner_info, staking_pool_info] = next_account_infos(account_info_iter, 2)? {
        if !current_owner_info.is_signer {
            msg!("To change owner, the current owner must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        let mut staking_pool = StakingPool::unpack(&staking_pool_info.data.borrow())?;
        if *current_owner_info.key != staking_pool.owner_authority {
            msg!("Owner didn't sign for changing owner");
            return Err(StakingError::InvalidSigner.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        staking_pool.owner_authority = new_owner;
        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;
        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}

fn process_change_admin(
    program_id: &Pubkey,
    new_admin: Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    if let [current_admin_info, staking_pool_info] = next_account_infos(account_info_iter, 2)? {
        if !current_admin_info.is_signer {
            msg!("To change admin, the current admin must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        let mut staking_pool = StakingPool::unpack(&staking_pool_info.data.borrow())?;
        if *current_admin_info.key != staking_pool.admin_authority {
            msg!("Admin didn't sign for changing admin");
            return Err(StakingError::InvalidSigner.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        staking_pool.admin_authority = new_admin;
        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;
        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}

fn process_change_duration(
    program_id: &Pubkey,
    amount: i64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    if let [admin_info, staking_pool_info, clock_info] = next_account_infos(account_info_iter, 3)? {
        if !admin_info.is_signer {
            msg!("To change owner, the current owner must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        let mut staking_pool = StakingPool::unpack(&staking_pool_info.data.borrow())?;
        if *admin_info.key != staking_pool.admin_authority {
            msg!("Admin didn't sign for transferring award out");
            return Err(StakingError::InvalidSigner.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }
        let clock = &Clock::from_account_info(clock_info)?;
        staking_pool.extend_duration(amount, clock.slot)?;

        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;
        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}

#[allow(clippy::too_many_arguments)]
fn process_change_reward_supply(
    program_id: &Pubkey,
    amount: i64,
    sub_amount: Option<i64>,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if amount == 0 {
        msg!("Cannot add zero amount to reward");
        return Err(StakingError::InvalidArgumentError.into());
    }

    let account_info_iter = &mut accounts.iter();

    if let [transfer_reward_token_authority_info, staking_pool_info, reward_token_supply_info, reward_token_pool_info, sub_reward_token_supply_info, sub_reward_token_pool_info, staking_program_derived_info, reward_token_mint_info, sub_reward_token_mint_info, token_program_info, clock_info] =
        next_account_infos(account_info_iter, 11)?
    {
        let mut staking_pool = StakingPool::unpack(&staking_pool_info.data.borrow())
            .map_err(|_| StakingError::InvalidStakingPool)?;

        if !transfer_reward_token_authority_info.is_signer {
            msg!("Transfer reward token authority must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        if amount < 0 && *transfer_reward_token_authority_info.key != staking_pool.admin_authority {
            msg!("Admin didn't sign for transferring award out");
            return Err(StakingError::InvalidSigner.into());
        }

        let reward_supply_token_account = Account::unpack(&reward_token_supply_info.data.borrow())
            .map_err(|_| StakingError::InvalidRewardTokenSupplyAccount)?;

        if amount > 0 && reward_supply_token_account.amount < amount as u64 {
            msg!(
                "Insufficient fund for rewarding token, {} < {}",
                reward_supply_token_account.amount,
                amount
            );
            return Err(StakingError::InSufficientSupplyError.into());
        }

        if reward_supply_token_account.mint != *reward_token_mint_info.key {
            msg!("reward supply account mint is different from the reward token mint");
            return Err(StakingError::InvalidRewardSupplyAccountError.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        if staking_pool.reward_token_pool != *reward_token_pool_info.key {
            msg!("reward token pool is not correct");
            return Err(StakingError::InvalidRewardTokenPool.into());
        }

        let clock = &Clock::from_account_info(clock_info)?;
        staking_pool.update_reward_supply(amount, sub_amount, clock.slot)?;

        if amount > 0 {
            spl_token_transfer(TokenTransferParams {
                source: reward_token_supply_info.clone(),
                destination: reward_token_pool_info.clone(),
                amount: amount as u64,
                authority: transfer_reward_token_authority_info.clone(),
                authority_signer_seeds: &[],
                token_program: token_program_info.clone(),
            })?;
        } else {
            spl_token_transfer(TokenTransferParams {
                source: reward_token_pool_info.clone(),
                destination: reward_token_supply_info.clone(),
                amount: -amount as u64,
                authority: staking_program_derived_info.clone(),
                authority_signer_seeds: &[
                    staking_pool_info.key.as_ref(),
                    &[staking_pool.bump_seed_staking_program],
                ],
                token_program: token_program_info.clone(),
            })?;
        }

        if let Some(sub_amount) = sub_amount {
            let sub_reward_supply_token_account =
                Account::unpack(&sub_reward_token_supply_info.data.borrow())
                    .map_err(|_| StakingError::InvalidRewardTokenSupplyAccount)?;

            if sub_amount > 0 && sub_reward_supply_token_account.amount < sub_amount as u64 {
                msg!(
                    "Insufficient fund for rewarding token, {} < {}",
                    sub_reward_supply_token_account.amount,
                    sub_amount
                );
                return Err(StakingError::InSufficientSupplyError.into());
            }

            if sub_reward_supply_token_account.mint != *sub_reward_token_mint_info.key {
                msg!("sub reward supply account mint is different from the reward token mint");
                return Err(StakingError::InvalidRewardSupplyAccountError.into());
            }

            if staking_pool.sub_reward_token_pool.unwrap() != *sub_reward_token_pool_info.key {
                msg!("sub reward token pool is not correct");
                return Err(StakingError::InvalidRewardTokenPool.into());
            }
            if amount > 0 {
                spl_token_transfer(TokenTransferParams {
                    source: sub_reward_token_supply_info.clone(),
                    destination: sub_reward_token_pool_info.clone(),
                    amount: sub_amount as u64,
                    authority: transfer_reward_token_authority_info.clone(),
                    authority_signer_seeds: &[],
                    token_program: token_program_info.clone(),
                })?;
            } else {
                spl_token_transfer(TokenTransferParams {
                    source: sub_reward_token_pool_info.clone(),
                    destination: sub_reward_token_supply_info.clone(),
                    amount: -sub_amount as u64,
                    authority: staking_program_derived_info.clone(),
                    authority_signer_seeds: &[
                        staking_pool_info.key.as_ref(),
                        &[staking_pool.bump_seed_staking_program],
                    ],
                    token_program: token_program_info.clone(),
                })?;
            }
        }

        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;
        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}

#[allow(clippy::too_many_arguments)]
fn process_init_staking_pool(
    program_id: &Pubkey,
    supply: u64,
    sub_supply: Option<u64>,
    duration: u64,
    earliest_reward_claim_time: Slot,
    bump_seed_staking_program: u8,
    pool_owner_authority: Pubkey,
    admin_authority: Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if supply == 0 {
        msg!("staking pool must have non zero supply");
        return Err(StakingError::InvalidSupplyError.into());
    }

    if duration == 0 {
        msg!("staking pool must have non zero duration");
        return Err(StakingError::InvalidDurationError.into());
    }
    let account_info_iter = &mut accounts.iter();
    if let [transfer_reward_token_authority_info, reward_token_supply_info, reward_token_pool_info, sub_reward_token_supply_info, sub_reward_token_pool_info, staking_pool_info, reward_token_mint_info, sub_reward_token_mint_info, staking_program_derived_info, rent_info, token_program_info] =
        next_account_infos(account_info_iter, 11)?
    {
        let rent = &Rent::from_account_info(rent_info)?;
        // check rent exempt
        assert_rent_exempt(rent, staking_pool_info)?;
        assert_rent_exempt(rent, reward_token_pool_info)?;

        if !transfer_reward_token_authority_info.is_signer {
            msg!("Transfer reward token authority must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        let reward_supply_token_account = Account::unpack(&reward_token_supply_info.data.borrow())
            .map_err(|_| StakingError::InvalidRewardTokenSupplyAccount)?;
        if reward_supply_token_account.amount < supply {
            msg!(
                "Insufficient fund for rewarding token, {} < {}",
                reward_supply_token_account.amount,
                supply
            );
            return Err(StakingError::InSufficientSupplyError.into());
        }

        if reward_supply_token_account.mint != *reward_token_mint_info.key {
            msg!("reward supply account mint is different from the reward token mint");
            return Err(StakingError::InvalidRewardSupplyAccountError.into());
        }

        if reward_token_mint_info.owner != token_program_info.key {
            msg!("Reward token mint is not owned by the token program provided");
            return Err(StakingError::InvalidTokenOwner.into());
        }

        if reward_token_supply_info.owner != token_program_info.key
            || reward_token_pool_info.owner != token_program_info.key
        {
            msg!("Reward token supply or reward token pool is not owned by the token program");
            return Err(StakingError::InvalidTokenOwner.into());
        }

        let reward_token_pool_owner_seeds =
            &[staking_pool_info.key.as_ref(), &[bump_seed_staking_program]];

        let reward_token_pool_owner_derived_pubkey =
            Pubkey::create_program_address(reward_token_pool_owner_seeds, program_id)?;

        if reward_token_pool_owner_derived_pubkey != *staking_program_derived_info.key {
            msg!("reward token pool must be owned by the staking program");
            return Err(StakingError::InvalidRewardTokenPoolOwner.into());
        }

        spl_token_init_account(TokenInitializeAccountParams {
            account: reward_token_pool_info.clone(),
            mint: reward_token_mint_info.clone(),
            owner: staking_program_derived_info.clone(),
            rent: rent_info.clone(),
            token_program: token_program_info.clone(),
        })?;

        spl_token_transfer(TokenTransferParams {
            source: reward_token_supply_info.clone(),
            destination: reward_token_pool_info.clone(),
            amount: supply,
            authority: transfer_reward_token_authority_info.clone(),
            authority_signer_seeds: &[],
            token_program: token_program_info.clone(),
        })?;

        if let Some(sub_supply) = sub_supply {
            if sub_supply == 0 {
                msg!("staking pool must have non zero sub supply");
                return Err(StakingError::InvalidSupplyError.into());
            }
            let sub_reward_supply_token_account =
                Account::unpack(&sub_reward_token_supply_info.data.borrow())
                    .map_err(|_| StakingError::InvalidRewardTokenSupplyAccount)?;
            if sub_reward_supply_token_account.amount < sub_supply {
                msg!(
                    "Insufficient fund for rewarding token, {} < {}",
                    sub_reward_supply_token_account.amount,
                    sub_supply
                );
                return Err(StakingError::InSufficientSupplyError.into());
            }

            if sub_reward_supply_token_account.mint != *sub_reward_token_mint_info.key {
                msg!("reward supply account mint is different from the reward token mint");
                return Err(StakingError::InvalidRewardSupplyAccountError.into());
            }

            if sub_reward_token_mint_info.owner != token_program_info.key {
                msg!("Reward token mint is not owned by the token program provided");
                return Err(StakingError::InvalidTokenOwner.into());
            }

            if sub_reward_token_supply_info.owner != token_program_info.key
                || sub_reward_token_pool_info.owner != token_program_info.key
            {
                msg!("Reward token supply or reward token pool is not owned by the token program");
                return Err(StakingError::InvalidTokenOwner.into());
            }

            spl_token_init_account(TokenInitializeAccountParams {
                account: sub_reward_token_pool_info.clone(),
                mint: sub_reward_token_mint_info.clone(),
                owner: staking_program_derived_info.clone(),
                rent: rent_info.clone(),
                token_program: token_program_info.clone(),
            })?;

            spl_token_transfer(TokenTransferParams {
                source: sub_reward_token_supply_info.clone(),
                destination: sub_reward_token_pool_info.clone(),
                amount: sub_supply,
                authority: transfer_reward_token_authority_info.clone(),
                authority_signer_seeds: &[],
                token_program: token_program_info.clone(),
            })?;
        }

        let mut staking_pool = assert_uninitialized::<StakingPool>(staking_pool_info)?;
        staking_pool.init(
            pool_owner_authority,
            admin_authority,
            *reward_token_pool_info.key,
            sub_supply.map(|_| *sub_reward_token_pool_info.key),
            duration,
            supply,
            sub_supply,
            earliest_reward_claim_time,
            bump_seed_staking_program,
        )?;
        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;

        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}

fn process_create_stake_account(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    if let [stake_account_info, staking_pool_info, stake_account_owner_info, rent_info] =
        next_account_infos(account_info_iter, 4)?
    {
        let rent = &Rent::from_account_info(rent_info)?;
        assert_rent_exempt(rent, stake_account_info)?;

        if stake_account_info.owner != program_id {
            msg!("Stake account is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        if stake_account_info.data_len() == 0 {
            msg!("Stake account must be funded and data len > 0");
            return Err(StakingError::InvalidStakeAccount.into());
        }

        let mut stake_account = assert_uninitialized::<StakeAccount>(stake_account_info)?;

        stake_account.init(*stake_account_owner_info.key, *staking_pool_info.key)?;

        StakeAccount::pack(stake_account, &mut stake_account_info.data.borrow_mut())?;
        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}

fn process_deposit(program_id: &Pubkey, amount: u64, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    if let [authority_info, stake_account_info, staking_pool_info, clock_info] =
        next_account_infos(account_info_iter, 4)?
    {
        if !authority_info.is_signer {
            msg!("staking pool owner derived must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        if stake_account_info.owner != program_id {
            msg!("Stake account is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        let mut staking_pool = StakingPool::unpack(&staking_pool_info.data.borrow())
            .map_err(|_| StakingError::InvalidStakingPool)?;

        if authority_info.key != &staking_pool.owner_authority
            && authority_info.key != &staking_pool.admin_authority
        {
            msg!("deposit to account must be signed by the owner of the staking pool");
            return Err(StakingError::InvalidSigner.into());
        }

        let mut stake_account = StakeAccount::unpack(&stake_account_info.data.borrow())
            .map_err(|_| StakingError::InvalidStakeAccount)?;

        if staking_pool_info.key != &stake_account.pool_pubkey {
            msg!("The staking pool is not the one that the stake account belongs to");
            return Err(StakingError::InvalidStakingPool.into());
        }

        let clock = &Clock::from_account_info(clock_info)?;

        staking_pool
            .deposit(clock.slot, amount)
            .and_then(|current_rate| stake_account.deposit(current_rate, amount))?;

        StakeAccount::pack(stake_account, &mut stake_account_info.data.borrow_mut())?;
        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;

        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}

fn process_withdraw(program_id: &Pubkey, amount: u64, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    if let [authority, stake_account_info, staking_pool_info, clock_info] =
        next_account_infos(account_info_iter, 4)?
    {
        if !authority.is_signer {
            msg!("Staking pool owner derived must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        if stake_account_info.owner != program_id {
            msg!("Stake account is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        let mut staking_pool = StakingPool::unpack(&staking_pool_info.data.borrow())
            .map_err(|_| StakingError::InvalidStakingPool)?;

        if authority.key != &staking_pool.owner_authority
            && authority.key != &staking_pool.admin_authority
        {
            msg!("withdraw from stake account must be signed by the owner of the staking pool");
            return Err(StakingError::InvalidSigner.into());
        }

        let mut stake_account = StakeAccount::unpack(&stake_account_info.data.borrow())
            .map_err(|_| StakingError::InvalidStakeAccount)?;

        if staking_pool_info.key != &stake_account.pool_pubkey {
            msg!("The staking pool is not the one that the stake account belongs to");
            return Err(StakingError::InvalidStakingPool.into());
        }
        let clock = &Clock::from_account_info(clock_info)?;

        staking_pool
            .withdraw(clock.slot, amount)
            .and_then(|current_rate| stake_account.withdraw(current_rate, amount))?;

        StakeAccount::pack(stake_account, &mut stake_account_info.data.borrow_mut())?;
        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;

        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}

fn process_claim_reward(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    if let [stake_account_owner, stake_account_info, staking_pool_info, reward_token_pool_info, reward_destination_info, staking_program_derived_info, clock_info, token_program_info] =
        next_account_infos(account_info_iter, 8)?
    {
        if !stake_account_owner.is_signer {
            msg!("Stake_account_owner must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }
        let clock = &Clock::from_account_info(clock_info)?;

        let mut stake_account = StakeAccount::unpack(&stake_account_info.data.borrow())
            .map_err(|_| StakingError::InvalidStakeAccount)?;
        let mut staking_pool = StakingPool::unpack(&staking_pool_info.data.borrow())
            .map_err(|_| StakingError::InvalidStakingPool)?;

        if clock.slot < staking_pool.earliest_reward_claim_time {
            msg!("It is not the time to claim reward yet");
            return Ok(());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        if stake_account_info.owner != program_id {
            msg!("Stake account is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        if staking_pool_info.key != &stake_account.pool_pubkey {
            msg!("The staking pool is not the one that the stake account belongs to");
            return Err(StakingError::InvalidStakingPool.into());
        }

        if stake_account_owner.key != &stake_account.owner {
            msg!("claim rewards must be signed by the owner of the stake account");
            return Err(StakingError::InvalidSigner.into());
        }

        if &staking_pool.reward_token_pool != reward_token_pool_info.key {
            msg!("reward token pool is not the one associated with the staking pool");
            return Err(StakingError::InvalidRewardTokenPool.into());
        }

        if reward_destination_info.owner != token_program_info.key
            || reward_token_pool_info.owner != token_program_info.key
        {
            msg!("Reward token pool or reward destination is not owned by the token program provided");
            return Err(StakingError::InvalidTokenOwner.into());
        }

        let (reward_claim_amount, sub_reward_claim_amount) = staking_pool
            .claim_reward(clock.slot)
            .and_then(|current_rate| stake_account.claim_reward(current_rate))?;
        let reward_token_pool_owner_seeds = &[
            staking_pool_info.key.as_ref(),
            &[staking_pool.bump_seed_staking_program],
        ];

        let reward_token_pool_owner_derived_pubkey =
            Pubkey::create_program_address(reward_token_pool_owner_seeds, program_id)?;

        if &reward_token_pool_owner_derived_pubkey != staking_program_derived_info.key {
            msg!("reward token pool must be owned by the staking program");
            return Err(StakingError::InvalidRewardTokenPoolOwner.into());
        }

        //Todo remove debug log
        msg!("claim amount {}", reward_claim_amount);
        spl_token_transfer(TokenTransferParams {
            source: reward_token_pool_info.clone(),
            destination: reward_destination_info.clone(),
            amount: reward_claim_amount,
            authority: staking_program_derived_info.clone(),
            authority_signer_seeds: reward_token_pool_owner_seeds,
            token_program: token_program_info.clone(),
        })?;

        //Todo remove debug log
        msg!("claim sub_amount {:?}", sub_reward_claim_amount);
        if let Some(sub_reward_claim_amount) = sub_reward_claim_amount {
            let sub_reward_token_pool_info = next_account_info(account_info_iter)?;
            let sub_reward_destination_info = next_account_info(account_info_iter)?;
            if &staking_pool
                .sub_reward_token_pool
                .ok_or(StakingError::InvalidRewardTokenPool)?
                != sub_reward_token_pool_info.key
            {
                msg!("reward token pool is not the one associated with the staking pool");
                return Err(StakingError::InvalidRewardTokenPool.into());
            }
            spl_token_transfer(TokenTransferParams {
                source: sub_reward_token_pool_info.clone(),
                destination: sub_reward_destination_info.clone(),
                amount: sub_reward_claim_amount,
                authority: staking_program_derived_info.clone(),
                authority_signer_seeds: reward_token_pool_owner_seeds,
                token_program: token_program_info.clone(),
            })?;
        }

        StakeAccount::pack(stake_account, &mut stake_account_info.data.borrow_mut())?;
        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;
        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}

fn process_update_earliest_reward_claim_time(
    program_id: &Pubkey,
    time: Slot,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    if let [admin_authority, staking_pool_info] = next_account_infos(account_info_iter, 2)? {
        if !admin_authority.is_signer {
            msg!("Staking pool owner derived must be a signer");
            return Err(StakingError::InvalidArgumentError.into());
        }

        if staking_pool_info.owner != program_id {
            msg!("Staking pool is not owned by the staking program");
            return Err(StakingError::InvalidAccountOwner.into());
        }

        let mut staking_pool = StakingPool::unpack(&staking_pool_info.data.borrow())
            .map_err(|_| StakingError::InvalidStakingPool)?;

        if admin_authority.key != &staking_pool.admin_authority {
            msg!(
                "update earliest reward claim time must be signed by the admin of the staking pool"
            );
            return Err(StakingError::InvalidSigner.into());
        }

        staking_pool.earliest_reward_claim_time = time;

        StakingPool::pack(staking_pool, &mut staking_pool_info.data.borrow_mut())?;
        Ok(())
    } else {
        msg!("Wrong number of accounts");
        Err(StakingError::InvalidArgumentError.into())
    }
}
/// Issue a spl_token `InitializeAccount` instruction.
#[inline(always)]
fn spl_token_init_account(params: TokenInitializeAccountParams<'_>) -> ProgramResult {
    let TokenInitializeAccountParams {
        account,
        mint,
        owner,
        rent,
        token_program,
    } = params;
    let ix = spl_token::instruction::initialize_account(
        token_program.key,
        account.key,
        mint.key,
        owner.key,
    )?;
    let result = invoke(&ix, &[account, mint, owner, rent, token_program]);
    result.map_err(|_| StakingError::TokenInitializeAccountFailed.into())
}

/// Issue a spl_token `Transfer` instruction.
#[inline(always)]
fn spl_token_transfer(params: TokenTransferParams<'_, '_>) -> ProgramResult {
    let TokenTransferParams {
        source,
        destination,
        authority,
        token_program,
        amount,
        authority_signer_seeds,
    } = params;
    let result = if authority_signer_seeds.is_empty() {
        invoke(
            &spl_token::instruction::transfer(
                token_program.key,
                source.key,
                destination.key,
                authority.key,
                &[],
                amount,
            )?,
            &[source, destination, authority, token_program],
        )
    } else {
        invoke_signed(
            &spl_token::instruction::transfer(
                token_program.key,
                source.key,
                destination.key,
                authority.key,
                &[],
                amount,
            )?,
            &[source, destination, authority, token_program],
            &[authority_signer_seeds],
        )
    };

    result.map_err(|_| StakingError::TokenTransferFailed.into())
}

struct TokenInitializeAccountParams<'a> {
    account: AccountInfo<'a>,
    mint: AccountInfo<'a>,
    owner: AccountInfo<'a>,
    rent: AccountInfo<'a>,
    token_program: AccountInfo<'a>,
}

struct TokenTransferParams<'a: 'b, 'b> {
    source: AccountInfo<'a>,
    destination: AccountInfo<'a>,
    amount: u64,
    authority: AccountInfo<'a>,
    authority_signer_seeds: &'b [&'b [u8]],
    token_program: AccountInfo<'a>,
}
