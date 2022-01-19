use core::convert::TryInto;
use std::mem::size_of;

use solana_program::clock::Slot;
use solana_program::instruction::{AccountMeta, Instruction};

use crate::dummy_id;
use crate::error::StakingError;
use crate::instruction::StakingInstruction::*;
use crate::solana_program::pubkey::PUBKEY_BYTES;
use crate::solana_program::{msg, program_error::ProgramError, pubkey::Pubkey, sysvar};

/// Instructions supported by the lending program.
#[derive(Clone, Debug, PartialEq)]
pub enum StakingInstruction {
    /// Accounts expected by this instruction:
    ///   0. `[signer]` Transfer reward token authority.
    ///   1. `[writable]` Reward token supply.
    ///   2. `[writable]` Reward token pool - uninitialized.

    ///   3. `[writable]` Staking pool - uninitialized.
    ///   4. `[]` Reward token mint.
    ///   5. `[]` Staking program derived that owns reward token pool.
    ///   6. `[]` Rent sysvar .
    ///   7. `[]` Token program.
    ///   8. `[writable, optional]` Sub Reward token supply.
    ///   9. `[writable, optional]` Sub Reward token pool - uninitialized.
    ///   10. `[optional]` Sub Reward token mint.
    InitStakingPool {
        supply: u64, // rate per slot = supply / duration
        sub_supply: Option<u64>,
        duration: u64, // num of slots
        earliest_reward_claim_time: Slot,
        bump_seed_staking_program: u8,
        pool_owner_authority: Pubkey,
        admin_authority: Pubkey,
    },

    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Stake account - uninitialized.
    ///   1. `[]` Staking Pool.
    ///   2. `[]` Stake account owner.
    ///   3. `[]` Rent sysvar.
    CreateStakeAccount,

    /// Deposit to a stake account.
    ///
    /// Accounts expected by this instruction:
    ///   0. `[signer]` authority.
    ///   1. `[writable]` Stake account.
    ///   2. `[writable]` Staking pool.
    ///   3. `[]` Clock sysvar.
    Deposit(u64),

    /// Withdrawn to a stake account.
    ///
    /// Accounts expected by this instruction:
    ///   0. `[signer]` authority.
    ///   1. `[writable]` Stake account.
    ///   2. `[writable]` Staking pool.
    ///   3. `[]` Clock sysvar.
    Withdraw(u64),

    /// Claim all unclaimed Reward from a stake account
    ///
    /// Accounts expected by this instruction:
    ///   0. `[signer]` Stake account owner.
    ///   1. `[writable]` Stake account.
    ///   2. `[writable]` Staking pool.
    ///   3. `[writable]` Reward token pool.
    ///   4. `[writable]` Reward destination.
    ///   5. `[]` Staking Pool owner derived from staking pool pubkey
    ///   6. `[]` Clock sysvar.
    ///   7. `[]` Token program.
    ///   8. `[writable, optional]` Sub Reward destination.
    ///   . `[writable, optional]` Sub Reward token pool.
    ClaimReward,

    /// Update the earliest_reward_claim_tim
    /// 0. `[signer]` admin authority .
    /// 1. `[writable]` Staking Pool
    UpdateEarliestRewardClaimTime(Slot),

    ///Add Reward Supply
    /// 0. `[signer]` Transfer reward token authority (supply authority or admin authority).
    /// 1. `[writable]` Staking Pool
    /// 2. `[writable]` Reward token supply.
    /// 3. `[writable]` Reward token pool
    /// 4. `[]` staking program derived
    /// 5. `[]` Reward token mint.
    /// 6. `[optional]` Sub Reward token mint.
    /// 7. `[]` Token program.
    /// 8.`[]` Clock sysvar
    /// 9. `[writable, optional]` Sub Reward token supply.
    /// 10. `[writable, optional]` Sub Reward token pool
    ChangeRewardSupply(i64, Option<i64>),

    ///Change Staking Pool Owner
    /// 0. `[signer]` Current owner
    /// 1. `[writable]` Staking Pool
    ChangeOwner(Pubkey),

