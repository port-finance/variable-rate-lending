//! Instruction types

use std::{convert::TryInto, mem::size_of};

use arrayref::array_refs;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    msg,
    program_error::ProgramError,
    pubkey::{Pubkey, PUBKEY_BYTES},
    sysvar,
};
use spl_token::solana_program::program_option::COption;

use crate::math::Decimal;
use crate::{
    error::LendingError,
    state::{pack_coption_key_compact, unpack_coption_key_compact, ReserveConfig, ReserveFees},
};

/// Instructions supported by the lending program.
#[derive(Clone, Debug, PartialEq)]
pub enum LendingInstruction {
    // 0
    /// Initializes a new lending market.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Lending market account - uninitialized.
    ///   1. `[]` Rent sysvar.
    ///   2. `[]` Token program id.
    InitLendingMarket {
        /// Owner authority which can add new reserves
        owner: Pubkey,
        /// Currency market prices are quoted in
        /// e.g. "USD" null padded (`*b"USD\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0"`) or a SPL token mint pubkey
        quote_currency: [u8; 32],
    },

    // 1
    /// Sets the new owner of a lending market.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Lending market account.
    ///   1. `[signer]` Current owner.
    SetLendingMarketOwner {
        /// The new owner
        new_owner: Pubkey,
    },

    // 2
    /// Initializes a new lending market reserve.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source liquidity token account.
    ///                     $authority can transfer $liquidity_amount.
    ///   1. `[writable]` Destination collateral token account - uninitialized.
    ///   2. `[writable]` Reserve account - uninitialized.
    ///   3. `[]` Reserve liquidity SPL Token mint.
    ///   4. `[writable]` Reserve liquidity supply SPL Token account - uninitialized.
    ///   5. `[writable]` Reserve liquidity fee receiver - uninitialized.
    ///   6. `[writable]` Reserve collateral SPL Token mint - uninitialized.
    ///   7. `[writable]` Reserve collateral token supply - uninitialized.
    ///   8 `[]` Lending market account.
    ///   9 `[]` Derived lending market authority.
    ///   10 `[signer]` Lending market owner.
    ///   11 `[signer]` User transfer authority ($authority).
    ///   12 `[]` Clock sysvar.
    ///   13 `[]` Rent sysvar.
    ///   14 `[]` Token program id.
    ///   15 `[optional]` Oracle price account, pyth or switchboard.
    ///           This will be used as the reserve liquidity oracle account.
    InitReserve {
        /// Initial amount of liquidity to deposit into the new reserve
        liquidity_amount: u64,
        /// Fixed the price for this reserve
        fixed_price: COption<Decimal>,
        /// Reserve configuration values
        config: ReserveConfig,
    },

    // 3
    /// Accrue interest and update market price of liquidity on a reserve.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Reserve account.
    ///   1. `[]` Clock sysvar.
    ///   2. `[]` Reserve liquidity oracle account.
    ///             Must be the Pyth price account specified at InitReserve.
    RefreshReserve,

    // 4
    /// Deposit liquidity into a reserve in exchange for collateral. Collateral represents a share
    /// of the reserve liquidity pool.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source liquidity token account.
    ///                     $authority can transfer $liquidity_amount.
    ///   1. `[writable]` Destination collateral token account.
    ///   2. `[writable]` Reserve account.
    ///   3. `[writable]` Reserve liquidity supply SPL Token account.
    ///   4. `[writable]` Reserve collateral SPL Token mint.
    ///   5. `[]` Lending market account.
    ///   6. `[]` Derived lending market authority.
    ///   7. `[signer]` User transfer authority ($authority).
    ///   8. `[]` Clock sysvar.
    ///   9. `[]` Token program id.
    DepositReserveLiquidity {
        /// Amount of liquidity to deposit in exchange for collateral tokens
        liquidity_amount: u64,
    },

    // 5
    /// Redeem collateral from a reserve in exchange for liquidity.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source collateral token account.
    ///                     $authority can transfer $collateral_amount.
    ///   1. `[writable]` Destination liquidity token account.
    ///   2. `[writable]` Reserve account.
    ///   3. `[writable]` Reserve collateral SPL Token mint.
    ///   4. `[writable]` Reserve liquidity supply SPL Token account.
    ///   5. `[]` Lending market account.
    ///   6. `[]` Derived lending market authority.
    ///   7. `[signer]` User transfer authority ($authority).
    ///   8. `[]` Clock sysvar.
    ///   9. `[]` Token program id.
    RedeemReserveCollateral {
        /// Amount of collateral tokens to redeem in exchange for liquidity
        collateral_amount: u64,
    },

    // 6
    /// Initializes a new lending market obligation.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Obligation account - uninitialized.
    ///   1. `[]` Lending market account.
    ///   2. `[signer]` Obligation owner.
    ///   3. `[]` Clock sysvar.
    ///   4. `[]` Rent sysvar.
    ///   5. `[]` Token program id.
    InitObligation,

    // 7
    /// Refresh an obligation's accrued interest and collateral and liquidity prices. Requires
    /// refreshed reserves, as all obligation collateral deposit reserves in order, followed by all
    /// liquidity borrow reserves in order.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Obligation account.
    ///   1. `[]` Clock sysvar.
    ///   .. `[]` Collateral deposit reserve accounts - refreshed, all, in order.
    ///   .. `[]` Liquidity borrow reserve accounts - refreshed, all, in order.
    RefreshObligation,

