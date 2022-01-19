use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};
use do_notation::{m, Lift};
use solana_program::entrypoint::ProgramResult;
use solana_program::pubkey::PUBKEY_BYTES;

use crate::error::StakingError;
use crate::math::{Decimal, TryAdd, TryMul, TrySub};
use crate::solana_program::program_error::ProgramError;
use crate::solana_program::program_pack::{IsInitialized, Pack, Sealed};
use crate::solana_program::{msg, pubkey::Pubkey};
use crate::state::{
    pack_option_decimal, unpack_option_decimal, PROGRAM_VERSION, UNINITIALIZED_VERSION,
};

use super::staking_pool::CumulativeRate;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct StakeAccount {
    /// Version of the struct
    pub version: u8,
    /// rate when last time the state changes
    pub start_rate: CumulativeRate,
    pub owner: Pubkey,
    pub pool_pubkey: Pubkey,
    pub deposited_amount: u64,
    pub unclaimed_reward_wads: Reward,
    // since rust on implement traits for array from 0..33 len
    pub reserve_fields2: [u8; 32],
    pub reserve_fields3: [u8; 32],
    pub reserve_fields4: [u8; 30],
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Reward {
    pub reward: Decimal,
    pub sub_reward: Option<Decimal>,
}

impl<T> From<(T, Option<T>)> for Reward
where
    T: Into<Decimal>,
{
    fn from((r, sub_r): (T, Option<T>)) -> Self {
        Self {
            reward: r.into(),
            sub_reward: sub_r.map(|x| x.into()),
        }
    }
}

impl TrySub for Reward {
    fn try_sub(self, rhs: Self) -> Result<Self, ProgramError> {
        let mut res = self;
        res.reward = res.reward.try_sub(rhs.reward)?;
        res.sub_reward = m! {
            lhs <- res.sub_reward;
            rhs <- rhs.sub_reward;
            Lift::lift(lhs.try_sub(rhs))
        }
        .map_or(Ok(None), |r| r.map(Some))?;
        Ok(res)
    }
}

impl TryAdd for Reward {
    fn try_add(self, rhs: Self) -> Result<Self, ProgramError> {
        let mut res = self;
        res.reward = res.reward.try_add(rhs.reward)?;
        res.sub_reward = m! {
            lhs <- res.sub_reward;
            rhs <- rhs.sub_reward;
            Lift::lift(lhs.try_add(rhs))
        }
        .map_or(Ok(None), |r| r.map(Some))?;
        Ok(res)
    }
}
impl Reward {
    pub fn accumulate_reward(&mut self, reward: Reward) -> ProgramResult {
        self.reward = self.reward.try_add(reward.reward)?;
        if reward.sub_reward.is_some() && self.sub_reward.is_none() {
            self.sub_reward = Some(Decimal::zero())
        }
        self.sub_reward = m! {
            lhs <- self.sub_reward;
            rhs <- reward.sub_reward;
            Lift::lift(lhs.try_add(rhs))
        }
        .map_or(Ok(None), |r| r.map(Some))?;
        Ok(())
    }
    pub fn try_floor_u64(&self) -> Result<(u64, Option<u64>), ProgramError> {
        let reward = self.reward.try_floor_u64()?;
        let sub_reward = self
            .sub_reward
            .as_ref()
            .map(Decimal::try_floor_u64)
            .map_or(Ok(None), |r| r.map(Some))?;
        Ok((reward, sub_reward))
    }
}

impl StakeAccount {
    pub fn init(&mut self, owner: Pubkey, pool: Pubkey) -> ProgramResult {
        self.owner = owner;
        self.version = PROGRAM_VERSION;
        self.pool_pubkey = pool;
        Ok(())
    }

    fn calculate_reward(&mut self, rate: CumulativeRate) -> Result<Reward, ProgramError> {
        let deposited_amount = self.deposited_amount;
        let calculate_reward = |current_rate: Decimal, start_rate: Decimal| {
            (m! {
                rate_diff <- current_rate.try_sub(start_rate);
                reward <- rate_diff.try_mul(deposited_amount);
                Lift::lift(reward)
            })
            .map_err(|_| {
                msg!("current rate smaller than start rate");
                StakingError::InvalidCurrentRateError
            })
        };
        let reward = calculate_reward(rate.reward, self.start_rate.reward)?;

        if self.start_rate.sub_reward.is_none() && rate.sub_reward.is_some() {
            self.start_rate.sub_reward = Some(Decimal::zero());
        }

        let sub_reward = m! {
            sub_rate <- rate.sub_reward;
            start_rate <- self.start_rate.sub_reward;
            Lift::lift(
                calculate_reward(sub_rate, start_rate)
            )
        }
        .map_or(Ok(None), |r: Result<Decimal, StakingError>| r.map(Some))?;

        Ok(Reward { reward, sub_reward })
    }
    pub fn deposit(&mut self, current_rate: CumulativeRate, amount: u64) -> ProgramResult {
        if amount == 0 {
            msg!("Cannot deposit zero amount");
            return Err(StakingError::StakeDepositsZero.into());
        }

        let reward = self.calculate_reward(current_rate)?;

        self.unclaimed_reward_wads.accumulate_reward(reward)?;
        self.deposited_amount = self
            .deposited_amount
            .checked_add(amount)
            .ok_or(StakingError::MathOverflow)?;
        self.start_rate = current_rate;
        Ok(())
    }

    pub fn withdraw(&mut self, current_rate: CumulativeRate, amount: u64) -> ProgramResult {
        if amount == 0 {
            msg!("Cannot withdraw zero amount");
            return Err(StakingError::StakeWithdrawsZero.into());
        }

        let reward = self.calculate_reward(current_rate)?;
        self.unclaimed_reward_wads.accumulate_reward(reward)?;

        self.deposited_amount = self
            .deposited_amount
            .checked_sub(amount)
            .ok_or(StakingError::InvalidWithdrawAmountError)?;
        self.start_rate = current_rate;
        Ok(())
    }

    pub fn claim_reward(
        &mut self,
        current_rate: CumulativeRate,
    ) -> Result<(u64, Option<u64>), ProgramError> {
        let reward = self.calculate_reward(current_rate)?;
        self.unclaimed_reward_wads.accumulate_reward(reward)?;
        let reward_lamports = self.unclaimed_reward_wads.try_floor_u64()?;
        self.unclaimed_reward_wads = self.unclaimed_reward_wads.try_sub(reward_lamports.into())?;
        self.start_rate = current_rate;
        Ok(reward_lamports)
    }
}
impl Sealed for StakeAccount {}
impl IsInitialized for StakeAccount {
    fn is_initialized(&self) -> bool {
        self.version != UNINITIALIZED_VERSION
    }
}
impl Pack for StakeAccount {
    const LEN: usize = 1
        + Decimal::LEN
        + PUBKEY_BYTES
        + PUBKEY_BYTES
        + 8
        + Decimal::LEN
        + Decimal::LEN
        + 1
        + Decimal::LEN
        + 1
        + 94;
    fn pack_into_slice(&self, dst: &mut [u8]) {
        let output = array_mut_ref![dst, 0, StakeAccount::LEN];
        #[allow(clippy::ptr_offset_with_cast)]
        let (
            version,
            start_rate,
            owner,
            pool_pubkey,
            deposited_value,
            unclaimed_reward_wads,
            sub_start_rate,
            sub_unclaimed_reward_wads,
            _,
        ) = mut_array_refs![
            output,
            1,
            Decimal::LEN,
            PUBKEY_BYTES,
            PUBKEY_BYTES,
            8,
            Decimal::LEN,
            Decimal::LEN + 1,
            Decimal::LEN + 1,
            94
        ];
        *version = self.version.to_le_bytes();
        self.start_rate.reward.pack_into_slice(start_rate);
        pack_option_decimal(&self.start_rate.sub_reward, sub_start_rate);
        owner.copy_from_slice(self.owner.as_ref());
        pool_pubkey.copy_from_slice(self.pool_pubkey.as_ref());
        *deposited_value = self.deposited_amount.to_le_bytes();
        self.unclaimed_reward_wads
            .reward
            .pack_into_slice(unclaimed_reward_wads);

        pack_option_decimal(
            &self.unclaimed_reward_wads.sub_reward,
            sub_unclaimed_reward_wads,
        );
    }
    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let input = array_ref![src, 0, StakeAccount::LEN];
        #[allow(clippy::ptr_offset_with_cast)]
        let (
            version,
            start_rate,
            owner,
            pool_pubkey,
            deposited_value,
            unclaimed_reward_wads,
            sub_start_rate,
            sub_unclaimed_reward_wads,
            _,
        ) = array_refs![
            input,
            1,
            Decimal::LEN,
            PUBKEY_BYTES,
            PUBKEY_BYTES,
            8,
            Decimal::LEN,
            Decimal::LEN + 1,
            Decimal::LEN + 1,
            94
        ];
        let version = u8::from_le_bytes(*version);
        if version > PROGRAM_VERSION {
            msg!("stake account version does not match staking program version");
            return Err(ProgramError::InvalidAccountData);
        }
        let start_rate = Decimal::unpack_from_slice(start_rate)?;
        let sub_start_rate = unpack_option_decimal(sub_start_rate)?;
        let owner = Pubkey::new_from_array(*owner);
        let pool_pubkey = Pubkey::new_from_array(*pool_pubkey);
        let deposited_value = u64::from_le_bytes(*deposited_value);
        let reward = Decimal::unpack_from_slice(unclaimed_reward_wads)?;
        let sub_reward = unpack_option_decimal(sub_unclaimed_reward_wads)?;

        let reserve_field = [0; 32];
        Ok(Self {
            version,
            start_rate: CumulativeRate {
                reward: start_rate,
                sub_reward: sub_start_rate,
            },
            owner,
            pool_pubkey,
            deposited_amount: deposited_value,
            unclaimed_reward_wads: Reward { reward, sub_reward },
            reserve_fields2: reserve_field,
            reserve_fields3: reserve_field,
            reserve_fields4: [0; 30],
        })
    }
}