    ///Add Reward Supply
    /// 0. `[signer]` Admin authority.
    /// 1. `[writable]` Staking Pool
    /// 2. `[]` Clock sysvar
    ChangeDuration(i64),

    ///Add Sub reward
    /// 0. `[signer]` Admin authority.
    /// 1. `[signer]` Transfer sub reward token authority
    /// 2. `[writable]` Staking Pool
    /// 3. `[writable]` Sub Reward token supply.
    /// 4. `[writable]` Sub Reward token pool
    /// 5. `[]` Sub Reward token mint.
    /// 6. `[]` Staking program derived that owns reward token pool.
    /// 7. `[]` Token program.
    /// 8. `[]  Rent sysvar
    /// 9. `[]` Clock sysvar
    AddSubRewardPool(u64),

    ///Change Staking Pool Owner
    /// 0. `[signer]` Current Admin
    /// 1. `[writable]` Staking Pool
    ChangeAdmin(Pubkey),
}

impl StakingInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        input
            .split_first()
            .ok_or_else(|| StakingError::InstructionUnpackError.into())
            .and_then(|(&tag, rest)| match tag {
                0 => {
                    let (supply, rest) = Self::unpack_u64(rest)?;
                    let (sub_supply, rest) = Self::unpack_option_u64(rest)?;
                    let (duration, rest) = Self::unpack_u64(rest)?;
                    let (earliest_reward_claim_time, rest) = Self::unpack_u64(rest)?;
                    let (bump_seed_staking_program, rest) = Self::unpack_u8(rest)?;
                    let (pool_owner_authority, rest) = Self::unpack_pubkey(rest)?;
                    let (admin_authority, rest) = Self::unpack_pubkey(rest)?;
                    Ok((
                        InitStakingPool {
                            supply,
                            sub_supply,
                            duration,
                            earliest_reward_claim_time,
                            bump_seed_staking_program,
                            pool_owner_authority,
                            admin_authority,
                        },
                        rest,
                    ))
                }
                1 => Ok((CreateStakeAccount, rest)),
                2 => {
                    let (amount, rest) = Self::unpack_u64(rest)?;
                    Ok((Deposit(amount), rest))
                }
                3 => {
                    let (amount, rest) = Self::unpack_u64(rest)?;
                    Ok((Withdraw(amount), rest))
                }
                4 => Ok((ClaimReward, rest)),
                5 => {
                    let (time, rest) = Self::unpack_u64(rest)?;
                    Ok((UpdateEarliestRewardClaimTime(time), rest))
                }
                6 => {
                    let (amount, rest) = Self::unpack_i64(rest)?;
                    let (sub_amount, rest) = Self::unpack_option_i64(rest)?;
                    Ok((ChangeRewardSupply(amount, sub_amount), rest))
                }
                7 => {
                    let (new_owner, rest) = Self::unpack_pubkey(rest)?;
                    Ok((ChangeOwner(new_owner), rest))
                }
                8 => {
                    let (amount, rest) = Self::unpack_i64(rest)?;
                    Ok((ChangeDuration(amount), rest))
                }
                9 => {
                    let (amount, rest) = Self::unpack_u64(rest)?;
                    Ok((AddSubRewardPool(amount), rest))
                }
                10 => {
                    let (new_owner, rest) = Self::unpack_pubkey(rest)?;
                    Ok((ChangeAdmin(new_owner), rest))
                }
                _ => {
                    msg!("Instruction cannot be unpacked");
                    Err(StakingError::InstructionUnpackError.into())
                }
            })
            .and_then(|(ins, rest)| {
                if rest.is_empty() {
                    Ok(ins)
                } else {
                    Err(StakingError::InstructionUnpackError.into())
                }
            })
    }
    fn unpack_pubkey(input: &[u8]) -> Result<(Pubkey, &[u8]), ProgramError> {
        if input.len() < PUBKEY_BYTES {
            msg!("Pubkey cannot be unpacked");
            return Err(StakingError::InstructionUnpackError.into());
        }
        let (key, rest) = input.split_at(PUBKEY_BYTES);
        let pk = Pubkey::new(key);
        Ok((pk, rest))
    }

    fn unpack_u64(input: &[u8]) -> Result<(u64, &[u8]), ProgramError> {
        if input.len() < 8 {
            msg!("u64 cannot be unpacked");
            return Err(StakingError::InstructionUnpackError.into());
        }
        let (bytes, rest) = input.split_at(8);
        let value = bytes
            .get(..8)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(StakingError::InstructionUnpackError)?;
        Ok((value, rest))
    }

    fn unpack_i64(input: &[u8]) -> Result<(i64, &[u8]), ProgramError> {
        if input.len() < 8 {
            msg!("i64 cannot be unpacked");
            return Err(StakingError::InstructionUnpackError.into());
        }
        let (bytes, rest) = input.split_at(8);
        let value = bytes
            .get(..8)
            .and_then(|slice| slice.try_into().ok())
            .map(i64::from_le_bytes)
            .ok_or(StakingError::InstructionUnpackError)?;
        Ok((value, rest))
    }

    fn unpack_u8(input: &[u8]) -> Result<(u8, &[u8]), ProgramError> {
        if input.is_empty() {
            msg!("u8 cannot be unpacked");
            return Err(StakingError::InstructionUnpackError.into());
        }
        let (bytes, rest) = input.split_at(1);
        let value = bytes
            .get(..1)
            .and_then(|slice| slice.try_into().ok())
            .map(u8::from_le_bytes)
            .ok_or(StakingError::InstructionUnpackError)?;
        Ok((value, rest))
    }

    fn unpack_option_u64(input: &[u8]) -> Result<(Option<u64>, &[u8]), ProgramError> {
        if input.len() < 1 + 8 {
            msg!("Option<u64> Pubkey cannot be unpacked, buffer length is not enough");
            return Err(StakingError::InstructionUnpackError.into());
        }
        let (option_u64, rest) = input.split_at(9);
        match option_u64[0] {
            0 => Ok((None, rest)),
            1 => {
                let num = u64::from_le_bytes(option_u64[1..9].try_into().unwrap());
                Ok((Some(num), rest))
            }
            _ => {
                msg!("Option<u64> cannot be unpacked");
                Err(StakingError::InstructionUnpackError.into())
            }
        }
    }

    fn unpack_option_i64(input: &[u8]) -> Result<(Option<i64>, &[u8]), ProgramError> {
        if input.len() < 1 + 8 {
            msg!("Option<i64> Pubkey cannot be unpacked, buffer length is not enough");
            return Err(StakingError::InstructionUnpackError.into());
        }
        let (option_i64, rest) = input.split_at(9);
        match option_i64[0] {
            0 => Ok((None, rest)),
            1 => {
                let num = i64::from_le_bytes(option_i64[1..9].try_into().unwrap());
                Ok((Some(num), rest))
            }
            _ => {
                msg!("Option<i64> cannot be unpacked");
                msg!("Option<i64> cannot be unpacked");
                Err(StakingError::InstructionUnpackError.into())
            }
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        match *self {
            Self::InitStakingPool {
                supply,
                sub_supply,
                duration,
                earliest_reward_claim_time,
                bump_seed_staking_program,
                pool_owner_authority,
                admin_authority,
            } => {
                buf.push(0);
                buf.extend_from_slice(&supply.to_le_bytes());
                Self::pack_option_u64(&mut buf, sub_supply);
                buf.extend_from_slice(&duration.to_le_bytes());
                buf.extend_from_slice(&earliest_reward_claim_time.to_le_bytes());
                buf.extend_from_slice(&bump_seed_staking_program.to_le_bytes());
                buf.extend_from_slice(pool_owner_authority.as_ref());
                buf.extend_from_slice(admin_authority.as_ref());
            }
            Self::CreateStakeAccount => {
                buf.push(1);
            }
            Self::Deposit(amount) => {
                buf.push(2);
                buf.extend_from_slice(&amount.to_le_bytes());
            }
            Self::Withdraw(amount) => {
                buf.push(3);
                buf.extend_from_slice(&amount.to_le_bytes());
            }
            Self::ClaimReward => {
                buf.push(4);
            }
            Self::UpdateEarliestRewardClaimTime(slot) => {
                buf.push(5);
                buf.extend_from_slice(&slot.to_le_bytes());
            }
            Self::ChangeRewardSupply(amount, sub_amount) => {
                buf.push(6);
                buf.extend_from_slice(&amount.to_le_bytes());
                Self::pack_option_i64(&mut buf, sub_amount);
            }
            Self::ChangeOwner(new_owner) => {
                buf.push(7);
                buf.extend_from_slice(new_owner.as_ref());
            }
            Self::ChangeDuration(amount) => {
                buf.push(8);
                buf.extend_from_slice(&amount.to_le_bytes());
            }
            Self::AddSubRewardPool(amount) => {
                buf.push(9);
                buf.extend_from_slice(&amount.to_le_bytes());
            }
            Self::ChangeAdmin(new_admin) => {
                buf.push(10);
                buf.extend_from_slice(new_admin.as_ref());
            }
        };
        buf
    }

    fn pack_option_u64(buf: &mut Vec<u8>, option_u64: Option<u64>) {
        match option_u64 {
            None => {
                buf.extend(vec![0; 9]);
            }
            Some(num) => {
                buf.push(1);
                buf.extend_from_slice(&num.to_le_bytes());
            }
        }
    }

    fn pack_option_i64(buf: &mut Vec<u8>, option_i64: Option<i64>) {
        match option_i64 {
            None => {
                buf.extend(vec![0; 9]);
            }
            Some(num) => {
                buf.push(1);
                buf.extend_from_slice(&num.to_le_bytes());
            }
        }
    }
}