    // 8
    /// Deposit collateral to an obligation. Requires a refreshed reserve.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source collateral token account.
    ///                     Minted by deposit reserve collateral mint.
    ///                     $authority can transfer $collateral_amount.
    ///   1. `[writable]` Destination deposit reserve collateral supply SPL Token account.
    ///   2. `[]` Deposit reserve account - refreshed.
    ///   3. `[writable]` Obligation account.
    ///   4. `[]` Lending market account.
    ///   5. `[]` Derived lending market authority.
    ///   6. `[signer]` Obligation owner.
    ///   7. `[signer]` User transfer authority ($authority).
    ///   8. `[]` Clock sysvar.
    ///   9. `[]` Token program id.
    ///   10 `[writable, optional]` Stake account.
    ///   11 `[writable, optional]` Staking pool.
    ///   12 `[optional]` staking program id.
    DepositObligationCollateral {
        /// Amount of collateral tokens to deposit
        collateral_amount: u64,
    },

    // 9
    /// Withdraw collateral from an obligation. Requires a refreshed obligation and reserve.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source withdraw reserve collateral supply SPL Token account.
    ///   1. `[writable]` Destination collateral token account.
    ///                     Minted by withdraw reserve collateral mint.
    ///   2. `[]` Withdraw reserve account - refreshed.
    ///   3. `[writable]` Obligation account - refreshed.
    ///   4. `[]` Lending market account.
    ///   5. `[]` Derived lending market authority.
    ///   6. `[signer]` Obligation owner.
    ///   7. `[]` Clock sysvar.
    ///   8. `[]` Token program id.
    ///   9. `[writable, optional]` Stake account.
    ///   10 `[writable, optional]` Staking pool.
    ///   11 `[optional]` staking program id.
    WithdrawObligationCollateral {
        /// Amount of collateral tokens to withdraw - u64::MAX for up to 100% of deposited amount
        collateral_amount: u64,
    },

    // 10
    /// Borrow liquidity from a reserve by depositing collateral tokens. Requires a refreshed
    /// obligation and reserve.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source borrow reserve liquidity supply SPL Token account.
    ///   1. `[writable]` Destination liquidity token account.
    ///                     Minted by borrow reserve liquidity mint.
    ///   2. `[writable]` Borrow reserve account - refreshed.
    ///   3. `[writable]` Borrow reserve liquidity fee receiver account.
    ///                     Must be the fee account specified at InitReserve.
    ///   4. `[writable]` Obligation account - refreshed.
    ///   5. `[]` Lending market account.
    ///   6. `[]` Derived lending market authority.
    ///   7. `[signer]` Obligation owner.
    ///   8. `[]` Clock sysvar.
    ///   9. `[]` Token program id.
    BorrowObligationLiquidity {
        /// Amount of liquidity to borrow - u64::MAX for 100% of borrowing power
        liquidity_amount: u64,
        // @TODO: slippage constraint - https://git.io/JmV67
    },

    // 11
    /// Repay borrowed liquidity to a reserve. Requires a refreshed obligation and reserve.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source liquidity token account.
    ///                     Minted by repay reserve liquidity mint.
    ///                     $authority can transfer $liquidity_amount.
    ///   1. `[writable]` Destination repay reserve liquidity supply SPL Token account.
    ///   2. `[writable]` Repay reserve account - refreshed.
    ///   3. `[writable]` Obligation account - refreshed.
    ///   4. `[]` Lending market account.
    ///   5. `[signer]` User transfer authority ($authority).
    ///   6. `[]` Clock sysvar.
    ///   7. `[]` Token program id.
    RepayObligationLiquidity {
        /// Amount of liquidity to repay - u64::MAX for 100% of borrowed amount
        liquidity_amount: u64,
    },

    // 12
    /// Repay borrowed liquidity to a reserve to receive collateral at a discount from an unhealthy
    /// obligation. Requires a refreshed obligation and reserves.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source liquidity token account.
    ///                     Minted by repay reserve liquidity mint.
    ///                     $authority can transfer $liquidity_amount.
    ///   1. `[writable]` Destination collateral token account.
    ///                     Minted by withdraw reserve collateral mint.
    ///   2. `[writable]` Repay reserve account - refreshed.
    ///   3. `[writable]` Repay reserve liquidity supply SPL Token account.
    ///   4. `[]` Withdraw reserve account - refreshed.
    ///   5. `[writable]` Withdraw reserve collateral supply SPL Token account.
    ///   6. `[writable]` Obligation account - refreshed.
    ///   7. `[]` Lending market account.
    ///   8. `[]` Derived lending market authority.
    ///   9. `[signer]` User transfer authority ($authority).
    ///   10 `[]` Clock sysvar.
    ///   11 `[]` Token program id.
    ///   14 `[writable, optional]` Deposit stake account.
    ///   15 `[writable, optional]` Deposit staking pool.
    ///   16 `[optional]` staking program id.
    LiquidateObligation {
        /// Amount of liquidity to repay - u64::MAX for up to 100% of borrowed amount
        liquidity_amount: u64,
    },

    // 13
    /// Make a flash loan.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source liquidity token account.
    ///                     Minted by reserve liquidity mint.
    ///                     Must match the reserve liquidity supply.
    ///   1. `[writable]` Destination liquidity token account.
    ///                     Minted by reserve liquidity mint.
    ///   2. `[writable]` Reserve account.
    ///   3. `[]` Lending market account.
    ///   4. `[]` Derived lending market authority.
    ///   5. `[]` Flash loan receiver program account.
    ///             Must implement an instruction that has tag of 0 and a signature of `(repay_amount: u64)`
    ///             This instruction must return the amount to the source liquidity account.
    ///   6. `[]` Token program id.
    ///   7. `[writable]` Flash loan fee receiver account.
    ///                     Must match the reserve liquidity fee receiver.
    ///   8. `[writable]` Host fee receiver.
    ///   .. `[any]` Additional accounts expected by the receiving program's `ReceiveFlashLoan` instruction.
    ///
    ///   The flash loan receiver program that is to be invoked should contain an instruction with
    ///   tag `0` and accept the total amount (including fee) that needs to be returned back after
    ///   its execution has completed.
    ///
    ///   Flash loan receiver should have an instruction with the following signature:
    ///
    ///   0. `[writable]` Source liquidity (matching the destination from above).
    ///   1. `[writable]` Destination liquidity (matching the source from above).
    ///   2. `[]` Token program id
    ///   .. `[any]` Additional accounts provided to the lending program's `FlashLoan` instruction above.
    ///   ReceiveFlashLoan {
    ///       // Amount that is loaned to the receiver program
    ///       amount: u64
    ///   }
    FlashLoan {
        /// The amount that is to be borrowed - u64::MAX for up to 100% of available liquidity
        amount: u64,
    },

