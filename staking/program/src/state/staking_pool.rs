use std::cmp::min;
use std::convert::TryInto;

use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};
use do_notation::{m, Lift};
use num_traits::abs;
use solana_program::clock::Slot;
use solana_program::entrypoint::ProgramResult;
use solana_program::program_error::ProgramError;
use solana_program::pubkey::PUBKEY_BYTES;
use solana_program::{msg, pubkey::Pubkey};

use crate::error::StakingError;
use crate::math::{Decimal, TryAdd, TryDiv, TryMul, TrySub};
use crate::solana_program::program_pack::{IsInitialized, Pack, Sealed};
use crate::state::{
    pack_option_decimal, pack_option_key, unpack_option_decimal, unpack_option_key,
    PROGRAM_VERSION, UNINITIALIZED_VERSION,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StakingPool {
    /// Version of the struct
    pub version: u8,
    pub owner_authority: Pubkey,
    pub admin_authority: Pubkey,
    pub reward_token_pool: Pubkey,
    pub last_update: Slot, // last time the state changes
    pub end_time: Slot,
    pub duration: u64,
    pub earliest_reward_claim_time: Slot,
    pub rate_per_slot: RatePerSlot,
    pub cumulative_rate: CumulativeRate,
    pub pool_size: u64,
    pub bump_seed_staking_program: u8,
    pub sub_reward_token_pool: Option<Pubkey>,
    pub reserve_fields3: [u8; 32],
    pub reserve_fields4: [u8; 29],
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct RatePerSlot {
    pub reward: Decimal,
    pub sub_reward: Option<Decimal>,
}

impl RatePerSlot {
    pub fn init(supply: u64, sub_supply: Option<u64>, duration: u64) -> Result<Self, ProgramError> {
        Ok(RatePerSlot {
            reward: Decimal::from(supply).try_div(duration)?,
            sub_reward: sub_supply
                .map(|x| Decimal::from(x).try_div(duration))
                .map_or(Ok(None), |r| r.map(Some))?,
        })
    }

    pub fn try_floor_u64(&self) -> Result<(u64, Option<u64>), ProgramError> {
        Ok((
            self.reward.try_floor_u64()?,
            self.sub_reward
                .as_ref()
                .map(Decimal::try_floor_u64)
                .map_or(Ok(None), |r| r.map(Some))?,
        ))
    }

    pub fn clear(&mut self) {
        self.reward = Decimal::zero();
        self.sub_reward = self.sub_reward.map(|_| Decimal::zero());
    }
}

impl TryDiv<u64> for RatePerSlot {
    /// Divide
    fn try_div(self, rhs: u64) -> Result<Self, ProgramError> {
        Ok(Self {
            reward: self.reward.try_div(rhs)?,
            sub_reward: m! {
                lhs <- self.sub_reward;
                Lift::lift(lhs.try_div(Decimal::from(rhs)))
            }
            .map_or(Ok(None), |r| r.map(Some))?,
        })
    }
}

/// Try to multiply, return an error on overflow
impl TryMul<u64> for RatePerSlot {
    /// Multiply
    fn try_mul(self, rhs: u64) -> Result<Self, ProgramError> {
        Ok(Self {
            reward: self.reward.try_mul(rhs)?,
            sub_reward: m! {
                lhs <- self.sub_reward;
                Lift::lift(lhs.try_mul(Decimal::from(rhs)))
            }
            .map_or(Ok(None), |r| r.map(Some))?,
        })
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct CumulativeRate {
    pub reward: Decimal,
    pub sub_reward: Option<Decimal>,
}

impl CumulativeRate {
    pub fn accumulate_rate(
        &self,
        rate: &RatePerSlot,
        time_elapsed: u64,
        pool_size: u64,
    ) -> Result<Self, ProgramError> {
        m! {
            total_reward <- rate.try_mul(time_elapsed);
            reward_per_lamport <- total_reward.try_div(pool_size);
            reward <- self.reward.try_add(reward_per_lamport.reward);
            sub_reward <- (m! {
                rhs <- self.sub_reward;
                lhs <- reward_per_lamport.sub_reward;
                Lift::lift(rhs.try_add(lhs))
            }).map_or(Ok(None), |r| r.map(Some));
            Lift::lift(Self {
              reward,
              sub_reward
            })
        }
    }
}

impl StakingPool {
    #[allow(clippy::too_many_arguments)]
    pub fn init(
        &mut self,
        owner_authority: Pubkey,
        admin_authority: Pubkey,
        reward_token_pool_pubkey: Pubkey,
        sub_reward_token_pool_pubkey: Option<Pubkey>,
        duration: u64,
        supply: u64,
        sub_supply: Option<u64>,
        earliest_reward_claim_time: Slot,
        bump_seed_staking_program: u8,
    ) -> ProgramResult {
        if supply == 0 {
            Err(StakingError::InvalidSupplyError.into())
        } else if duration == 0 {
            Err(StakingError::InvalidDurationError.into())
        } else {
            self.version = PROGRAM_VERSION;
            self.owner_authority = owner_authority;
            self.admin_authority = admin_authority;
            self.reward_token_pool = reward_token_pool_pubkey;
            self.duration = duration;
            self.rate_per_slot = RatePerSlot::init(supply, sub_supply, duration)?;
            self.earliest_reward_claim_time = earliest_reward_claim_time;
            self.bump_seed_staking_program = bump_seed_staking_program;
            self.sub_reward_token_pool = sub_reward_token_pool_pubkey;
            self.cumulative_rate.sub_reward = sub_supply.map(|_| Decimal::zero());
            Ok(())
        }
    }
    fn claim_reward_helper(&mut self, current_time: Slot) -> Result<CumulativeRate, ProgramError> {
        let mark_time = min(current_time, self.end_time);
        let time_elapsed = mark_time
            .checked_sub(self.last_update)
            .ok_or(StakingError::InvalidCurrentTimeError)?;

        self.last_update = mark_time;
        self.cumulative_rate
            .accumulate_rate(&self.rate_per_slot, time_elapsed, self.pool_size)
    }

    pub fn claim_reward(&mut self, current_time: Slot) -> Result<CumulativeRate, ProgramError> {
        self.cumulative_rate = self.claim_reward_helper(current_time)?;
        Ok(self.cumulative_rate)
    }

    pub fn update_reward_supply(
        &mut self,
        amount: i64,
        sub_amount: Option<i64>,
        current_time: Slot,
    ) -> ProgramResult {
        let time_to_end = self
            .end_time
            .checked_sub(current_time)
            .ok_or(StakingError::InvalidArgumentError)?;
        self.cumulative_rate = self.claim_reward_helper(current_time)?;
        let reward_rate_change = Decimal::from(abs(amount) as u64).try_div(time_to_end)?;
        let sub_reward_rate_change = sub_amount
            .map(|x| Decimal::from(abs(x) as u64).try_div(time_to_end))
            .map_or(Ok(None), |r| r.map(Some))?;

        if amount > 0 {
            self.rate_per_slot.reward = self.rate_per_slot.reward.try_add(reward_rate_change)?;
        } else {
            self.rate_per_slot.reward = self
                .rate_per_slot
                .reward
                .try_sub(reward_rate_change)
                .map_err(|_| StakingError::ReduceRewardTooMuch)?;
        }

        if let Some(sub_amount) = sub_amount {
            if sub_amount > 0 {
                self.rate_per_slot.sub_reward = (m! {
                    reward_rate <- self.rate_per_slot.sub_reward;
                        Lift::lift(reward_rate.try_add(sub_reward_rate_change.unwrap_or_else(Decimal::zero)))
                    }).map_or(Ok(None), |r| r.map(Some))?;
            } else {
                self.rate_per_slot.sub_reward = (m! {
                    reward_rate <- self.rate_per_slot.sub_reward;
                        Lift::lift(reward_rate.try_sub(sub_reward_rate_change.unwrap_or_else(Decimal::zero)))
                    }).map_or(Ok(None), |r| r.map(Some)).map_err(|_| StakingError::ReduceRewardTooMuch)?;
            }
        }

        Ok(())
    }

    pub fn add_sub_reward(
        &mut self,
        amount: u64,
        current_time: Slot,
        token_pool: Pubkey,
    ) -> ProgramResult {
        if self.sub_reward_token_pool.is_some() {
            return Err(StakingError::AlreadyHasSubReward.into());
        }
        self.cumulative_rate = self.claim_reward_helper(current_time)?;
        self.sub_reward_token_pool = Some(token_pool);
        let time_to_end = self
            .end_time
            .checked_sub(current_time)
            .ok_or(StakingError::InvalidArgumentError)?;
        self.cumulative_rate.sub_reward = Some(Decimal::zero());
        self.rate_per_slot.sub_reward = Some(Decimal::from(amount).try_div(time_to_end)?);
        Ok(())
    }

    pub fn extend_duration(&mut self, extend_amount: i64, current_time: Slot) -> ProgramResult {
        if self.end_time == 0 {
            let duration = self.duration;
            self.duration = m! {
                u <- extend_amount.try_into().map_err(|_| StakingError::MathOverflow);
                duration.checked_add(u).ok_or(StakingError::MathOverflow)
            }?;
            let supply = self.rate_per_slot.try_mul(duration)?;
            self.rate_per_slot = supply.try_div(self.duration)?;
            return Ok(());
        }
        self.cumulative_rate = self.claim_reward_helper(current_time)?;
        if let Some(time_to_end) = self.end_time.checked_sub(current_time) {
            if extend_amount < 0 && abs(extend_amount) as u64 >= time_to_end {
                msg!("Cannot change duration to the time before current slot");
                return Err(StakingError::InvalidArgumentError.into());
            }
            let (reward_amount, sub_reward_amount) = m! {
                d <- self.rate_per_slot.try_mul(time_to_end);
                us <- d.try_floor_u64();
                reward_i <- us.0.try_into().map_err(|_| StakingError::MathOverflow.into());
                sub_reward_i <- us.1.map(|x| x.try_into().map_err(|_| StakingError::MathOverflow.into())).map_or(
                    Ok(None), |r| r.map(Some)
                );
                Lift::lift((reward_i, sub_reward_i))
            }?;
            if extend_amount > 0 {
                self.end_time += extend_amount as u64;
                self.duration += extend_amount as u64;
            } else {
                self.end_time -= abs(extend_amount) as u64;
                self.duration -= abs(extend_amount) as u64;
            }
            self.rate_per_slot.clear();
            self.update_reward_supply(reward_amount, sub_reward_amount, current_time)?;
        } else {
            if extend_amount > 0 {
                self.end_time += extend_amount as u64;
                self.duration += extend_amount as u64;
            } else {
                msg!("You can only extend not retract when the mining has already ended");
                return Err(StakingError::InvalidArgumentError.into());
            }
            self.rate_per_slot.clear()
        }
        Ok(())
    }
    pub fn deposit(
        &mut self,
        current_time: Slot,
        amount: u64,
    ) -> Result<CumulativeRate, ProgramError> {
        if amount == 0 {
            msg!("Cannot deposit zero amount");
            return Err(StakingError::StakeDepositsZero.into());
        }
        if self.pool_size == 0u64 {
            self.end_time = current_time
                .checked_add(self.duration)
                .ok_or(StakingError::MathOverflow)?;
            self.last_update = current_time;
        } else {
            self.cumulative_rate = self.claim_reward_helper(current_time)?;
        }

        self.pool_size = self
            .pool_size
            .checked_add(amount)
            .ok_or(StakingError::MathOverflow)?;
        Ok(self.cumulative_rate)
    }

    pub fn withdraw(
        &mut self,
        current_time: Slot,
        amount: u64,
    ) -> Result<CumulativeRate, ProgramError> {
        if amount == 0 {
            msg!("Cannot withdraw zero amount");
            return Err(StakingError::StakeWithdrawsZero.into());
        }

        self.cumulative_rate = self.claim_reward_helper(current_time)?;
        self.pool_size = self
            .pool_size
            .checked_sub(amount)
            .ok_or(StakingError::InvalidWithdrawAmountError)?;
        Ok(self.cumulative_rate)
    }
}

impl Sealed for StakingPool {}
impl IsInitialized for StakingPool {
    fn is_initialized(&self) -> bool {
        self.version != UNINITIALIZED_VERSION
    }
}
impl Pack for StakingPool {
    const LEN: usize = 1
        + PUBKEY_BYTES
        + PUBKEY_BYTES
        + PUBKEY_BYTES
        + 8
        + 8
        + 8
        + 8
        + Decimal::LEN
        + Decimal::LEN
        + 8
        + 1
        + PUBKEY_BYTES
        + 1
        + Decimal::LEN
        + 1
        + Decimal::LEN
        + 1
        + 61;

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let output = array_mut_ref![dst, 0, StakingPool::LEN];
        #[allow(clippy::ptr_offset_with_cast)]
        let (
            version,
            owner_authority,
            admin_authority,
            supply_pubkey,
            last_update,
            end_time,
            duration,
            earliest_reward_claim_time,
            rate_per_slot,
            cumulative_rate,
            pool_size,
            bump_seed_staking_program,
            sub_reward_token_pool,
            sub_rate_per_slot,
            sub_cumulative_rate,
            _,
        ) = mut_array_refs![
            output,
            1,
            PUBKEY_BYTES,
            PUBKEY_BYTES,
            PUBKEY_BYTES,
            8,
            8,
            8,
            8,
            Decimal::LEN,
            Decimal::LEN,
            8,
            1,
            PUBKEY_BYTES + 1,
            Decimal::LEN + 1,
            Decimal::LEN + 1,
            61
        ];
        *version = self.version.to_le_bytes();
        owner_authority.copy_from_slice(self.owner_authority.as_ref());
        admin_authority.copy_from_slice(self.admin_authority.as_ref());
        supply_pubkey.copy_from_slice(self.reward_token_pool.as_ref());
        *last_update = self.last_update.to_le_bytes();
        *end_time = self.end_time.to_le_bytes();
        *duration = self.duration.to_le_bytes();
        *earliest_reward_claim_time = self.earliest_reward_claim_time.to_le_bytes();
        self.rate_per_slot.reward.pack_into_slice(rate_per_slot);
        pack_option_decimal(&self.rate_per_slot.sub_reward, sub_rate_per_slot);
        self.cumulative_rate.reward.pack_into_slice(cumulative_rate);
        pack_option_decimal(&self.cumulative_rate.sub_reward, sub_cumulative_rate);
        *pool_size = self.pool_size.to_le_bytes();
        *bump_seed_staking_program = self.bump_seed_staking_program.to_le_bytes();
        pack_option_key(&self.sub_reward_token_pool, sub_reward_token_pool);
    }
    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let input = array_ref![src, 0, StakingPool::LEN];
        #[allow(clippy::ptr_offset_with_cast)]
        let (
            version,
            owner_authority,
            admin_authority,
            supply_pubkey,
            last_update,
            end_time,
            duration,
            earliest_reward_claim_time,
            rate_per_slot,
            cumulative_rate,
            pool_size,
            bump_seed_staking_program,
            sub_reward_token_pool,
            sub_rate_per_slot,
            sub_cumulative_rate,
            _,
        ) = array_refs![
            input,
            1,
            PUBKEY_BYTES,
            PUBKEY_BYTES,
            PUBKEY_BYTES,
            8,
            8,
            8,
            8,
            Decimal::LEN,
            Decimal::LEN,
            8,
            1,
            PUBKEY_BYTES + 1,
            Decimal::LEN + 1,
            Decimal::LEN + 1,
            61
        ];
        let version = u8::from_le_bytes(*version);
        if version > PROGRAM_VERSION {
            msg!("staking pool version does not match staking program version");
            return Err(ProgramError::InvalidAccountData);
        }
        let owner_authority = Pubkey::new_from_array(*owner_authority);
        let admin_authority = Pubkey::new_from_array(*admin_authority);
        let supply_pubkey = Pubkey::new_from_array(*supply_pubkey);
        let last_update = Slot::from_le_bytes(*last_update);
        let end_time = Slot::from_le_bytes(*end_time);
        let duration = u64::from_le_bytes(*duration);
        let earliest_reward_claim_time = Slot::from_le_bytes(*earliest_reward_claim_time);
        let rate_per_slot = Decimal::unpack_from_slice(rate_per_slot)?;

        let sub_rate_per_slot = unpack_option_decimal(sub_rate_per_slot)?;
        let cumulative_rate = Decimal::unpack_from_slice(cumulative_rate)?;
        let sub_cumulative_rate = unpack_option_decimal(sub_cumulative_rate)?;
        let pool_size = u64::from_le_bytes(*pool_size);
        let bump_seed_staking_program = u8::from_le_bytes(*bump_seed_staking_program);
        let sub_reward_token_pool = unpack_option_key(sub_reward_token_pool)?;

        let reserve_field = [0; 32];
        Ok(StakingPool {
            version,
            owner_authority,
            admin_authority,
            reward_token_pool: supply_pubkey,
            last_update,
            end_time,
            duration,
            earliest_reward_claim_time,
            rate_per_slot: RatePerSlot {
                reward: rate_per_slot,
                sub_reward: sub_rate_per_slot,
            },
            cumulative_rate: CumulativeRate {
                reward: cumulative_rate,
                sub_reward: sub_cumulative_rate,
            },
            pool_size,
            bump_seed_staking_program,
            sub_reward_token_pool,
            reserve_fields3: reserve_field,
            reserve_fields4: [0; 29],
        })
    }
}