//helpers
fn create_write_accounts(accounts: Vec<Pubkey>) -> impl Iterator<Item = AccountMeta> {
    accounts.into_iter().map(|acc| AccountMeta::new(acc, false))
}

fn create_read_accounts(accounts: Vec<Pubkey>) -> impl Iterator<Item = AccountMeta> {
    accounts
        .into_iter()
        .map(|acc| AccountMeta::new_readonly(acc, false))
}

pub fn change_owner(
    program_id: Pubkey,
    new_owner: Pubkey,
    old_owner: Pubkey,
    staking_pool: Pubkey,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new_readonly(old_owner, true),
        AccountMeta::new(staking_pool, false),
    ];
    Instruction {
        program_id,
        accounts,
        data: StakingInstruction::ChangeOwner(new_owner).pack(),
    }
}

pub fn change_admin(
    program_id: Pubkey,
    new_admin: Pubkey,
    old_admin: Pubkey,
    staking_pool: Pubkey,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new_readonly(old_admin, true),
        AccountMeta::new(staking_pool, false),
    ];
    Instruction {
        program_id,
        accounts,
        data: StakingInstruction::ChangeAdmin(new_admin).pack(),
    }
}

/// Creates an InitStakingPool instruction
#[allow(clippy::too_many_arguments)]
pub fn init_staking_pool(
    program_id: Pubkey,
    supply: u64,
    sub_supply: Option<u64>,
    duration: u64,
    earliest_reward_claim_time: Slot,
    transfer_reward_token_authority: Pubkey,
    reward_token_supply: Pubkey,
    reward_token_pool: Pubkey,
    sub_reward_token_supply: Option<Pubkey>,
    sub_reward_token_pool: Option<Pubkey>,
    staking_pool: Pubkey,
    reward_token_mint: Pubkey,
    sub_reward_token_mint: Option<Pubkey>,
    staking_pool_owner_derived: Pubkey,
    admin_authority: Pubkey,
) -> Instruction {
    let (staking_program_derived, bump_seed) =
        Pubkey::find_program_address(&[staking_pool.as_ref()], &program_id);
    let write_accounts = create_write_accounts(vec![
        reward_token_supply,
        reward_token_pool,
        sub_reward_token_supply.unwrap_or_else(|| dummy_id!()),
        sub_reward_token_pool.unwrap_or_else(|| dummy_id!()),
        staking_pool,
    ]);

    let read_accounts = create_read_accounts(vec![
        reward_token_mint,
        sub_reward_token_mint.unwrap_or_else(|| dummy_id!()),
        staking_program_derived,
        sysvar::rent::id(),
        spl_token::id(),
    ]);

    let accounts = vec![AccountMeta::new_readonly(
        transfer_reward_token_authority,
        true,
    )]
    .into_iter()
    .chain(write_accounts)
    .chain(read_accounts)
    .collect();

    Instruction {
        program_id,
        accounts,
        data: StakingInstruction::InitStakingPool {
            supply,
            sub_supply,
            duration,
            earliest_reward_claim_time,
            bump_seed_staking_program: bump_seed,
            pool_owner_authority: staking_pool_owner_derived,
            admin_authority,
        }
        .pack(),
    }
}