    // 14
    /// Combines DepositReserveLiquidity and DepositObligationCollateral
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Source liquidity token account.
    ///                     $authority can transfer $liquidity_amount.
    ///   1. `[writable]` Destination collateral token account.
    ///   2. `[writable]` Reserve account.
    ///   3. `[writable]` Reserve liquidity supply SPL Token account.
    ///   4. `[writable]` Reserve collateral SPL Token mint.
    ///   5. `[]` Lending market account.
    ///   6. `[]` Derived lending market authority.
    ///   7. `[writable]` Destination deposit reserve collateral supply SPL Token account.
    ///   8. `[writable]` Obligation account.
    ///   9. `[signer]` Obligation owner.
    ///   10 `[signer]` User transfer authority ($authority).
    ///   11 `[]` Clock sysvar.
    ///   12 `[]` Token program id.
    ///   13 `[writable, optional]` Stake account.
    ///   14 `[writable, optional]` Staking pool.
    ///   15 `[optional]` staking program id.
    DepositReserveLiquidityAndObligationCollateral {
        /// Amount of liquidity to deposit in exchange
        liquidity_amount: u64,
    },

    // 16
    /// Update configuration for an existing market reserve.
    ///
    /// Accounts expected by this instruction:
    ///
    ///   0. `[writable]` Reserve account
    ///   1. `[]` Lending market account.
    ///   2. `[]` Derived lending market authority.
    ///   3. `[signer]` Lending market owner.
    ///   4. `[]` Rent sysvar.
    ///   5. `[]` Token program id.
    UpdateReserve {
        /// Reserve configuration values
        config: ReserveConfig,
    },

    ///17
    /// Withdraw fee from a reserve.
    /// Accounts expected by this instruction:
    ///
    ///   0. `[]` Reserve account
    ///   1. `[]` Lending market account.
    ///   2. `[]` Derived lending market authority.
    ///   3. `[signer]` Lending market owner.
    ///   4. `[writable]` Reserve fee account
    ///   5. `[writable]` Destination fee account
    ///   4. `[]` Rent sysvar.
    ///   5. `[]` Token program id.
    WithdrawFee,
}

