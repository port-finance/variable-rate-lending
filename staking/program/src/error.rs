use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use thiserror::Error;

use crate::solana_program::decode_error::DecodeError;
use crate::solana_program::msg;
use crate::solana_program::program_error::{PrintProgramError, ProgramError};

#[derive(Clone, Debug, Eq, Error, FromPrimitive, PartialEq)]
pub enum StakingError {
    //0
    #[error("Failed to unpack instruction data")]
    InstructionUnpackError,
    #[error("Account is already initialized")]
    AlreadyInitialized,
    /// Lamport balance below rent-exempt threshold.
    #[error("Lamport balance below rent-exempt threshold")]
    NotRentExempt,
    /// Math operation overflow
    #[error("Math operation overflow")]
    MathOverflow,
    #[error("Stake Account deposits have zero value")]
    StakeDepositsZero,

    //5
    #[error("Stake Account withdraws have zero value")]
    StakeWithdrawsZero,
    #[error("Invalid Argument")]
    InvalidArgumentError,
    #[error("Supply to the staking pool must be non zero")]
    InvalidSupplyError,
    #[error("Duration to the staking pool must be non zero")]
    InvalidDurationError,
    #[error("Current Time must be greater than start time")]
    InvalidCurrentTimeError,

    //10
    #[error("Withdraw amount must be smaller than balance")]
    InvalidWithdrawAmountError,
    #[error("Reward rate must be monotonic increasing")]
    InvalidCurrentRateError,
    #[error("Invalid account owner")]
    InvalidAccountOwner,
    #[error("Insufficient token supply for rewarding")]
    InSufficientSupplyError,
    #[error("Not correct signer")]
    InvalidSigner,

    //15
    #[error("Reward supply token account is illegal")]
    InvalidRewardSupplyAccountError,
    #[error("Input token mint account is not valid")]
    InvalidTokenMint,
    #[error("Input token account is not owned by the correct token program id")]
    InvalidTokenOwner,
    #[error("Reward token pool must be owned by the staking program")]
    InvalidRewardTokenPoolOwner,
    #[error("Invalid reward token supply account")]
    InvalidRewardTokenSupplyAccount,

    //20
    #[error("Invalid staking pool")]
    InvalidStakingPool,
    #[error("Invalid stake account")]
    InvalidStakeAccount,
    #[error("Invalid reward token pool")]
    InvalidRewardTokenPool,
    #[error("Transfer token failed")]
    TokenTransferFailed,
    #[error("token account init failed")]
    TokenInitializeAccountFailed,

    //25
    #[error("Cannot reduce reward smaller than zero")]
    ReduceRewardTooMuch,
    #[error("The staking pool already has a sub reward")]
    AlreadyHasSubReward,
}

impl From<StakingError> for ProgramError {
    fn from(e: StakingError) -> Self {
        ProgramError::Custom(e as u32)
    }
}

impl<T> DecodeError<T> for StakingError {
    fn type_of() -> &'static str {
        "Staking Error"
    }
}

impl PrintProgramError for StakingError {
    fn print<E>(&self)
    where
        E: 'static + std::error::Error + DecodeError<E> + PrintProgramError + FromPrimitive,
    {
        msg!(&self.to_string());
    }
}