pub fn create_stake_account(
    program_id: Pubkey,
    stake_account: Pubkey,
    staking_pool: Pubkey,
    stake_account_owner: Pubkey,
) -> Instruction {
    let read_accounts =
        create_read_accounts(vec![staking_pool, stake_account_owner, sysvar::rent::id()]);

    let accounts = vec![AccountMeta::new(stake_account, false)]
        .into_iter()
        .chain(read_accounts)
        .collect();

    Instruction {
        program_id,
        accounts,
        data: StakingInstruction::CreateStakeAccount.pack(),
    }
}

pub fn deposit(
    program_id: Pubkey,
    amount: u64,
    authority: Pubkey,
    stake_account: Pubkey,
    staking_pool: Pubkey,
) -> Instruction {
    let write_accounts = create_write_accounts(vec![stake_account, staking_pool]);
    let accounts = vec![AccountMeta::new_readonly(authority, true)]
        .into_iter()
        .chain(write_accounts)
        .chain(vec![AccountMeta::new_readonly(sysvar::clock::id(), false)])
        .collect();

    Instruction {
        program_id,
        accounts,
        data: Deposit(amount).pack(),
    }
}

pub fn withdraw(
    program_id: Pubkey,
    amount: u64,
    authority: Pubkey,
    stake_account: Pubkey,
    staking_pool: Pubkey,
) -> Instruction {
    let write_accounts = create_write_accounts(vec![stake_account, staking_pool]);

    let accounts = vec![AccountMeta::new_readonly(authority, true)]
        .into_iter()
        .chain(write_accounts)
        .chain(vec![AccountMeta::new_readonly(sysvar::clock::id(), false)])
        .collect();

    Instruction {
        program_id,
        accounts,
        data: Withdraw(amount).pack(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn claim_reward(
    program_id: Pubkey,
    stake_account_owner: Pubkey,
    stake_account: Pubkey,
    staking_pool: Pubkey,
    reward_token_pool: Pubkey,
    sub_reward_token_pool: Option<Pubkey>,
    reward_destination: Pubkey,
    sub_reward_destination: Option<Pubkey>,
) -> Instruction {
    let (staking_program_derived, _bump_seed) =
        Pubkey::find_program_address(&[staking_pool.as_ref()], &program_id);

    let write_accounts = create_write_accounts(vec![
        stake_account,
        staking_pool,
        reward_token_pool,
        reward_destination,
    ]);

    let read_accounts = create_read_accounts(vec![
        staking_program_derived,
        sysvar::clock::id(),
        spl_token::id(),
    ]);

    let optional_accounts = create_write_accounts(
        if let Some([sub_reward_token_pool, sub_reward_dest]) =
            sub_reward_token_pool.and_then(|pool| sub_reward_destination.map(|dest| [pool, dest]))
        {
            vec![sub_reward_token_pool, sub_reward_dest]
        } else {
            vec![]
        },
    );
    let accounts = vec![AccountMeta::new_readonly(stake_account_owner, true)]
        .into_iter()
        .chain(write_accounts)
        .chain(read_accounts)
        .chain(optional_accounts)
        .collect();

    Instruction {
        program_id,
        accounts,
        data: ClaimReward.pack(),
    }
}

pub fn update_earliest_reward_claim_time(
    program_id: Pubkey,
    time: Slot,
    admin_authority: Pubkey,
    staking_pool: Pubkey,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new_readonly(admin_authority, true),
        AccountMeta::new(staking_pool, false),
    ];

    Instruction {
        program_id,
        accounts,
        data: UpdateEarliestRewardClaimTime(time).pack(),
    }
}

// Change reward supply instructions
#[allow(clippy::too_many_arguments)]
pub fn change_reward_supply(
    program_id: Pubkey,
    amount: i64,
    sub_amount: Option<i64>,
    transfer_reward_token_authority: Pubkey,
    reward_token_supply: Pubkey,
    reward_token_mint: Pubkey,
    staking_pool: Pubkey,
    reward_token_pool: Pubkey,
    sub_reward_token_supply: Option<Pubkey>,
    sub_reward_token_mint: Option<Pubkey>,
    sub_reward_token_pool: Option<Pubkey>,
) -> Instruction {
    let (staking_program_derived, _bump_seed) =
        Pubkey::find_program_address(&[staking_pool.as_ref()], &program_id);
    let write_accounts = create_write_accounts(vec![
        staking_pool,
        reward_token_supply,
        reward_token_pool,
        sub_reward_token_supply.unwrap_or_else(|| dummy_id!()),
        sub_reward_token_pool.unwrap_or_else(|| dummy_id!()),
    ]);
    let read_accounts = create_read_accounts(vec![
        staking_program_derived,
        reward_token_mint,
        sub_reward_token_mint.unwrap_or_else(|| dummy_id!()),
        spl_token::id(),
        sysvar::clock::id(),
    ]);
    let accounts = vec![AccountMeta::new_readonly(
        transfer_reward_token_authority,
        true,
    )]
    .into_iter()
    .chain(write_accounts)
    .chain(read_accounts)
    .collect();

    Instruction {
        program_id,
        accounts,
        data: StakingInstruction::ChangeRewardSupply(amount, sub_amount).pack(),
    }
}

// Change reward supply instructions
pub fn change_duration(
    program_id: Pubkey,
    amount: i64,
    admin_authority: Pubkey,
    staking_pool: Pubkey,
) -> Instruction {
    let write_accounts = create_write_accounts(vec![staking_pool]);
    let read_accounts = create_read_accounts(vec![sysvar::clock::id()]);
    let accounts = vec![AccountMeta::new_readonly(admin_authority, true)]
        .into_iter()
        .chain(write_accounts)
        .chain(read_accounts)
        .collect();

    Instruction {
        program_id,
        accounts,
        data: StakingInstruction::ChangeDuration(amount).pack(),
    }
}

// Add sub reward supply instructions
#[allow(clippy::too_many_arguments)]
pub fn add_sub_reward_pool(
    program_id: Pubkey,
    amount: u64,
    transfer_reward_token_authority: Pubkey,
    admin_authority: Pubkey,
    reward_token_supply: Pubkey,
    reward_token_mint: Pubkey,
    staking_pool: Pubkey,
    reward_token_pool: Pubkey,
) -> Instruction {
    let (staking_program_derived, _bump_seed) =
        Pubkey::find_program_address(&[staking_pool.as_ref()], &program_id);
    let write_accounts =
        create_write_accounts(vec![staking_pool, reward_token_supply, reward_token_pool]);
    let read_accounts = create_read_accounts(vec![
        reward_token_mint,
        staking_program_derived,
        spl_token::id(),
        sysvar::rent::id(),
        sysvar::clock::id(),
    ]);
    let accounts = vec![
        AccountMeta::new_readonly(admin_authority, true),
        AccountMeta::new_readonly(transfer_reward_token_authority, true),
    ]
    .into_iter()
    .chain(write_accounts)
    .chain(read_accounts)
    .collect();

    Instruction {
        program_id,
        accounts,
        data: StakingInstruction::AddSubRewardPool(amount).pack(),
    }
}