impl LendingInstruction {
    /// Unpacks a byte buffer into a [LendingInstruction](enum.LendingInstruction.html).
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let (&tag, rest) = input
            .split_first()
            .ok_or(LendingError::InstructionUnpackError)?;
        Ok(match tag {
            0 => {
                let (owner, rest) = Self::unpack_pubkey(rest)?;
                let (quote_currency, _rest) = Self::unpack_bytes32(rest)?;
                Self::InitLendingMarket {
                    owner,
                    quote_currency: *quote_currency,
                }
            }
            1 => {
                let (new_owner, _rest) = Self::unpack_pubkey(rest)?;
                Self::SetLendingMarketOwner { new_owner }
            }
            2 => {
                let (liquidity_amount, rest) = Self::unpack_u64(rest)?;
                let (fixed_price, rest) = Self::unpack_coption_decimal(rest)?;
                let (config, _rest) = Self::unpack_reserve_config(rest)?;
                Self::InitReserve {
                    liquidity_amount,
                    fixed_price,
                    config,
                }
            }
            3 => Self::RefreshReserve,
            4 => {
                let (liquidity_amount, _rest) = Self::unpack_u64(rest)?;
                Self::DepositReserveLiquidity { liquidity_amount }
            }
            5 => {
                let (collateral_amount, _rest) = Self::unpack_u64(rest)?;
                Self::RedeemReserveCollateral { collateral_amount }
            }
            6 => Self::InitObligation,
            7 => Self::RefreshObligation,
            8 => {
                let (collateral_amount, _rest) = Self::unpack_u64(rest)?;
                Self::DepositObligationCollateral { collateral_amount }
            }
            9 => {
                let (collateral_amount, _rest) = Self::unpack_u64(rest)?;
                Self::WithdrawObligationCollateral { collateral_amount }
            }
            10 => {
                let (liquidity_amount, _rest) = Self::unpack_u64(rest)?;
                Self::BorrowObligationLiquidity { liquidity_amount }
            }
            11 => {
                let (liquidity_amount, _rest) = Self::unpack_u64(rest)?;
                Self::RepayObligationLiquidity { liquidity_amount }
            }
            12 => {
                let (liquidity_amount, _rest) = Self::unpack_u64(rest)?;
                Self::LiquidateObligation { liquidity_amount }
            }
            13 => {
                let (amount, _rest) = Self::unpack_u64(rest)?;
                Self::FlashLoan { amount }
            }
            14 => {
                let (liquidity_amount, _rest) = Self::unpack_u64(rest)?;
                Self::DepositReserveLiquidityAndObligationCollateral { liquidity_amount }
            }
            16 => {
                let (config, _rest) = Self::unpack_reserve_config(rest)?;
                Self::UpdateReserve { config }
            }
            17 => Self::WithdrawFee,
            _ => {
                msg!("Instruction cannot be unpacked");
                return Err(LendingError::InstructionUnpackError.into());
            }
        })
    }

    fn unpack_u64(input: &[u8]) -> Result<(u64, &[u8]), ProgramError> {
        if input.len() < 8 {
            msg!("u64 cannot be unpacked");
            return Err(LendingError::InstructionUnpackError.into());
        }
        let (bytes, rest) = input.split_at(8);
        let value = bytes
            .get(..8)
            .and_then(|slice| slice.try_into().ok())
            .map(u64::from_le_bytes)
            .ok_or(LendingError::InstructionUnpackError)?;
        Ok((value, rest))
    }

    fn unpack_u8(input: &[u8]) -> Result<(u8, &[u8]), ProgramError> {
        if input.is_empty() {
            msg!("u8 cannot be unpacked");
            return Err(LendingError::InstructionUnpackError.into());
        }
        let (bytes, rest) = input.split_at(1);
        let value = bytes
            .get(..1)
            .and_then(|slice| slice.try_into().ok())
            .map(u8::from_le_bytes)
            .ok_or(LendingError::InstructionUnpackError)?;
        Ok((value, rest))
    }

    fn unpack_bytes32(input: &[u8]) -> Result<(&[u8; 32], &[u8]), ProgramError> {
        if input.len() < 32 {
            msg!("32 bytes cannot be unpacked");
            return Err(LendingError::InstructionUnpackError.into());
        }
        let (bytes, rest) = input.split_at(32);
        Ok((
            bytes
                .try_into()
                .map_err(|_| LendingError::InstructionUnpackError)?,
            rest,
        ))
    }

    fn unpack_pubkey(input: &[u8]) -> Result<(Pubkey, &[u8]), ProgramError> {
        if input.len() < PUBKEY_BYTES {
            msg!("Pubkey cannot be unpacked");
            return Err(LendingError::InstructionUnpackError.into());
        }
        let (key, rest) = input.split_at(PUBKEY_BYTES);
        let pk = Pubkey::new(key);
        Ok((pk, rest))
    }

    fn unpack_decimal(input: &[u8; 16]) -> Decimal {
        Decimal::from_scaled_val(u128::from_le_bytes(*input))
    }

    fn unpack_coption_decimal(input: &[u8]) -> Result<(COption<Decimal>, &[u8]), ProgramError> {
        if input.len() < 4 + 16 {
            msg!("Coption Decimal cannot be unpacked");
            return Err(LendingError::InstructionUnpackError.into());
        }
        let (coption_decimal, rest) = input.split_at(20);
        let (tag, decimal) = array_refs![coption_decimal.try_into().unwrap(), 4, 16];
        #[allow(clippy::ptr_offset_with_cast)]
        match *tag {
            [0, 0, 0, 0] => Ok((COption::None, rest)),
            [1, 0, 0, 0] => Ok((COption::Some(Self::unpack_decimal(decimal)), rest)),
            _ => {
                msg!("COption<Decimal> cannot be unpacked");
                Err(ProgramError::InvalidAccountData)
            }
        }
    }

    fn unpack_coption_key_compact(input: &[u8]) -> Result<(COption<Pubkey>, &[u8]), ProgramError> {
        if input.len() < 1 + PUBKEY_BYTES {
            msg!("Coption Pubkey cannot be unpacked, buffer length is not enough");
            return Err(LendingError::InstructionUnpackError.into());
        }
        let (coption_pubkey, rest) = input.split_at(33);

        let coption_pubkey = unpack_coption_key_compact(coption_pubkey.try_into().unwrap())?;
        Ok((coption_pubkey, rest))
    }

    fn unpack_reserve_config(input: &[u8]) -> Result<(ReserveConfig, &[u8]), ProgramError> {
        let (optimal_utilization_rate, rest) = Self::unpack_u8(input)?;
        let (loan_to_value_ratio, rest) = Self::unpack_u8(rest)?;
        let (liquidation_bonus, rest) = Self::unpack_u8(rest)?;
        let (liquidation_threshold, rest) = Self::unpack_u8(rest)?;
        let (min_borrow_rate, rest) = Self::unpack_u8(rest)?;
        let (optimal_borrow_rate, rest) = Self::unpack_u8(rest)?;
        let (max_borrow_rate, rest) = Self::unpack_u8(rest)?;
        let (borrow_fee_wad, rest) = Self::unpack_u64(rest)?;
        let (flash_loan_fee_wad, rest) = Self::unpack_u64(rest)?;
        let (host_fee_percentage, rest) = Self::unpack_u8(rest)?;
        let (deposit_staking_pool, rest) = Self::unpack_coption_key_compact(rest)?;
        Ok((
            ReserveConfig {
                optimal_utilization_rate,
                loan_to_value_ratio,
                liquidation_bonus,
                liquidation_threshold,
                min_borrow_rate,
                optimal_borrow_rate,
                max_borrow_rate,
                fees: ReserveFees {
                    borrow_fee_wad,
                    flash_loan_fee_wad,
                    host_fee_percentage,
                },
                deposit_staking_pool,
            },
            rest,
        ))
    }

    /// Packs a [LendingInstruction](enum.LendingInstruction.html) into a byte buffer.
    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        match *self {
            Self::InitLendingMarket {
                owner,
                quote_currency,
            } => {
                buf.push(0);
                buf.extend_from_slice(owner.as_ref());
                buf.extend_from_slice(quote_currency.as_ref());
            }
            Self::SetLendingMarketOwner { new_owner } => {
                buf.push(1);
                buf.extend_from_slice(new_owner.as_ref());
            }
            Self::InitReserve {
                liquidity_amount,
                fixed_price,
                config,
            } => {
                buf.push(2);
                buf.extend_from_slice(&liquidity_amount.to_le_bytes());
                Self::pack_coption_decimal(&mut buf, fixed_price);
                Self::pack_reserve_config(&mut buf, config);
            }
            Self::RefreshReserve => {
                buf.push(3);
            }
            Self::DepositReserveLiquidity { liquidity_amount } => {
                buf.push(4);
                buf.extend_from_slice(&liquidity_amount.to_le_bytes());
            }
            Self::RedeemReserveCollateral { collateral_amount } => {
                buf.push(5);
                buf.extend_from_slice(&collateral_amount.to_le_bytes());
            }
            Self::InitObligation => {
                buf.push(6);
            }
            Self::RefreshObligation => {
                buf.push(7);
            }
            Self::DepositObligationCollateral { collateral_amount } => {
                buf.push(8);
                buf.extend_from_slice(&collateral_amount.to_le_bytes());
            }
            Self::WithdrawObligationCollateral { collateral_amount } => {
                buf.push(9);
                buf.extend_from_slice(&collateral_amount.to_le_bytes());
            }
            Self::BorrowObligationLiquidity { liquidity_amount } => {
                buf.push(10);
                buf.extend_from_slice(&liquidity_amount.to_le_bytes());
            }
            Self::RepayObligationLiquidity { liquidity_amount } => {
                buf.push(11);
                buf.extend_from_slice(&liquidity_amount.to_le_bytes());
            }
            Self::LiquidateObligation { liquidity_amount } => {
                buf.push(12);
                buf.extend_from_slice(&liquidity_amount.to_le_bytes());
            }
            Self::FlashLoan { amount } => {
                buf.push(13);
                buf.extend_from_slice(&amount.to_le_bytes());
            }
            Self::DepositReserveLiquidityAndObligationCollateral { liquidity_amount } => {
                buf.push(14);
                buf.extend_from_slice(&liquidity_amount.to_le_bytes());
            }
            Self::UpdateReserve { config } => {
                buf.push(16);
                Self::pack_reserve_config(&mut buf, config);
            }
            Self::WithdrawFee => {
                buf.push(17);
            }
        }
        buf
    }

    fn pack_coption_decimal(buf: &mut Vec<u8>, coption_decimal: COption<Decimal>) {
        match coption_decimal {
            COption::None => {
                buf.extend_from_slice(&[0u8, 0u8, 0u8, 0u8]);
                buf.extend_from_slice(
                    &Decimal::zero()
                        .to_scaled_val()
                        .expect("Decimal cannot be packed")
                        .to_le_bytes(),
                );
            }
            COption::Some(decimal) => {
                buf.extend_from_slice(&[1u8, 0u8, 0u8, 0u8]);
                buf.extend_from_slice(
                    &decimal
                        .to_scaled_val()
                        .expect("Decimal cannot be packed")
                        .to_le_bytes(),
                );
            }
        }
    }

    fn pack_reserve_config(buf: &mut Vec<u8>, reserve_config: ReserveConfig) {
        let ReserveConfig {
            optimal_utilization_rate,
            loan_to_value_ratio,
            liquidation_bonus,
            liquidation_threshold,
            min_borrow_rate,
            optimal_borrow_rate,
            max_borrow_rate,
            fees:
                ReserveFees {
                    borrow_fee_wad,
                    flash_loan_fee_wad,
                    host_fee_percentage,
                },
            deposit_staking_pool,
        } = reserve_config;
        buf.extend_from_slice(&optimal_utilization_rate.to_le_bytes());
        buf.extend_from_slice(&loan_to_value_ratio.to_le_bytes());
        buf.extend_from_slice(&liquidation_bonus.to_le_bytes());
        buf.extend_from_slice(&liquidation_threshold.to_le_bytes());
        buf.extend_from_slice(&min_borrow_rate.to_le_bytes());
        buf.extend_from_slice(&optimal_borrow_rate.to_le_bytes());
        buf.extend_from_slice(&max_borrow_rate.to_le_bytes());
        buf.extend_from_slice(&borrow_fee_wad.to_le_bytes());
        buf.extend_from_slice(&flash_loan_fee_wad.to_le_bytes());
        buf.extend_from_slice(&host_fee_percentage.to_le_bytes());
        let mut coption_key_buf = [0u8; 33];
        pack_coption_key_compact(&deposit_staking_pool, &mut coption_key_buf);
        buf.extend_from_slice(&coption_key_buf);
    }
}

/// Creates an 'InitLendingMarket' instruction.
pub fn init_lending_market(
    program_id: Pubkey,
    owner: Pubkey,
    quote_currency: [u8; 32],
    lending_market_pubkey: Pubkey,
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(lending_market_pubkey, false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: LendingInstruction::InitLendingMarket {
            owner,
            quote_currency,
        }
        .pack(),
    }
}

/// Creates a 'SetLendingMarketOwner' instruction.
pub fn set_lending_market_owner(
    program_id: Pubkey,
    lending_market_pubkey: Pubkey,
    lending_market_owner: Pubkey,
    new_owner: Pubkey,
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(lending_market_pubkey, false),
            AccountMeta::new_readonly(lending_market_owner, true),
        ],
        data: LendingInstruction::SetLendingMarketOwner { new_owner }.pack(),
    }
}

/// Creates an 'InitReserve' instruction.
#[allow(clippy::too_many_arguments)]
pub fn init_reserve(
    program_id: Pubkey,
    liquidity_amount: u64,
    fixed_price: COption<Decimal>,
    config: ReserveConfig,
    source_liquidity_pubkey: Pubkey,
    destination_collateral_pubkey: Pubkey,
    reserve_pubkey: Pubkey,
    reserve_liquidity_mint_pubkey: Pubkey,
    reserve_liquidity_supply_pubkey: Pubkey,
    reserve_liquidity_fee_receiver_pubkey: Pubkey,
    reserve_collateral_mint_pubkey: Pubkey,
    reserve_collateral_supply_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    lending_market_owner_pubkey: Pubkey,
    user_transfer_authority_pubkey: Pubkey,
    coption_pyth_price_pubkey: COption<Pubkey>,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    let mut accounts = vec![
        AccountMeta::new(source_liquidity_pubkey, false),
        AccountMeta::new(destination_collateral_pubkey, false),
        AccountMeta::new(reserve_pubkey, false),
        AccountMeta::new_readonly(reserve_liquidity_mint_pubkey, false),
        AccountMeta::new(reserve_liquidity_supply_pubkey, false),
        AccountMeta::new(reserve_liquidity_fee_receiver_pubkey, false),
        AccountMeta::new(reserve_collateral_mint_pubkey, false),
        AccountMeta::new(reserve_collateral_supply_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(lending_market_authority_pubkey, false),
        AccountMeta::new_readonly(lending_market_owner_pubkey, true),
        AccountMeta::new_readonly(user_transfer_authority_pubkey, true),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(sysvar::rent::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];
    if let COption::Some(pyth_price_pubkey) = coption_pyth_price_pubkey {
        accounts.push(AccountMeta::new_readonly(pyth_price_pubkey, false))
    }
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::InitReserve {
            liquidity_amount,
            fixed_price,
            config,
        }
        .pack(),
    }
}

/// Creates a `RefreshReserve` instruction
pub fn refresh_reserve(
    program_id: Pubkey,
    reserve_pubkey: Pubkey,
    reserve_liquidity_oracle_pubkey: COption<Pubkey>,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(reserve_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];
    if let COption::Some(reserve_liquidity_oracle_pubkey) = reserve_liquidity_oracle_pubkey {
        accounts.push(AccountMeta::new_readonly(
            reserve_liquidity_oracle_pubkey,
            false,
        ));
    }
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::RefreshReserve.pack(),
    }
}

/// Creates a 'DepositReserveLiquidity' instruction.
#[allow(clippy::too_many_arguments)]
pub fn deposit_reserve_liquidity(
    program_id: Pubkey,
    liquidity_amount: u64,
    source_liquidity_pubkey: Pubkey,
    destination_collateral_pubkey: Pubkey,
    reserve_pubkey: Pubkey,
    reserve_liquidity_supply_pubkey: Pubkey,
    reserve_collateral_mint_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    user_transfer_authority_pubkey: Pubkey,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(source_liquidity_pubkey, false),
            AccountMeta::new(destination_collateral_pubkey, false),
            AccountMeta::new(reserve_pubkey, false),
            AccountMeta::new(reserve_liquidity_supply_pubkey, false),
            AccountMeta::new(reserve_collateral_mint_pubkey, false),
            AccountMeta::new_readonly(lending_market_pubkey, false),
            AccountMeta::new_readonly(lending_market_authority_pubkey, false),
            AccountMeta::new_readonly(user_transfer_authority_pubkey, true),
            AccountMeta::new_readonly(sysvar::clock::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: LendingInstruction::DepositReserveLiquidity { liquidity_amount }.pack(),
    }
}

/// Creates a 'RedeemReserveCollateral' instruction.
#[allow(clippy::too_many_arguments)]
pub fn redeem_reserve_collateral(
    program_id: Pubkey,
    collateral_amount: u64,
    source_collateral_pubkey: Pubkey,
    destination_liquidity_pubkey: Pubkey,
    reserve_pubkey: Pubkey,
    reserve_collateral_mint_pubkey: Pubkey,
    reserve_liquidity_supply_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    user_transfer_authority_pubkey: Pubkey,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(source_collateral_pubkey, false),
            AccountMeta::new(destination_liquidity_pubkey, false),
            AccountMeta::new(reserve_pubkey, false),
            AccountMeta::new(reserve_collateral_mint_pubkey, false),
            AccountMeta::new(reserve_liquidity_supply_pubkey, false),
            AccountMeta::new_readonly(lending_market_pubkey, false),
            AccountMeta::new_readonly(lending_market_authority_pubkey, false),
            AccountMeta::new_readonly(user_transfer_authority_pubkey, true),
            AccountMeta::new_readonly(sysvar::clock::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: LendingInstruction::RedeemReserveCollateral { collateral_amount }.pack(),
    }
}

/// Creates an `InitObligation` instruction.
#[allow(clippy::too_many_arguments)]
pub fn init_obligation(
    program_id: Pubkey,
    obligation_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    obligation_owner_pubkey: Pubkey,
) -> Instruction {
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(obligation_pubkey, false),
            AccountMeta::new_readonly(lending_market_pubkey, false),
            AccountMeta::new_readonly(obligation_owner_pubkey, true),
            AccountMeta::new_readonly(sysvar::clock::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: LendingInstruction::InitObligation.pack(),
    }
}

/// Creates a `RefreshObligation` instruction.
#[allow(clippy::too_many_arguments)]
pub fn refresh_obligation(
    program_id: Pubkey,
    obligation_pubkey: Pubkey,
    reserve_pubkeys: Vec<Pubkey>,
) -> Instruction {
    let mut accounts = vec![
        AccountMeta::new(obligation_pubkey, false),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
    ];
    accounts.extend(
        reserve_pubkeys
            .into_iter()
            .map(|pubkey| AccountMeta::new_readonly(pubkey, false)),
    );
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::RefreshObligation.pack(),
    }
}

/// Creates a `DepositObligationCollateral` instruction.
#[allow(clippy::too_many_arguments)]
pub fn deposit_obligation_collateral(
    program_id: Pubkey,
    collateral_amount: u64,
    source_collateral_pubkey: Pubkey,
    destination_collateral_pubkey: Pubkey,
    deposit_reserve_pubkey: Pubkey,
    obligation_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    obligation_owner_pubkey: Pubkey,
    user_transfer_authority_pubkey: Pubkey,
    option_stake_account_pubkey: Option<Pubkey>,
    option_staking_pool_pubkey: Option<Pubkey>,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    let mut accounts = vec![
        AccountMeta::new(source_collateral_pubkey, false),
        AccountMeta::new(destination_collateral_pubkey, false),
        AccountMeta::new_readonly(deposit_reserve_pubkey, false),
        AccountMeta::new(obligation_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(lending_market_authority_pubkey, false),
        AccountMeta::new_readonly(obligation_owner_pubkey, true),
        AccountMeta::new_readonly(user_transfer_authority_pubkey, true),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];

    if let [Some(staking_account_pubkey), Some(staking_pool_pubkey)] =
        [option_stake_account_pubkey, option_staking_pool_pubkey]
    {
        accounts.push(AccountMeta::new(staking_account_pubkey, false));
        accounts.push(AccountMeta::new(staking_pool_pubkey, false));
        accounts.push(AccountMeta::new_readonly(port_finance_staking::id(), false));
    }
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::DepositObligationCollateral { collateral_amount }.pack(),
    }
}

/// Creates a `WithdrawObligationCollateral` instruction.
#[allow(clippy::too_many_arguments)]
pub fn withdraw_obligation_collateral(
    program_id: Pubkey,
    collateral_amount: u64,
    source_collateral_pubkey: Pubkey,
    destination_collateral_pubkey: Pubkey,
    withdraw_reserve_pubkey: Pubkey,
    obligation_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    obligation_owner_pubkey: Pubkey,
    option_stake_account_pubkey: Option<Pubkey>,
    option_staking_pool_pubkey: Option<Pubkey>,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    let mut accounts = vec![
        AccountMeta::new(source_collateral_pubkey, false),
        AccountMeta::new(destination_collateral_pubkey, false),
        AccountMeta::new_readonly(withdraw_reserve_pubkey, false),
        AccountMeta::new(obligation_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(lending_market_authority_pubkey, false),
        AccountMeta::new_readonly(obligation_owner_pubkey, true),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];

    if let [Some(staking_account_pubkey), Some(staking_pool_pubkey)] =
        [option_stake_account_pubkey, option_staking_pool_pubkey]
    {
        accounts.push(AccountMeta::new(staking_account_pubkey, false));
        accounts.push(AccountMeta::new(staking_pool_pubkey, false));
        accounts.push(AccountMeta::new_readonly(port_finance_staking::id(), false));
    }
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::WithdrawObligationCollateral { collateral_amount }.pack(),
    }
}

/// Creates a `BorrowObligationLiquidity` instruction.
#[allow(clippy::too_many_arguments)]
pub fn borrow_obligation_liquidity(
    program_id: Pubkey,
    liquidity_amount: u64,
    source_liquidity_pubkey: Pubkey,
    destination_liquidity_pubkey: Pubkey,
    borrow_reserve_pubkey: Pubkey,
    borrow_reserve_liquidity_fee_receiver_pubkey: Pubkey,
    obligation_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    obligation_owner_pubkey: Pubkey,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    let accounts = vec![
        AccountMeta::new(source_liquidity_pubkey, false),
        AccountMeta::new(destination_liquidity_pubkey, false),
        AccountMeta::new(borrow_reserve_pubkey, false),
        AccountMeta::new(borrow_reserve_liquidity_fee_receiver_pubkey, false),
        AccountMeta::new(obligation_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(lending_market_authority_pubkey, false),
        AccountMeta::new_readonly(obligation_owner_pubkey, true),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];

    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::BorrowObligationLiquidity { liquidity_amount }.pack(),
    }
}

/// Creates a `RepayObligationLiquidity` instruction
#[allow(clippy::too_many_arguments)]
pub fn repay_obligation_liquidity(
    program_id: Pubkey,
    liquidity_amount: u64,
    source_liquidity_pubkey: Pubkey,
    destination_liquidity_pubkey: Pubkey,
    repay_reserve_pubkey: Pubkey,
    obligation_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    user_transfer_authority_pubkey: Pubkey,
) -> Instruction {
    let accounts = vec![
        AccountMeta::new(source_liquidity_pubkey, false),
        AccountMeta::new(destination_liquidity_pubkey, false),
        AccountMeta::new(repay_reserve_pubkey, false),
        AccountMeta::new(obligation_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(user_transfer_authority_pubkey, true),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::RepayObligationLiquidity { liquidity_amount }.pack(),
    }
}

/// Creates a `LiquidateObligation` instruction
#[allow(clippy::too_many_arguments)]
pub fn liquidate_obligation(
    program_id: Pubkey,
    liquidity_amount: u64,
    source_liquidity_pubkey: Pubkey,
    destination_collateral_pubkey: Pubkey,
    repay_reserve_pubkey: Pubkey,
    repay_reserve_liquidity_supply_pubkey: Pubkey,
    withdraw_reserve_pubkey: Pubkey,
    withdraw_reserve_collateral_supply_pubkey: Pubkey,
    obligation_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    user_transfer_authority_pubkey: Pubkey,
    option_borrow_stake_account_pubkey: Option<Pubkey>,
    option_borrow_staking_pool_pubkey: Option<Pubkey>,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    let mut accounts = vec![
        AccountMeta::new(source_liquidity_pubkey, false),
        AccountMeta::new(destination_collateral_pubkey, false),
        AccountMeta::new(repay_reserve_pubkey, false),
        AccountMeta::new(repay_reserve_liquidity_supply_pubkey, false),
        AccountMeta::new_readonly(withdraw_reserve_pubkey, false),
        AccountMeta::new(withdraw_reserve_collateral_supply_pubkey, false),
        AccountMeta::new(obligation_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(lending_market_authority_pubkey, false),
        AccountMeta::new_readonly(user_transfer_authority_pubkey, true),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];
    if let [Some(staking_account_pubkey), Some(staking_pool_pubkey)] = [
        option_borrow_stake_account_pubkey,
        option_borrow_staking_pool_pubkey,
    ] {
        accounts.push(AccountMeta::new(staking_account_pubkey, false));
        accounts.push(AccountMeta::new(staking_pool_pubkey, false));
        accounts.push(AccountMeta::new_readonly(port_finance_staking::id(), false));
    }
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::LiquidateObligation { liquidity_amount }.pack(),
    }
}

/// Creates a `FlashLoan` instruction.
#[allow(clippy::too_many_arguments)]
pub fn flash_loan(
    program_id: Pubkey,
    amount: u64,
    source_liquidity_pubkey: Pubkey,
    destination_liquidity_pubkey: Pubkey,
    reserve_pubkey: Pubkey,
    reserve_liquidity_fee_receiver_pubkey: Pubkey,
    host_fee_receiver_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    flash_loan_receiver_program_id: Pubkey,
    flash_loan_receiver_program_accounts: Vec<AccountMeta>,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    let mut accounts = vec![
        AccountMeta::new(source_liquidity_pubkey, false),
        AccountMeta::new(destination_liquidity_pubkey, false),
        AccountMeta::new(reserve_pubkey, false),
        AccountMeta::new(reserve_liquidity_fee_receiver_pubkey, false),
        AccountMeta::new(host_fee_receiver_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(lending_market_authority_pubkey, false),
        AccountMeta::new_readonly(spl_token::id(), false),
        AccountMeta::new_readonly(flash_loan_receiver_program_id, false),
    ];
    accounts.extend(flash_loan_receiver_program_accounts);
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::FlashLoan { amount }.pack(),
    }
}

/// Creates a `DepositReserveLiquidityAndObligationCollateral` instruction.
#[allow(clippy::too_many_arguments)]
pub fn deposit_reserve_liquidity_and_obligation_collateral(
    program_id: Pubkey,
    liquidity_amount: u64,
    source_liquidity_pubkey: Pubkey,
    user_collateral_pubkey: Pubkey,
    reserve_pubkey: Pubkey,
    reserve_liquidity_supply_pubkey: Pubkey,
    reserve_collateral_mint_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    destination_deposit_collateral_pubkey: Pubkey,
    obligation_pubkey: Pubkey,
    obligation_owner_pubkey: Pubkey,
    user_transfer_authority_pubkey: Pubkey,
    option_stake_account_pubkey: Option<Pubkey>,
    option_staking_pool_pubkey: Option<Pubkey>,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    let mut accounts = vec![
        AccountMeta::new(source_liquidity_pubkey, false),
        AccountMeta::new(user_collateral_pubkey, false),
        AccountMeta::new(reserve_pubkey, false),
        AccountMeta::new(reserve_liquidity_supply_pubkey, false),
        AccountMeta::new(reserve_collateral_mint_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(lending_market_authority_pubkey, false),
        AccountMeta::new(destination_deposit_collateral_pubkey, false),
        AccountMeta::new(obligation_pubkey, false),
        AccountMeta::new(obligation_owner_pubkey, true),
        AccountMeta::new_readonly(user_transfer_authority_pubkey, true),
        AccountMeta::new_readonly(sysvar::clock::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];
    if let [Some(staking_account_pubkey), Some(staking_pool_pubkey)] =
        [option_stake_account_pubkey, option_staking_pool_pubkey]
    {
        accounts.push(AccountMeta::new(staking_account_pubkey, false));
        accounts.push(AccountMeta::new(staking_pool_pubkey, false));
        accounts.push(AccountMeta::new_readonly(port_finance_staking::id(), false));
    }
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::DepositReserveLiquidityAndObligationCollateral {
            liquidity_amount,
        }
        .pack(),
    }
}

/// Creates an `UpdateReserveConfig` instruction.
#[allow(clippy::too_many_arguments)]
pub fn update_reserve(
    program_id: Pubkey,
    config: ReserveConfig,
    reserve_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    lending_market_owner_pubkey: Pubkey,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    let accounts = vec![
        AccountMeta::new(reserve_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(lending_market_authority_pubkey, false),
        AccountMeta::new_readonly(lending_market_owner_pubkey, true),
        AccountMeta::new_readonly(sysvar::rent::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::UpdateReserve { config }.pack(),
    }
}

/// Creates an `WithdrawFee` instruction.
#[allow(clippy::too_many_arguments)]
pub fn withdraw_fee(
    program_id: Pubkey,
    reserve_pubkey: Pubkey,
    lending_market_pubkey: Pubkey,
    lending_market_owner_pubkey: Pubkey,
    reserve_fee_token_pubkey: Pubkey,
    destination_fee_token_pubkey: Pubkey,
) -> Instruction {
    let (lending_market_authority_pubkey, _bump_seed) = Pubkey::find_program_address(
        &[&lending_market_pubkey.to_bytes()[..PUBKEY_BYTES]],
        &program_id,
    );
    let accounts = vec![
        AccountMeta::new(reserve_pubkey, false),
        AccountMeta::new_readonly(lending_market_pubkey, false),
        AccountMeta::new_readonly(lending_market_authority_pubkey, false),
        AccountMeta::new_readonly(lending_market_owner_pubkey, true),
        AccountMeta::new(reserve_fee_token_pubkey, false),
        AccountMeta::new(destination_fee_token_pubkey, false),
        AccountMeta::new_readonly(sysvar::rent::id(), false),
        AccountMeta::new_readonly(spl_token::id(), false),
    ];
    Instruction {
        program_id,
        accounts,
        data: LendingInstruction::WithdrawFee.pack(),
    }
}
