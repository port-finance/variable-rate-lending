//! Program state processor

use std::convert::TryInto;

use num_traits::FromPrimitive;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    decode_error::DecodeError,
    entrypoint::ProgramResult,
    instruction::Instruction,
    msg,
    program::{invoke, invoke_signed},
    program_error::{PrintProgramError, ProgramError},
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    sysvar::{clock::Clock, rent::Rent, Sysvar},
};
use spl_token::solana_program::instruction::AccountMeta;
use spl_token::solana_program::program_option::COption;
use spl_token::state::{Account, Mint};
use switchboard_program::{
    get_aggregator, get_aggregator_result, FastRoundResultAccountData, SwitchboardAccountType,
};

use port_finance_staking::instruction::{deposit, withdraw};
use port_finance_staking::state::stake_account::StakeAccount;

use crate::{
    error::LendingError,
    instruction::LendingInstruction,
    math::{Decimal, Rate, TryAdd, TryDiv, TryMul, WAD},
    pyth,
    state::{
        CalculateBorrowResult, CalculateLiquidationResult, CalculateRepayResult,
        InitLendingMarketParams, InitObligationParams, InitReserveParams, LendingMarket,
        NewReserveCollateralParams, NewReserveLiquidityParams, Obligation, Reserve,
        ReserveCollateral, ReserveConfig, ReserveLiquidity,
    },
};
use switchboard_v2::AggregatorAccountData;

const PYTH: &str = "FsJ3A3u2vn5cTVofAjvy6y5kwABJAqYWpe4975bi2epH";
const PYTH_DEV: &str = "gSbePebfvPy7tRqimPoVecS2UsBvYv46ynrzWocc92s";

const SWITCHBOARD: &str = "DtmE9D2CSB4L5D6A15mraeEjrGMm6auWVzgaD8hK2tZM";
const SWITCHBOARD_DEV: &str = "7azgmy1pFXHikv36q1zZASvFq5vFa39TT9NweVugKKTU";

const SWITCHBOARD_V2: &str = "SW1TCH7qEPTdLsDHRgPuMQjbQxKdH2aBStViMFnt64f";
const SWITCHBOARD_V2_DEV: &str = "2TfB33aLaneQb5TNVwyDz3jSZXS6jdW2ARw1Dgf84XCG";

fn is_pyth_program(pubkey: &Pubkey) -> bool {
    let pubkey_str = pubkey.to_string();
    pubkey_str == PYTH || pubkey_str == PYTH_DEV
}

fn is_switchbaord_program(pubkey: &Pubkey) -> bool {
    is_switchbaord_program_v1(pubkey) || is_switchbaord_program_v2(pubkey)
}

fn is_switchbaord_program_v1(pubkey: &Pubkey) -> bool {
    let pubkey_str = pubkey.to_string();
    pubkey_str == SWITCHBOARD || pubkey_str == SWITCHBOARD_DEV
}

fn is_switchbaord_program_v2(pubkey: &Pubkey) -> bool {
    let pubkey_str = pubkey.to_string();
    pubkey_str == SWITCHBOARD_V2 || pubkey_str == SWITCHBOARD_V2_DEV
}

/// Processes an instruction
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    input: &[u8],
) -> ProgramResult {
    let instruction = LendingInstruction::unpack(input)?;
    match instruction {
        LendingInstruction::InitLendingMarket {
            owner,
            quote_currency,
        } => {
            msg!("Instruction: Init Lending Market");
            process_init_lending_market(program_id, owner, quote_currency, accounts)
        }
        LendingInstruction::InitReserve {
            liquidity_amount,
            fixed_price,
            config,
        } => {
            msg!("Instruction: Init Reserve");
            process_init_reserve(program_id, liquidity_amount, fixed_price, config, accounts)
        }
        LendingInstruction::InitObligation => {
            msg!("Instruction: Init Obligation");
            process_init_obligation(program_id, accounts)
        }
        LendingInstruction::DepositReserveLiquidity { liquidity_amount } => {
            msg!("Instruction: Deposit Reserve Liquidity");
            process_deposit_reserve_liquidity(program_id, liquidity_amount, accounts)
        }
        LendingInstruction::RedeemReserveCollateral { collateral_amount } => {
            msg!("Instruction: Redeem Reserve Collateral");
            process_redeem_reserve_collateral(program_id, collateral_amount, accounts)
        }
        LendingInstruction::BorrowObligationLiquidity { liquidity_amount } => {
            msg!("Instruction: Borrow Obligation Liquidity");
            process_borrow_obligation_liquidity(program_id, liquidity_amount, accounts)
        }
        LendingInstruction::RepayObligationLiquidity { liquidity_amount } => {
            msg!("Instruction: Repay Obligation Liquidity");
            process_repay_obligation_liquidity(program_id, liquidity_amount, accounts)
        }
        LendingInstruction::LiquidateObligation { liquidity_amount } => {
            msg!("Instruction: Liquidate Obligation");
            process_liquidate_obligation(program_id, liquidity_amount, accounts)
        }
        LendingInstruction::RefreshReserve => {
            msg!("Instruction: Refresh Reserve");
            process_refresh_reserve(program_id, accounts)
        }
        LendingInstruction::DepositObligationCollateral { collateral_amount } => {
            msg!("Instruction: Deposit Obligation Collateral");
            process_deposit_obligation_collateral(program_id, collateral_amount, accounts)
        }
        LendingInstruction::WithdrawObligationCollateral { collateral_amount } => {
            msg!("Instruction: Withdraw Obligation Collateral");
            process_withdraw_obligation_collateral(program_id, collateral_amount, accounts)
        }
        LendingInstruction::SetLendingMarketOwner { new_owner } => {
            msg!("Instruction: Set Lending Market Owner");
            process_set_lending_market_owner(program_id, new_owner, accounts)
        }
        LendingInstruction::RefreshObligation => {
            msg!("Instruction: Refresh Obligation");
            process_refresh_obligation(program_id, accounts)
        }
        LendingInstruction::FlashLoan { amount } => {
            msg!("Instruction: Flash Loan");
            process_flash_loan(program_id, amount, accounts)
        }
        LendingInstruction::DepositReserveLiquidityAndObligationCollateral { liquidity_amount } => {
            msg!("Instruction: Deposit Reserve Liquidity and Obligation Collateral");
            process_deposit_reserve_liquidity_and_obligation_collateral(
                program_id,
                liquidity_amount,
                accounts,
            )
        }
        LendingInstruction::UpdateReserve { config } => {
            msg!("Instruction: Update Reserve");
            process_update_reserve(program_id, config, accounts)
        }
        LendingInstruction::WithdrawFee => {
            msg!("Withdraw fee from reserve");
            process_withdraw_fee(program_id, accounts)
        }
    }
}

fn process_init_lending_market(
    program_id: &Pubkey,
    owner: Pubkey,
    quote_currency: [u8; 32],
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let lending_market_info = next_account_info(account_info_iter)?;
    let rent = &Rent::from_account_info(next_account_info(account_info_iter)?)?;
    let token_program_id = next_account_info(account_info_iter)?;

    assert_rent_exempt(rent, lending_market_info)?;
    let mut lending_market = assert_uninitialized::<LendingMarket>(lending_market_info)?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }

    lending_market.init(InitLendingMarketParams {
        bump_seed: Pubkey::find_program_address(&[lending_market_info.key.as_ref()], program_id).1,
        owner,
        quote_currency,
        token_program_id: *token_program_id.key,
    });
    LendingMarket::pack(lending_market, &mut lending_market_info.data.borrow_mut())?;

    Ok(())
}

#[inline(never)] // avoid stack frame limit
fn process_set_lending_market_owner(
    program_id: &Pubkey,
    new_owner: Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_owner_info = next_account_info(account_info_iter)?;

    let mut lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.owner != lending_market_owner_info.key {
        msg!("Lending market owner does not match the lending market owner provided");
        return Err(LendingError::InvalidMarketOwner.into());
    }
    if !lending_market_owner_info.is_signer {
        msg!("Lending market owner provided must be a signer");
        return Err(LendingError::InvalidSigner.into());
    }

    lending_market.owner = new_owner;
    LendingMarket::pack(lending_market, &mut lending_market_info.data.borrow_mut())?;

    Ok(())
}

fn process_init_reserve(
    program_id: &Pubkey,
    liquidity_amount: u64,
    fixed_price: COption<Decimal>,
    config: ReserveConfig,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if liquidity_amount == 0 {
        msg!("Reserve must be initialized with liquidity");
        return Err(LendingError::InvalidAmount.into());
    }
    validate_reserve_config(config)?;

    let account_info_iter = &mut accounts.iter().peekable();
    let source_liquidity_info = next_account_info(account_info_iter)?;
    let destination_collateral_info = next_account_info(account_info_iter)?;
    let reserve_info = next_account_info(account_info_iter)?;
    let reserve_liquidity_mint_info = next_account_info(account_info_iter)?;
    let reserve_liquidity_supply_info = next_account_info(account_info_iter)?;
    let reserve_liquidity_fee_receiver_info = next_account_info(account_info_iter)?;
    let reserve_collateral_mint_info = next_account_info(account_info_iter)?;
    let reserve_collateral_supply_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let lending_market_owner_info = next_account_info(account_info_iter)?;
    let user_transfer_authority_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(next_account_info(account_info_iter)?)?;
    let rent_info = next_account_info(account_info_iter)?;
    let rent = &Rent::from_account_info(rent_info)?;
    let token_program_id = next_account_info(account_info_iter)?;

    assert_rent_exempt(rent, reserve_info)?;
    let mut reserve = assert_uninitialized::<Reserve>(reserve_info)?;
    if reserve_info.owner != program_id {
        msg!("Reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }

    if reserve_liquidity_supply_info.key == source_liquidity_info.key {
        msg!("Reserve liquidity supply cannot be used as the source liquidity provided");
        return Err(LendingError::InvalidAccountInput.into());
    }

    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }
    if &lending_market.owner != lending_market_owner_info.key {
        msg!("Lending market owner does not match the lending market owner provided");
        return Err(LendingError::InvalidMarketOwner.into());
    }
    if !lending_market_owner_info.is_signer {
        msg!("Lending market owner provided must be a signer");
        return Err(LendingError::InvalidSigner.into());
    }

    let (reserve_liquidity_oracle_pubkey, reserve_liquidity_market_price) =
        if let COption::Some(fixed_price_unwrap) = fixed_price {
            if account_info_iter.peek().is_some() {
                msg!(
                    "Reserve liquidity oracle should not be provided if there is fixed price given"
                );
                return Err(LendingError::InvalidAccountInput.into());
            }
            (COption::None, fixed_price_unwrap)
        } else {
            if account_info_iter.peek().is_none() {
                msg!(
                    "Reserve liquidity oracle should be provided if there is no fixed price given"
                );
                return Err(LendingError::InvalidAccountInput.into());
            }

            let oracle_price_info = next_account_info(account_info_iter)?;

            if is_pyth_program(oracle_price_info.owner) {
                (
                    COption::Some(*oracle_price_info.key),
                    get_pyth_price(oracle_price_info, clock)?,
                )
            } else if is_switchbaord_program(oracle_price_info.owner) {
                (
                    COption::Some(*oracle_price_info.key),
                    get_switchboard_price(oracle_price_info, clock)?,
                )
            } else {
                msg!("Oracle owner is {}, not correct", oracle_price_info.owner);
                return Err(LendingError::InvalidOracleConfig.into());
            }
        };

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    let reserve_liquidity_mint = unpack_mint(&reserve_liquidity_mint_info.data.borrow())?;
    if reserve_liquidity_mint_info.owner != token_program_id.key {
        msg!("Reserve liquidity mint is not owned by the token program provided");
        return Err(LendingError::InvalidTokenOwner.into());
    }

    reserve.init(InitReserveParams {
        current_slot: clock.slot,
        lending_market: *lending_market_info.key,
        liquidity: ReserveLiquidity::new(NewReserveLiquidityParams {
            mint_pubkey: *reserve_liquidity_mint_info.key,
            mint_decimals: reserve_liquidity_mint.decimals,
            supply_pubkey: *reserve_liquidity_supply_info.key,
            fee_receiver: *reserve_liquidity_fee_receiver_info.key,
            oracle_pubkey: reserve_liquidity_oracle_pubkey,
            market_price: reserve_liquidity_market_price,
        }),
        collateral: ReserveCollateral::new(NewReserveCollateralParams {
            mint_pubkey: *reserve_collateral_mint_info.key,
            supply_pubkey: *reserve_collateral_supply_info.key,
        }),
        config,
    });

    let collateral_amount = reserve.deposit_liquidity(liquidity_amount)?;
    Reserve::pack(reserve, &mut reserve_info.data.borrow_mut())?;

    spl_token_init_account(TokenInitializeAccountParams {
        account: reserve_liquidity_supply_info.clone(),
        mint: reserve_liquidity_mint_info.clone(),
        owner: lending_market_authority_info.clone(),
        rent: rent_info.clone(),
        token_program: token_program_id.clone(),
    })?;

    spl_token_init_account(TokenInitializeAccountParams {
        account: reserve_liquidity_fee_receiver_info.clone(),
        mint: reserve_liquidity_mint_info.clone(),
        owner: lending_market_authority_info.clone(),
        rent: rent_info.clone(),
        token_program: token_program_id.clone(),
    })?;

    spl_token_init_mint(TokenInitializeMintParams {
        mint: reserve_collateral_mint_info.clone(),
        authority: lending_market_authority_info.key,
        rent: rent_info.clone(),
        decimals: reserve_liquidity_mint.decimals,
        token_program: token_program_id.clone(),
    })?;

    spl_token_init_account(TokenInitializeAccountParams {
        account: reserve_collateral_supply_info.clone(),
        mint: reserve_collateral_mint_info.clone(),
        owner: lending_market_authority_info.clone(),
        rent: rent_info.clone(),
        token_program: token_program_id.clone(),
    })?;

    spl_token_init_account(TokenInitializeAccountParams {
        account: destination_collateral_info.clone(),
        mint: reserve_collateral_mint_info.clone(),
        owner: user_transfer_authority_info.clone(),
        rent: rent_info.clone(),
        token_program: token_program_id.clone(),
    })?;

    spl_token_transfer(TokenTransferParams {
        source: source_liquidity_info.clone(),
        destination: reserve_liquidity_supply_info.clone(),
        amount: liquidity_amount,
        authority: user_transfer_authority_info.clone(),
        authority_signer_seeds: &[],
        token_program: token_program_id.clone(),
    })?;

    spl_token_mint_to(TokenMintToParams {
        mint: reserve_collateral_mint_info.clone(),
        destination: destination_collateral_info.clone(),
        amount: collateral_amount,
        authority: lending_market_authority_info.clone(),
        authority_signer_seeds,
        token_program: token_program_id.clone(),
    })?;

    Ok(())
}

fn validate_reserve_config(config: ReserveConfig) -> ProgramResult {
    if config.optimal_utilization_rate > 100 {
        msg!("Optimal utilization rate must be in range [0, 100]");
        return Err(LendingError::InvalidConfig.into());
    }
    if config.loan_to_value_ratio >= 100 {
        msg!("Loan to value ratio must be in range [0, 100)");
        return Err(LendingError::InvalidConfig.into());
    }
    if config.liquidation_bonus > 100 {
        msg!("Liquidation bonus must be in range [0, 100]");
        return Err(LendingError::InvalidConfig.into());
    }
    if config.liquidation_threshold <= config.loan_to_value_ratio
        || config.liquidation_threshold > 100
    {
        msg!(
            "Liquidation threshold must be in range (LTV {}, 100]",
            config.loan_to_value_ratio
        );
        return Err(LendingError::InvalidConfig.into());
    }
    if config.optimal_borrow_rate < config.min_borrow_rate {
        msg!("Optimal borrow rate must be >= min borrow rate");
        return Err(LendingError::InvalidConfig.into());
    }
    if config.optimal_borrow_rate > config.max_borrow_rate {
        msg!("Optimal borrow rate must be <= max borrow rate");
        return Err(LendingError::InvalidConfig.into());
    }
    if config.fees.borrow_fee_wad >= WAD {
        msg!("Borrow fee must be in range [0, 1_000_000_000_000_000_000)");
        return Err(LendingError::InvalidConfig.into());
    }
    if config.fees.host_fee_percentage > 100 {
        msg!("Host fee percentage must be in range [0, 100]");
        return Err(LendingError::InvalidConfig.into());
    }

    Ok(())
}

fn process_refresh_reserve(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter().peekable();
    let reserve_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(next_account_info(account_info_iter)?)?;

    let mut reserve = Reserve::unpack(&reserve_info.data.borrow())?;
    if reserve_info.owner != program_id {
        msg!("Reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if let COption::Some(reserve_liquidity_oracle_pubkey) = reserve.liquidity.oracle_pubkey {
        let reserve_liquidity_oracle_info = next_account_info(account_info_iter)?;
        if &reserve_liquidity_oracle_pubkey != reserve_liquidity_oracle_info.key {
            msg!("Reserve liquidity oracle does not match the reserve liquidity oracle provided");
            return Err(LendingError::InvalidAccountInput.into());
        }

        // @TODO: sanity check https://git.io/JOCcb
        reserve.liquidity.market_price = if is_pyth_program(reserve_liquidity_oracle_info.owner) {
            get_pyth_price(reserve_liquidity_oracle_info, clock)
        } else if is_switchbaord_program(reserve_liquidity_oracle_info.owner) {
            get_switchboard_price(reserve_liquidity_oracle_info, clock)
        } else {
            Err(LendingError::InvalidAccountInput.into())
        }?;
    } else if account_info_iter.peek().is_some() {
        msg!("Reserve liquidity oracle cannot be provided when reserve liquidity is the quote currency");
        return Err(LendingError::InvalidAccountInput.into());
    }

    reserve.accrue_interest(clock.slot)?;
    reserve.last_update.update_slot(clock.slot);
    Reserve::pack(reserve, &mut reserve_info.data.borrow_mut())?;

    Ok(())
}

fn process_deposit_reserve_liquidity(
    program_id: &Pubkey,
    liquidity_amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if liquidity_amount == 0 {
        msg!("Liquidity amount provided cannot be zero");
        return Err(LendingError::InvalidAmount.into());
    }

    let account_info_iter = &mut accounts.iter();
    let source_liquidity_info = next_account_info(account_info_iter)?;
    let destination_collateral_info = next_account_info(account_info_iter)?;
    let reserve_info = next_account_info(account_info_iter)?;
    let reserve_liquidity_supply_info = next_account_info(account_info_iter)?;
    let reserve_collateral_mint_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let user_transfer_authority_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(next_account_info(account_info_iter)?)?;
    let token_program_id = next_account_info(account_info_iter)?;

    // We don't care about the return value here, so just ignore it.
    _deposit_reserve_liquidity(
        program_id,
        liquidity_amount,
        source_liquidity_info,
        destination_collateral_info,
        reserve_info,
        reserve_liquidity_supply_info,
        reserve_collateral_mint_info,
        lending_market_info,
        lending_market_authority_info,
        user_transfer_authority_info,
        clock,
        token_program_id,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn _deposit_reserve_liquidity<'a>(
    program_id: &Pubkey,
    liquidity_amount: u64,
    source_liquidity_info: &AccountInfo<'a>,
    destination_collateral_info: &AccountInfo<'a>,
    reserve_info: &AccountInfo<'a>,
    reserve_liquidity_supply_info: &AccountInfo<'a>,
    reserve_collateral_mint_info: &AccountInfo<'a>,
    lending_market_info: &AccountInfo<'a>,
    lending_market_authority_info: &AccountInfo<'a>,
    user_transfer_authority_info: &AccountInfo<'a>,
    clock: &Clock,
    token_program_id: &AccountInfo<'a>,
) -> Result<u64, ProgramError> {
    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }

    let mut reserve = Reserve::unpack(&reserve_info.data.borrow())?;
    if reserve_info.owner != program_id {
        msg!("Reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &reserve.lending_market != lending_market_info.key {
        msg!("Reserve lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.liquidity.supply_pubkey != reserve_liquidity_supply_info.key {
        msg!("Reserve liquidity supply does not match the reserve liquidity supply provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.collateral.mint_pubkey != reserve_collateral_mint_info.key {
        msg!("Reserve collateral mint does not match the reserve collateral mint provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.liquidity.supply_pubkey == source_liquidity_info.key {
        msg!("Reserve liquidity supply cannot be used as the source liquidity provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.collateral.supply_pubkey == destination_collateral_info.key {
        msg!("Reserve collateral supply cannot be used as the destination collateral provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if reserve.last_update.is_stale(clock.slot)? {
        msg!("Reserve is stale and must be refreshed in the current slot");
        return Err(LendingError::ReserveStale.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    if Decimal::from(liquidity_amount)
        .try_add(reserve.liquidity.total_supply()?)?
        .try_floor_u64()?
        > reserve.config.deposit_limit
    {
        msg!("Cannot deposit liquidity above the reserve deposit limit");
        return Err(LendingError::InvalidAmount.into());
    }

    let collateral_amount = reserve.deposit_liquidity(liquidity_amount)?;
    reserve.last_update.mark_stale();
    Reserve::pack(reserve, &mut reserve_info.data.borrow_mut())?;

    spl_token_transfer(TokenTransferParams {
        source: source_liquidity_info.clone(),
        destination: reserve_liquidity_supply_info.clone(),
        amount: liquidity_amount,
        authority: user_transfer_authority_info.clone(),
        authority_signer_seeds: &[],
        token_program: token_program_id.clone(),
    })?;

    spl_token_mint_to(TokenMintToParams {
        mint: reserve_collateral_mint_info.clone(),
        destination: destination_collateral_info.clone(),
        amount: collateral_amount,
        authority: lending_market_authority_info.clone(),
        authority_signer_seeds,
        token_program: token_program_id.clone(),
    })?;

    Ok(collateral_amount)
}

fn process_redeem_reserve_collateral(
    program_id: &Pubkey,
    collateral_amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if collateral_amount == 0 {
        msg!("Collateral amount provided cannot be zero");
        return Err(LendingError::InvalidAmount.into());
    }

    let account_info_iter = &mut accounts.iter();
    let source_collateral_info = next_account_info(account_info_iter)?;
    let destination_liquidity_info = next_account_info(account_info_iter)?;
    let reserve_info = next_account_info(account_info_iter)?;
    let reserve_collateral_mint_info = next_account_info(account_info_iter)?;
    let reserve_liquidity_supply_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let user_transfer_authority_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(next_account_info(account_info_iter)?)?;
    let token_program_id = next_account_info(account_info_iter)?;

    _redeem_reserve_collateral(
        program_id,
        collateral_amount,
        source_collateral_info,
        destination_liquidity_info,
        reserve_info,
        reserve_collateral_mint_info,
        reserve_liquidity_supply_info,
        lending_market_info,
        lending_market_authority_info,
        user_transfer_authority_info,
        clock,
        token_program_id,
    )
}

#[allow(clippy::too_many_arguments)]
fn _redeem_reserve_collateral<'a>(
    program_id: &Pubkey,
    collateral_amount: u64,
    source_collateral_info: &AccountInfo<'a>,
    destination_liquidity_info: &AccountInfo<'a>,
    reserve_info: &AccountInfo<'a>,
    reserve_collateral_mint_info: &AccountInfo<'a>,
    reserve_liquidity_supply_info: &AccountInfo<'a>,
    lending_market_info: &AccountInfo<'a>,
    lending_market_authority_info: &AccountInfo<'a>,
    user_transfer_authority_info: &AccountInfo<'a>,
    clock: &Clock,
    token_program_id: &AccountInfo<'a>,
) -> ProgramResult {
    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }

    let mut reserve = Reserve::unpack(&reserve_info.data.borrow())?;
    if reserve_info.owner != program_id {
        msg!("Reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &reserve.lending_market != lending_market_info.key {
        msg!("Reserve lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.collateral.mint_pubkey != reserve_collateral_mint_info.key {
        msg!("Reserve collateral mint does not match the reserve collateral mint provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.collateral.supply_pubkey == source_collateral_info.key {
        msg!("Reserve collateral supply cannot be used as the source collateral provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.liquidity.supply_pubkey != reserve_liquidity_supply_info.key {
        msg!("Reserve liquidity supply does not match the reserve liquidity supply provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.liquidity.supply_pubkey == destination_liquidity_info.key {
        msg!("Reserve liquidity supply cannot be used as the destination liquidity provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if reserve.last_update.is_stale(clock.slot)? {
        msg!("Reserve is stale and must be refreshed in the current slot");
        return Err(LendingError::ReserveStale.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    let liquidity_amount = reserve.redeem_collateral(collateral_amount)?;
    reserve.last_update.mark_stale();
    Reserve::pack(reserve, &mut reserve_info.data.borrow_mut())?;

    spl_token_burn(TokenBurnParams {
        mint: reserve_collateral_mint_info.clone(),
        source: source_collateral_info.clone(),
        amount: collateral_amount,
        authority: user_transfer_authority_info.clone(),
        authority_signer_seeds: &[],
        token_program: token_program_id.clone(),
    })?;

    spl_token_transfer(TokenTransferParams {
        source: reserve_liquidity_supply_info.clone(),
        destination: destination_liquidity_info.clone(),
        amount: liquidity_amount,
        authority: lending_market_authority_info.clone(),
        authority_signer_seeds,
        token_program: token_program_id.clone(),
    })?;

    Ok(())
}

#[inline(never)] // avoid stack frame limit
fn process_init_obligation(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let obligation_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let obligation_owner_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(next_account_info(account_info_iter)?)?;
    let rent = &Rent::from_account_info(next_account_info(account_info_iter)?)?;
    let token_program_id = next_account_info(account_info_iter)?;

    assert_rent_exempt(rent, obligation_info)?;
    let mut obligation = assert_uninitialized::<Obligation>(obligation_info)?;
    if obligation_info.owner != program_id {
        msg!("Obligation provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }

    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }

    if !obligation_owner_info.is_signer {
        msg!("Obligation owner provided must be a signer");
        return Err(LendingError::InvalidSigner.into());
    }

    obligation.init(InitObligationParams {
        current_slot: clock.slot,
        lending_market: *lending_market_info.key,
        owner: *obligation_owner_info.key,
        deposits: vec![],
        borrows: vec![],
    });
    Obligation::pack(obligation, &mut obligation_info.data.borrow_mut())?;

    Ok(())
}

fn process_refresh_obligation(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter().peekable();
    let obligation_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(next_account_info(account_info_iter)?)?;

    let mut obligation = Obligation::unpack(&obligation_info.data.borrow())?;
    if obligation_info.owner != program_id {
        msg!("Obligation provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }

    let mut deposited_value = Decimal::zero();
    let mut borrowed_value = Decimal::zero();
    let mut allowed_borrow_value = Decimal::zero();
    let mut unhealthy_borrow_value = Decimal::zero();

    for (index, collateral) in obligation.deposits.iter_mut().enumerate() {
        let deposit_reserve_info = next_account_info(account_info_iter)?;
        if deposit_reserve_info.owner != program_id {
            msg!(
                "Deposit reserve provided for collateral {} is not owned by the lending program",
                index
            );
            return Err(LendingError::InvalidAccountOwner.into());
        }
        if collateral.deposit_reserve != *deposit_reserve_info.key {
            msg!(
                "Deposit reserve of collateral {} does not match the deposit reserve provided",
                index
            );
            return Err(LendingError::InvalidAccountInput.into());
        }

        let deposit_reserve = Reserve::unpack(&deposit_reserve_info.data.borrow())?;
        if deposit_reserve.last_update.is_stale(clock.slot)? {
            msg!(
                "Deposit reserve provided for collateral {} is stale and must be refreshed in the current slot",
                index
            );
            return Err(LendingError::ReserveStale.into());
        }

        // @TODO: add lookup table https://git.io/JOCYq
        let decimals = 10u64
            .checked_pow(deposit_reserve.liquidity.mint_decimals as u32)
            .ok_or(LendingError::MathOverflow)?;

        let market_value = deposit_reserve
            .collateral_exchange_rate()?
            .decimal_collateral_to_liquidity(collateral.deposited_amount.into())?
            .try_mul(deposit_reserve.liquidity.market_price)?
            .try_div(decimals)?;
        collateral.market_value = market_value;

        let loan_to_value_rate = Rate::from_percent(deposit_reserve.config.loan_to_value_ratio);
        let liquidation_threshold_rate =
            Rate::from_percent(deposit_reserve.config.liquidation_threshold);

        deposited_value = deposited_value.try_add(market_value)?;
        allowed_borrow_value =
            allowed_borrow_value.try_add(market_value.try_mul(loan_to_value_rate)?)?;
        unhealthy_borrow_value =
            unhealthy_borrow_value.try_add(market_value.try_mul(liquidation_threshold_rate)?)?;
    }

    for (index, liquidity) in obligation.borrows.iter_mut().enumerate() {
        let borrow_reserve_info = next_account_info(account_info_iter)?;
        if borrow_reserve_info.owner != program_id {
            msg!(
                "Borrow reserve provided for liquidity {} is not owned by the lending program",
                index
            );
            return Err(LendingError::InvalidAccountOwner.into());
        }
        if liquidity.borrow_reserve != *borrow_reserve_info.key {
            msg!(
                "Borrow reserve of liquidity {} does not match the borrow reserve provided",
                index
            );
            return Err(LendingError::InvalidAccountInput.into());
        }

        let borrow_reserve = Reserve::unpack(&borrow_reserve_info.data.borrow())?;
        if borrow_reserve.last_update.is_stale(clock.slot)? {
            msg!(
                "Borrow reserve provided for liquidity {} is stale and must be refreshed in the current slot",
                index
            );
            return Err(LendingError::ReserveStale.into());
        }
        // @TODO: add deposit difference to staking pool, consider change staking amount from u64 to Decimal
        liquidity.accrue_interest(borrow_reserve.liquidity.cumulative_borrow_rate_wads)?;

        // @TODO: add lookup table https://git.io/JOCYq
        let decimals = 10u64
            .checked_pow(borrow_reserve.liquidity.mint_decimals as u32)
            .ok_or(LendingError::MathOverflow)?;

        let market_value = liquidity
            .borrowed_amount_wads
            .try_mul(borrow_reserve.liquidity.market_price)?
            .try_div(decimals)?;
        liquidity.market_value = market_value;

        borrowed_value = borrowed_value.try_add(market_value)?;
    }

    if account_info_iter.peek().is_some() {
        msg!("Too many obligation deposit or borrow reserves provided");
        return Err(LendingError::InvalidAccountInput.into());
    }

    obligation.deposited_value = deposited_value;
    obligation.borrowed_value = borrowed_value;
    obligation.allowed_borrow_value = allowed_borrow_value;
    obligation.unhealthy_borrow_value = unhealthy_borrow_value;

    obligation.last_update.update_slot(clock.slot);
    Obligation::pack(obligation, &mut obligation_info.data.borrow_mut())?;

    Ok(())
}

#[inline(never)] // avoid stack frame limit
fn process_deposit_obligation_collateral(
    program_id: &Pubkey,
    collateral_amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if collateral_amount == 0 {
        msg!("Collateral amount provided cannot be zero");
        return Err(LendingError::InvalidAmount.into());
    }

    let account_info_iter = &mut accounts.iter().peekable();
    let source_collateral_info = next_account_info(account_info_iter)?;
    let destination_collateral_info = next_account_info(account_info_iter)?;
    let deposit_reserve_info = next_account_info(account_info_iter)?;
    let obligation_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let obligation_owner_info = next_account_info(account_info_iter)?;
    let user_transfer_authority_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(clock_info)?;
    let token_program_id = next_account_info(account_info_iter)?;
    let deposit_reserve = Reserve::unpack(&deposit_reserve_info.data.borrow())?;
    if account_info_iter.peek().is_some() ^ deposit_reserve.config.deposit_staking_pool.is_some() {
        msg!("This reserve has corresponded staking pool, a stake pool and stake account must be passed in when depositing");
        return Err(LendingError::InvalidStakingPool.into());
    }
    deposit_obligation_collateral(
        program_id,
        collateral_amount,
        source_collateral_info,
        destination_collateral_info,
        deposit_reserve_info,
        obligation_info,
        lending_market_info,
        lending_market_authority_info,
        obligation_owner_info,
        user_transfer_authority_info,
        clock,
        token_program_id,
    )?;

    if account_info_iter.peek().is_some() {
        let stake_account_info = next_account_info(account_info_iter)?;
        let staking_pool_info = next_account_info(account_info_iter)?;
        let staking_program_id = next_account_info(account_info_iter)?;

        if deposit_reserve
            .config
            .deposit_staking_pool
            .map_or(true, |k| k != *staking_pool_info.key)
        {
            msg!("Invalid staking pool, not the one corresponded to the reserve");
            return Err(LendingError::InvalidStakingPool.into());
        }
        deposit_to_staking_program(
            program_id,
            collateral_amount,
            lending_market_info,
            lending_market_authority_info,
            clock_info,
            stake_account_info,
            staking_pool_info,
            staking_program_id,
            *obligation_owner_info.key,
        )
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn deposit_obligation_collateral<'a>(
    program_id: &Pubkey,
    collateral_amount: u64,
    source_collateral_info: &AccountInfo<'a>,
    destination_collateral_info: &AccountInfo<'a>,
    deposit_reserve_info: &AccountInfo<'a>,
    obligation_info: &AccountInfo<'a>,
    lending_market_info: &AccountInfo<'a>,
    lending_market_authority_info: &AccountInfo<'a>,
    obligation_owner_info: &AccountInfo<'a>,
    user_transfer_authority_info: &AccountInfo<'a>,
    clock: &Clock,
    token_program_id: &AccountInfo<'a>,
) -> ProgramResult {
    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }

    let deposit_reserve = Reserve::unpack(&deposit_reserve_info.data.borrow())?;
    if deposit_reserve_info.owner != program_id {
        msg!("Deposit reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &deposit_reserve.lending_market != lending_market_info.key {
        msg!("Deposit reserve lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &deposit_reserve.collateral.supply_pubkey == source_collateral_info.key {
        msg!("Deposit reserve collateral supply cannot be used as the source collateral provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &deposit_reserve.collateral.supply_pubkey != destination_collateral_info.key {
        msg!(
            "Deposit reserve collateral supply must be used as the destination collateral provided"
        );
        return Err(LendingError::InvalidAccountInput.into());
    }
    if deposit_reserve.last_update.is_stale(clock.slot)? {
        msg!("Deposit reserve is stale and must be refreshed in the current slot");
        return Err(LendingError::ReserveStale.into());
    }
    if deposit_reserve.config.loan_to_value_ratio == 0 {
        msg!("Deposit reserve has collateral disabled for borrowing");
        return Err(LendingError::ReserveCollateralDisabled.into());
    }

    let mut obligation = Obligation::unpack(&obligation_info.data.borrow())?;
    if obligation_info.owner != program_id {
        msg!("Obligation provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &obligation.lending_market != lending_market_info.key {
        msg!("Obligation lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &obligation.owner != obligation_owner_info.key {
        msg!("Obligation owner does not match the obligation owner provided");
        return Err(LendingError::InvalidObligationOwner.into());
    }
    if !obligation_owner_info.is_signer {
        msg!("Obligation owner provided must be a signer");
        return Err(LendingError::InvalidSigner.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    obligation
        .find_or_add_collateral_to_deposits(*deposit_reserve_info.key)?
        .deposit(collateral_amount)?;
    obligation.last_update.mark_stale();
    Obligation::pack(obligation, &mut obligation_info.data.borrow_mut())?;

    spl_token_transfer(TokenTransferParams {
        source: source_collateral_info.clone(),
        destination: destination_collateral_info.clone(),
        amount: collateral_amount,
        authority: user_transfer_authority_info.clone(),
        authority_signer_seeds: &[],
        token_program: token_program_id.clone(),
    })?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn deposit_to_staking_program<'a>(
    program_id: &Pubkey,
    collateral_amount: u64,
    lending_market_info: &AccountInfo<'a>,
    lending_market_authority_info: &AccountInfo<'a>,
    clock_info: &AccountInfo<'a>,
    stake_account_info: &AccountInfo<'a>,
    staking_pool_info: &AccountInfo<'a>,
    staking_program_id: &AccountInfo<'a>,
    owner: Pubkey,
) -> ProgramResult {
    if staking_pool_info.owner != staking_program_id.key
        || stake_account_info.owner != staking_program_id.key
    {
        msg!("Wrong staking pool or staking program");
        return Err(LendingError::InvalidStakingPool.into());
    }

    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;

    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    let stake_account = StakeAccount::unpack(&stake_account_info.data.borrow())?;

    if stake_account.owner != owner {
        msg!("You are not depositing to your stake account");
        return Err(LendingError::InvalidStakeAccount.into());
    }
    invoke_signed(
        &deposit(
            *staking_program_id.key,
            collateral_amount,
            *lending_market_authority_info.key,
            *stake_account_info.key,
            *staking_pool_info.key,
        ),
        &(vec![
            lending_market_authority_info.clone(),
            stake_account_info.clone(),
            staking_pool_info.clone(),
            staking_program_id.clone(),
            clock_info.clone(),
        ])[..],
        &[authority_signer_seeds],
    )
}

#[inline(never)] // avoid stack frame limit
fn process_withdraw_obligation_collateral(
    program_id: &Pubkey,
    collateral_amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if collateral_amount == 0 {
        msg!("Collateral amount provided cannot be zero");
        return Err(LendingError::InvalidAmount.into());
    }
    let account_info_iter = &mut accounts.iter().peekable();
    let source_collateral_info = next_account_info(account_info_iter)?;
    let destination_collateral_info = next_account_info(account_info_iter)?;
    let withdraw_reserve_info = next_account_info(account_info_iter)?;
    let obligation_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let obligation_owner_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(clock_info)?;
    let token_program_id = next_account_info(account_info_iter)?;

    let reserve = Reserve::unpack(&withdraw_reserve_info.data.borrow())?;

    if account_info_iter.peek().is_some() ^ reserve.config.deposit_staking_pool.is_some() {
        msg!("This reserve has corresponded staking pool, a stake pool and stake account must be passed in when withdrawing");
        return Err(LendingError::InvalidStakingPool.into());
    }

    let withdraw_amount = withdraw_obligation_collateral(
        program_id,
        collateral_amount,
        source_collateral_info,
        destination_collateral_info,
        withdraw_reserve_info,
        obligation_info,
        lending_market_info,
        lending_market_authority_info,
        obligation_owner_info,
        clock,
        token_program_id,
    )?;

    if account_info_iter.peek().is_some() {
        let stake_account_info = next_account_info(account_info_iter)?;
        let staking_pool_info = next_account_info(account_info_iter)?;
        let staking_program_id = next_account_info(account_info_iter)?;
        let withdraw_reserve_info = Reserve::unpack(&withdraw_reserve_info.data.borrow())?;
        if withdraw_reserve_info
            .config
            .deposit_staking_pool
            .map_or(true, |k| k != *staking_pool_info.key)
        {
            msg!("Invalid staking pool, not the one corresponded to the reserve");
            return Err(LendingError::InvalidStakingPool.into());
        }
        withdraw_from_staking_program(
            program_id,
            withdraw_amount,
            lending_market_info,
            lending_market_authority_info,
            clock_info,
            stake_account_info,
            staking_pool_info,
            staking_program_id,
            *obligation_owner_info.key,
        )
    } else {
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn withdraw_obligation_collateral<'a>(
    program_id: &Pubkey,
    collateral_amount: u64,
    source_collateral_info: &AccountInfo<'a>,
    destination_collateral_info: &AccountInfo<'a>,
    withdraw_reserve_info: &AccountInfo<'a>,
    obligation_info: &AccountInfo<'a>,
    lending_market_info: &AccountInfo<'a>,
    lending_market_authority_info: &AccountInfo<'a>,
    obligation_owner_info: &AccountInfo<'a>,
    clock: &Clock,
    token_program_id: &AccountInfo<'a>,
) -> Result<u64, ProgramError> {
    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }

    let withdraw_reserve = Reserve::unpack(&withdraw_reserve_info.data.borrow())?;
    if withdraw_reserve_info.owner != program_id {
        msg!("Withdraw reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &withdraw_reserve.lending_market != lending_market_info.key {
        msg!("Withdraw reserve lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &withdraw_reserve.collateral.supply_pubkey != source_collateral_info.key {
        msg!("Withdraw reserve collateral supply must be used as the source collateral provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &withdraw_reserve.collateral.supply_pubkey == destination_collateral_info.key {
        msg!("Withdraw reserve collateral supply cannot be used as the destination collateral provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if withdraw_reserve.last_update.is_stale(clock.slot)? {
        msg!("Withdraw reserve is stale and must be refreshed in the current slot");
        return Err(LendingError::ReserveStale.into());
    }

    let mut obligation = Obligation::unpack(&obligation_info.data.borrow())?;
    if obligation_info.owner != program_id {
        msg!("Obligation provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &obligation.lending_market != lending_market_info.key {
        msg!("Obligation lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &obligation.owner != obligation_owner_info.key {
        msg!("Obligation owner does not match the obligation owner provided");
        return Err(LendingError::InvalidObligationOwner.into());
    }
    if !obligation_owner_info.is_signer {
        msg!("Obligation owner provided must be a signer");
        return Err(LendingError::InvalidSigner.into());
    }
    if obligation.last_update.is_stale(clock.slot)? {
        msg!("Obligation is stale and must be refreshed in the current slot");
        return Err(LendingError::ObligationStale.into());
    }

    let (collateral, collateral_index) =
        obligation.find_collateral_in_deposits(*withdraw_reserve_info.key)?;
    if collateral.deposited_amount == 0 {
        msg!("Collateral deposited amount is zero");
        return Err(LendingError::ObligationCollateralEmpty.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    let withdraw_amount = if obligation.borrows.is_empty() {
        if collateral_amount == u64::MAX {
            collateral.deposited_amount
        } else {
            collateral.deposited_amount.min(collateral_amount)
        }
    } else if obligation.deposited_value == Decimal::zero() {
        msg!("Obligation deposited value is zero");
        return Err(LendingError::ObligationDepositsZero.into());
    } else {
        let max_withdraw_value = obligation.max_withdraw_value(Rate::from_percent(
            withdraw_reserve.config.loan_to_value_ratio,
        ))?;
        if max_withdraw_value == Decimal::zero() {
            msg!("Maximum withdraw value is zero");
            return Err(LendingError::WithdrawTooLarge.into());
        }

        let withdraw_amount = if collateral_amount == u64::MAX {
            let withdraw_value = max_withdraw_value.min(collateral.market_value);
            let withdraw_pct = withdraw_value.try_div(collateral.market_value)?;
            withdraw_pct
                .try_mul(collateral.deposited_amount)?
                .try_floor_u64()?
                .min(collateral.deposited_amount)
        } else {
            let withdraw_amount = collateral_amount.min(collateral.deposited_amount);
            let withdraw_pct =
                Decimal::from(withdraw_amount).try_div(collateral.deposited_amount)?;
            let withdraw_value = collateral.market_value.try_mul(withdraw_pct)?;
            if withdraw_value > max_withdraw_value {
                msg!("Withdraw value cannot exceed maximum withdraw value");
                return Err(LendingError::WithdrawTooLarge.into());
            }
            withdraw_amount
        };
        if withdraw_amount == 0 {
            msg!("Withdraw amount is too small to transfer collateral");
            return Err(LendingError::WithdrawTooSmall.into());
        }
        withdraw_amount
    };

    obligation.withdraw(withdraw_amount, collateral_index)?;
    obligation.last_update.mark_stale();
    Obligation::pack(obligation, &mut obligation_info.data.borrow_mut())?;

    spl_token_transfer(TokenTransferParams {
        source: source_collateral_info.clone(),
        destination: destination_collateral_info.clone(),
        amount: withdraw_amount,
        authority: lending_market_authority_info.clone(),
        authority_signer_seeds,
        token_program: token_program_id.clone(),
    })?;

    Ok(withdraw_amount)
}

#[allow(clippy::too_many_arguments)]
fn withdraw_from_staking_program<'a>(
    program_id: &Pubkey,
    collateral_amount: u64,
    lending_market_info: &AccountInfo<'a>,
    lending_market_authority_info: &AccountInfo<'a>,
    clock_info: &AccountInfo<'a>,
    stake_account_info: &AccountInfo<'a>,
    staking_pool_info: &AccountInfo<'a>,
    staking_program_id: &AccountInfo<'a>,
    owner: Pubkey,
) -> ProgramResult {
    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if staking_pool_info.owner != staking_program_id.key
        || stake_account_info.owner != staking_program_id.key
    {
        msg!("Wrong staking pool or staking program");
        return Err(LendingError::InvalidStakingPool.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;

    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }
    let stake_account = StakeAccount::unpack(&stake_account_info.data.borrow())?;

    if stake_account.owner != owner {
        msg!("You are not withdrawing from your stake account");
        return Err(LendingError::InvalidStakeAccount.into());
    }

    invoke_signed(
        &withdraw(
            *staking_program_id.key,
            collateral_amount,
            *lending_market_authority_info.key,
            *stake_account_info.key,
            *staking_pool_info.key,
        ),
        &(vec![
            lending_market_authority_info.clone(),
            stake_account_info.clone(),
            staking_pool_info.clone(),
            staking_program_id.clone(),
            clock_info.clone(),
        ])[..],
        &[authority_signer_seeds],
    )?;

    Ok(())
}

#[inline(never)] // avoid stack frame limit
fn process_borrow_obligation_liquidity(
    program_id: &Pubkey,
    liquidity_amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if liquidity_amount == 0 {
        msg!("Liquidity amount provided cannot be zero");
        return Err(LendingError::InvalidAmount.into());
    }

    let account_info_iter = &mut accounts.iter();
    let source_liquidity_info = next_account_info(account_info_iter)?;
    let destination_liquidity_info = next_account_info(account_info_iter)?;
    let borrow_reserve_info = next_account_info(account_info_iter)?;
    let borrow_reserve_liquidity_fee_receiver_info = next_account_info(account_info_iter)?;
    let obligation_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let obligation_owner_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(next_account_info(account_info_iter)?)?;
    let token_program_id = next_account_info(account_info_iter)?;

    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }

    let mut borrow_reserve = Reserve::unpack(&borrow_reserve_info.data.borrow())?;
    if borrow_reserve_info.owner != program_id {
        msg!("Borrow reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &borrow_reserve.lending_market != lending_market_info.key {
        msg!("Borrow reserve lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &borrow_reserve.liquidity.supply_pubkey != source_liquidity_info.key {
        msg!("Borrow reserve liquidity supply must be used as the source liquidity provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &borrow_reserve.liquidity.supply_pubkey == destination_liquidity_info.key {
        msg!(
            "Borrow reserve liquidity supply cannot be used as the destination liquidity provided"
        );
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &borrow_reserve.liquidity.fee_receiver != borrow_reserve_liquidity_fee_receiver_info.key {
        msg!("Borrow reserve liquidity fee receiver does not match the borrow reserve liquidity fee receiver provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if borrow_reserve.last_update.is_stale(clock.slot)? {
        msg!("Borrow reserve is stale and must be refreshed in the current slot");
        return Err(LendingError::ReserveStale.into());
    }
    if liquidity_amount != u64::MAX
        && Decimal::from(liquidity_amount)
        .try_add(borrow_reserve.liquidity.borrowed_amount_wads)?
        .try_floor_u64()?
        > borrow_reserve.config.borrow_limit
    {
        msg!("Cannot borrow above the borrow limit");
        return Err(LendingError::InvalidAmount.into());
    }

    let mut obligation = Obligation::unpack(&obligation_info.data.borrow())?;
    if obligation_info.owner != program_id {
        msg!("Obligation provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &obligation.lending_market != lending_market_info.key {
        msg!("Obligation lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &obligation.owner != obligation_owner_info.key {
        msg!("Obligation owner does not match the obligation owner provided");
        return Err(LendingError::InvalidObligationOwner.into());
    }
    if !obligation_owner_info.is_signer {
        msg!("Obligation owner provided must be a signer");
        return Err(LendingError::InvalidSigner.into());
    }
    if obligation.last_update.is_stale(clock.slot)? {
        msg!("Obligation is stale and must be refreshed in the current slot");
        return Err(LendingError::ObligationStale.into());
    }
    if obligation.deposits.is_empty() {
        msg!("Obligation has no deposits to borrow against");
        return Err(LendingError::ObligationDepositsEmpty.into());
    }
    if obligation.deposited_value == Decimal::zero() {
        msg!("Obligation deposits have zero value");
        return Err(LendingError::ObligationDepositsZero.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    let remaining_borrow_value = obligation.remaining_borrow_value()?;
    if remaining_borrow_value == Decimal::zero() {
        msg!("Remaining borrow value is zero");
        return Err(LendingError::BorrowTooLarge.into());
    }

    let CalculateBorrowResult {
        borrow_amount,
        receive_amount,
        borrow_fee,
        host_fee: _,
    } = borrow_reserve.calculate_borrow(liquidity_amount, remaining_borrow_value)?;

    if receive_amount == 0 {
        msg!("Borrow amount is too small to receive liquidity after fees");
        return Err(LendingError::BorrowTooSmall.into());
    }

    let cumulative_borrow_rate_wads = borrow_reserve.liquidity.cumulative_borrow_rate_wads;

    borrow_reserve.liquidity.borrow(borrow_amount)?;
    borrow_reserve.last_update.mark_stale();
    Reserve::pack(borrow_reserve, &mut borrow_reserve_info.data.borrow_mut())?;

    let obligation_liquidity =
        obligation.find_or_add_liquidity_to_borrows(*borrow_reserve_info.key)?;
    if obligation_liquidity.cumulative_borrow_rate_wads == Decimal::zero() {
        obligation_liquidity.cumulative_borrow_rate_wads = cumulative_borrow_rate_wads;
    }

    obligation_liquidity.borrow(borrow_amount)?;
    obligation.last_update.mark_stale();
    Obligation::pack(obligation, &mut obligation_info.data.borrow_mut())?;

    if borrow_fee > 0 {
        spl_token_transfer(TokenTransferParams {
            source: source_liquidity_info.clone(),
            destination: borrow_reserve_liquidity_fee_receiver_info.clone(),
            amount: borrow_fee,
            authority: lending_market_authority_info.clone(),
            authority_signer_seeds,
            token_program: token_program_id.clone(),
        })?;
    }

    spl_token_transfer(TokenTransferParams {
        source: source_liquidity_info.clone(),
        destination: destination_liquidity_info.clone(),
        amount: receive_amount,
        authority: lending_market_authority_info.clone(),
        authority_signer_seeds,
        token_program: token_program_id.clone(),
    })
}

#[inline(never)] // avoid stack frame limit
fn process_repay_obligation_liquidity(
    program_id: &Pubkey,
    liquidity_amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if liquidity_amount == 0 {
        msg!("Liquidity amount provided cannot be zero");
        return Err(LendingError::InvalidAmount.into());
    }

    let account_info_iter = &mut accounts.iter();
    let source_liquidity_info = next_account_info(account_info_iter)?;
    let destination_liquidity_info = next_account_info(account_info_iter)?;
    let repay_reserve_info = next_account_info(account_info_iter)?;
    let obligation_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let user_transfer_authority_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(next_account_info(account_info_iter)?)?;
    let token_program_id = next_account_info(account_info_iter)?;

    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }

    let mut repay_reserve = Reserve::unpack(&repay_reserve_info.data.borrow())?;
    if repay_reserve_info.owner != program_id {
        msg!("Repay reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &repay_reserve.lending_market != lending_market_info.key {
        msg!("Repay reserve lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &repay_reserve.liquidity.supply_pubkey == source_liquidity_info.key {
        msg!("Repay reserve liquidity supply cannot be used as the source liquidity provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &repay_reserve.liquidity.supply_pubkey != destination_liquidity_info.key {
        msg!("Repay reserve liquidity supply must be used as the destination liquidity provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if repay_reserve.last_update.is_stale(clock.slot)? {
        msg!("Repay reserve is stale and must be refreshed in the current slot");
        return Err(LendingError::ReserveStale.into());
    }

    let mut obligation = Obligation::unpack(&obligation_info.data.borrow())?;
    if obligation_info.owner != program_id {
        msg!("Obligation provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &obligation.lending_market != lending_market_info.key {
        msg!("Obligation lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if obligation.last_update.is_stale(clock.slot)? {
        msg!("Obligation is stale and must be refreshed in the current slot");
        return Err(LendingError::ObligationStale.into());
    }

    let (liquidity, liquidity_index) =
        obligation.find_liquidity_in_borrows(*repay_reserve_info.key)?;
    if liquidity.borrowed_amount_wads == Decimal::zero() {
        msg!("Liquidity borrowed amount is zero");
        return Err(LendingError::ObligationLiquidityEmpty.into());
    }

    let CalculateRepayResult {
        settle_amount,
        repay_amount,
    } = repay_reserve.calculate_repay(liquidity_amount, liquidity.borrowed_amount_wads)?;

    if repay_amount == 0 {
        msg!("Repay amount is too small to transfer liquidity");
        return Err(LendingError::RepayTooSmall.into());
    }

    repay_reserve.liquidity.repay(repay_amount, settle_amount)?;
    repay_reserve.last_update.mark_stale();
    Reserve::pack(repay_reserve, &mut repay_reserve_info.data.borrow_mut())?;

    obligation.repay(settle_amount, liquidity_index)?;
    obligation.last_update.mark_stale();
    Obligation::pack(obligation, &mut obligation_info.data.borrow_mut())?;

    spl_token_transfer(TokenTransferParams {
        source: source_liquidity_info.clone(),
        destination: destination_liquidity_info.clone(),
        amount: repay_amount,
        authority: user_transfer_authority_info.clone(),
        authority_signer_seeds: &[],
        token_program: token_program_id.clone(),
    })
}

#[inline(never)] // avoid stack frame limit
fn process_liquidate_obligation(
    program_id: &Pubkey,
    liquidity_amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if liquidity_amount == 0 {
        msg!("Liquidity amount provided cannot be zero");
        return Err(LendingError::InvalidAmount.into());
    }

    let account_info_iter = &mut accounts.iter().peekable();
    let source_liquidity_info = next_account_info(account_info_iter)?;
    let destination_collateral_info = next_account_info(account_info_iter)?;
    let repay_reserve_info = next_account_info(account_info_iter)?;
    let repay_reserve_liquidity_supply_info = next_account_info(account_info_iter)?;
    let withdraw_reserve_info = next_account_info(account_info_iter)?;
    let withdraw_reserve_collateral_supply_info = next_account_info(account_info_iter)?;
    let obligation_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let user_transfer_authority_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(clock_info)?;
    let token_program_id = next_account_info(account_info_iter)?;

    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }

    let mut repay_reserve = Reserve::unpack(&repay_reserve_info.data.borrow())?;
    if repay_reserve_info.owner != program_id {
        msg!("Repay reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &repay_reserve.lending_market != lending_market_info.key {
        msg!("Repay reserve lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &repay_reserve.liquidity.supply_pubkey != repay_reserve_liquidity_supply_info.key {
        msg!("Repay reserve liquidity supply does not match the repay reserve liquidity supply provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &repay_reserve.liquidity.supply_pubkey == source_liquidity_info.key {
        msg!("Repay reserve liquidity supply cannot be used as the source liquidity provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &repay_reserve.collateral.supply_pubkey == destination_collateral_info.key {
        msg!(
            "Repay reserve collateral supply cannot be used as the destination collateral provided"
        );
        return Err(LendingError::InvalidAccountInput.into());
    }
    if repay_reserve.last_update.is_stale(clock.slot)? {
        msg!("Repay reserve is stale and must be refreshed in the current slot");
        return Err(LendingError::ReserveStale.into());
    }

    let withdraw_reserve = Reserve::unpack(&withdraw_reserve_info.data.borrow())?;
    if withdraw_reserve_info.owner != program_id {
        msg!("Withdraw reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &withdraw_reserve.lending_market != lending_market_info.key {
        msg!("Withdraw reserve lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &withdraw_reserve.collateral.supply_pubkey != withdraw_reserve_collateral_supply_info.key {
        msg!("Withdraw reserve collateral supply does not match the withdraw reserve collateral supply provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &withdraw_reserve.liquidity.supply_pubkey == source_liquidity_info.key {
        msg!("Withdraw reserve liquidity supply cannot be used as the source liquidity provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &withdraw_reserve.collateral.supply_pubkey == destination_collateral_info.key {
        msg!("Withdraw reserve collateral supply cannot be used as the destination collateral provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if withdraw_reserve.last_update.is_stale(clock.slot)? {
        msg!("Withdraw reserve is stale and must be refreshed in the current slot");
        return Err(LendingError::ReserveStale.into());
    }

    let mut obligation = Obligation::unpack(&obligation_info.data.borrow())?;
    if obligation_info.owner != program_id {
        msg!("Obligation provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &obligation.lending_market != lending_market_info.key {
        msg!("Obligation lending market does not match the lending market provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if obligation.last_update.is_stale(clock.slot)? {
        msg!("Obligation is stale and must be refreshed in the current slot");
        return Err(LendingError::ObligationStale.into());
    }
    if obligation.deposited_value == Decimal::zero() {
        msg!("Obligation deposited value is zero");
        return Err(LendingError::ObligationDepositsZero.into());
    }
    if obligation.borrowed_value == Decimal::zero() {
        msg!("Obligation borrowed value is zero");
        return Err(LendingError::ObligationBorrowsZero.into());
    }
    if obligation.borrowed_value < obligation.unhealthy_borrow_value {
        msg!("Obligation is healthy and cannot be liquidated");
        return Err(LendingError::ObligationHealthy.into());
    }

    let (liquidity, liquidity_index) =
        obligation.find_liquidity_in_borrows(*repay_reserve_info.key)?;
    if liquidity.market_value == Decimal::zero() {
        msg!("Obligation borrow value is zero");
        return Err(LendingError::ObligationLiquidityEmpty.into());
    }

    let (collateral, collateral_index) =
        obligation.find_collateral_in_deposits(*withdraw_reserve_info.key)?;
    if collateral.market_value == Decimal::zero() {
        msg!("Obligation deposit value is zero");
        return Err(LendingError::ObligationCollateralEmpty.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    if withdraw_reserve.config.deposit_staking_pool.is_some() ^ account_info_iter.peek().is_some() {
        msg!(
            "This reserve has corresponded staking pool, \
        a stake pool and stake account must be passed in when depositing"
        );
        return Err(LendingError::InvalidStakingPool.into());
    }

    let CalculateLiquidationResult {
        settle_amount,
        repay_amount,
        withdraw_amount,
    } = withdraw_reserve.calculate_liquidation(
        liquidity_amount,
        &obligation,
        liquidity,
        collateral,
    )?;

    if repay_amount == 0 {
        msg!("Liquidation is too small to transfer liquidity");
        return Err(LendingError::LiquidationTooSmall.into());
    }
    if withdraw_amount == 0 {
        msg!("Liquidation is too small to receive collateral");
        return Err(LendingError::LiquidationTooSmall.into());
    }

    repay_reserve.liquidity.repay(repay_amount, settle_amount)?;
    repay_reserve.last_update.mark_stale();
    Reserve::pack(repay_reserve, &mut repay_reserve_info.data.borrow_mut())?;

    obligation.repay(settle_amount, liquidity_index)?;
    obligation.withdraw(withdraw_amount, collateral_index)?;
    obligation.last_update.mark_stale();
    let obligation_owner = obligation.owner;
    Obligation::pack(obligation, &mut obligation_info.data.borrow_mut())?;

    spl_token_transfer(TokenTransferParams {
        source: source_liquidity_info.clone(),
        destination: repay_reserve_liquidity_supply_info.clone(),
        amount: repay_amount,
        authority: user_transfer_authority_info.clone(),
        authority_signer_seeds: &[],
        token_program: token_program_id.clone(),
    })?;

    spl_token_transfer(TokenTransferParams {
        source: withdraw_reserve_collateral_supply_info.clone(),
        destination: destination_collateral_info.clone(),
        amount: withdraw_amount,
        authority: lending_market_authority_info.clone(),
        authority_signer_seeds,
        token_program: token_program_id.clone(),
    })?;

    if account_info_iter.peek().is_some() {
        let stake_account = next_account_info(account_info_iter)?;
        let staking_pool = next_account_info(account_info_iter)?;
        let staking_program_id = next_account_info(account_info_iter)?;
        let withdraw_reserve = Reserve::unpack(&withdraw_reserve_info.data.borrow())?;
        if withdraw_reserve
            .config
            .deposit_staking_pool
            .map_or(true, |k| k != *staking_pool.key)
        {
            msg!("Invalid staking pool, not the one corresponded to the reserve");
            return Err(LendingError::InvalidStakingPool.into());
        }

        withdraw_from_staking_program(
            program_id,
            withdraw_amount,
            lending_market_info,
            lending_market_authority_info,
            clock_info,
            stake_account,
            staking_pool,
            staking_program_id,
            obligation_owner,
        )
    } else {
        Ok(())
    }
}

#[inline(never)] // avoid stack frame limit
fn process_flash_loan(
    program_id: &Pubkey,
    liquidity_amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if liquidity_amount == 0 {
        msg!("Liquidity amount provided cannot be zero");
        return Err(LendingError::InvalidAmount.into());
    }

    let account_info_iter = &mut accounts.iter();
    let source_liquidity_info = next_account_info(account_info_iter)?;
    let destination_liquidity_info = next_account_info(account_info_iter)?;
    let reserve_info = next_account_info(account_info_iter)?;
    let reserve_liquidity_fee_receiver_info = next_account_info(account_info_iter)?;
    let host_fee_receiver_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let token_program_id = next_account_info(account_info_iter)?;
    let flash_loan_receiver_program_id = next_account_info(account_info_iter)?;

    if program_id == flash_loan_receiver_program_id.key {
        msg!("Lending program cannot be used as the flash loan receiver program provided");
        return Err(LendingError::InvalidFlashLoanReceiverProgram.into());
    }

    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    let mut reserve = Reserve::unpack(&reserve_info.data.borrow())?;
    if reserve_info.owner != program_id {
        msg!("Reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &reserve.lending_market != lending_market_info.key {
        msg!("Invalid reserve lending market account");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.liquidity.supply_pubkey != source_liquidity_info.key {
        msg!("Reserve liquidity supply must be used as the source liquidity provided");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if &reserve.liquidity.fee_receiver != reserve_liquidity_fee_receiver_info.key {
        msg!("Reserve liquidity fee receiver does not match the reserve liquidity fee receiver provided");
        return Err(LendingError::InvalidAccountInput.into());
    }

    // @FIXME: if u64::MAX is flash loaned, fees should be inclusive as with ordinary borrows
    let flash_loan_amount = if liquidity_amount == u64::MAX {
        reserve.liquidity.available_amount
    } else {
        liquidity_amount
    };

    let flash_loan_amount_decimal = Decimal::from(flash_loan_amount);
    let (origination_fee, host_fee) = reserve
        .config
        .fees
        .calculate_flash_loan_fees(flash_loan_amount_decimal)?;

    let balance_before_flash_loan = Account::unpack(&source_liquidity_info.data.borrow())?.amount;
    let expected_balance_after_flash_loan = balance_before_flash_loan
        .checked_add(origination_fee)
        .ok_or(LendingError::MathOverflow)?;
    let returned_amount_required = flash_loan_amount
        .checked_add(origination_fee)
        .ok_or(LendingError::MathOverflow)?;

    let mut flash_loan_instruction_accounts = vec![
        AccountMeta::new(*destination_liquidity_info.key, false),
        AccountMeta::new(*source_liquidity_info.key, false),
        AccountMeta::new_readonly(*token_program_id.key, false),
    ];
    let mut flash_loan_instruction_account_infos = vec![
        destination_liquidity_info.clone(),
        flash_loan_receiver_program_id.clone(),
        source_liquidity_info.clone(),
        token_program_id.clone(),
    ];
    for account_info in account_info_iter {
        flash_loan_instruction_accounts.push(AccountMeta {
            pubkey: *account_info.key,
            is_signer: account_info.is_signer,
            is_writable: account_info.is_writable,
        });
        flash_loan_instruction_account_infos.push(account_info.clone());
    }

    reserve.liquidity.borrow(flash_loan_amount_decimal)?;
    Reserve::pack(reserve, &mut reserve_info.data.borrow_mut())?;

    spl_token_transfer(TokenTransferParams {
        source: source_liquidity_info.clone(),
        destination: destination_liquidity_info.clone(),
        amount: flash_loan_amount,
        authority: lending_market_authority_info.clone(),
        authority_signer_seeds,
        token_program: token_program_id.clone(),
    })?;

    const RECEIVE_FLASH_LOAN_INSTRUCTION_DATA_SIZE: usize = 9;
    // @FIXME: don't use 0 to indicate a flash loan receiver instruction https://git.io/JGzz9
    const RECEIVE_FLASH_LOAN_INSTRUCTION_TAG: u8 = 0u8;

    let mut data = Vec::with_capacity(RECEIVE_FLASH_LOAN_INSTRUCTION_DATA_SIZE);
    data.push(RECEIVE_FLASH_LOAN_INSTRUCTION_TAG);
    data.extend_from_slice(&returned_amount_required.to_le_bytes());

    invoke(
        &Instruction {
            program_id: *flash_loan_receiver_program_id.key,
            accounts: flash_loan_instruction_accounts,
            data,
        },
        &flash_loan_instruction_account_infos[..],
    )?;

    reserve = Reserve::unpack(&reserve_info.data.borrow())?;
    reserve
        .liquidity
        .repay(flash_loan_amount, flash_loan_amount_decimal)?;
    Reserve::pack(reserve, &mut reserve_info.data.borrow_mut())?;

    let actual_balance_after_flash_loan =
        Account::unpack(&source_liquidity_info.data.borrow())?.amount;
    if actual_balance_after_flash_loan < expected_balance_after_flash_loan {
        msg!("Insufficient reserve liquidity after flash loan");
        return Err(LendingError::NotEnoughLiquidityAfterFlashLoan.into());
    }

    let mut owner_fee = origination_fee;
    if host_fee > 0 {
        owner_fee = owner_fee
            .checked_sub(host_fee)
            .ok_or(LendingError::MathOverflow)?;
        spl_token_transfer(TokenTransferParams {
            source: source_liquidity_info.clone(),
            destination: host_fee_receiver_info.clone(),
            amount: host_fee,
            authority: lending_market_authority_info.clone(),
            authority_signer_seeds,
            token_program: token_program_id.clone(),
        })?;
    }

    if owner_fee > 0 {
        spl_token_transfer(TokenTransferParams {
            source: source_liquidity_info.clone(),
            destination: reserve_liquidity_fee_receiver_info.clone(),
            amount: owner_fee,
            authority: lending_market_authority_info.clone(),
            authority_signer_seeds,
            token_program: token_program_id.clone(),
        })?;
    }

    Ok(())
}

#[inline(never)] // avoid stack frame limit
fn process_deposit_reserve_liquidity_and_obligation_collateral(
    program_id: &Pubkey,
    liquidity_amount: u64,
    accounts: &[AccountInfo],
) -> ProgramResult {
    if liquidity_amount == 0 {
        msg!("Liquidity amount provided cannot be zero");
        return Err(LendingError::InvalidAmount.into());
    }

    let account_info_iter = &mut accounts.iter().peekable();
    let source_liquidity_info = next_account_info(account_info_iter)?;
    let user_collateral_info = next_account_info(account_info_iter)?;
    let reserve_info = next_account_info(account_info_iter)?;
    let reserve_liquidity_supply_info = next_account_info(account_info_iter)?;
    let reserve_collateral_mint_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let destination_collateral_info = next_account_info(account_info_iter)?;
    let obligation_info = next_account_info(account_info_iter)?;
    let obligation_owner_info = next_account_info(account_info_iter)?;
    let user_transfer_authority_info = next_account_info(account_info_iter)?;
    let clock_info = next_account_info(account_info_iter)?;
    let clock = &Clock::from_account_info(clock_info)?;
    let token_program_id = next_account_info(account_info_iter)?;

    let mut reserve = Reserve::unpack(&reserve_info.data.borrow())?;

    if reserve.config.deposit_staking_pool.is_some() ^ account_info_iter.peek().is_some() {
        msg!(
            "This reserve has corresponded staking pool, \
        a stake pool and stake account must be passed in when depositing"
        );
        return Err(LendingError::InvalidStakingPool.into());
    }

    let collateral_amount = _deposit_reserve_liquidity(
        program_id,
        liquidity_amount,
        source_liquidity_info,
        user_collateral_info,
        reserve_info,
        reserve_liquidity_supply_info,
        reserve_collateral_mint_info,
        lending_market_info,
        lending_market_authority_info,
        user_transfer_authority_info,
        clock,
        token_program_id,
    )?;

    reserve = Reserve::unpack(&reserve_info.data.borrow())?;
    reserve.last_update.update_slot(clock.slot);
    let reserve_deposit_pool = reserve.config.deposit_staking_pool;
    Reserve::pack(reserve, &mut reserve_info.data.borrow_mut())?;

    deposit_obligation_collateral(
        program_id,
        collateral_amount,
        user_collateral_info,
        destination_collateral_info,
        reserve_info,
        obligation_info,
        lending_market_info,
        lending_market_authority_info,
        obligation_owner_info,
        user_transfer_authority_info,
        clock,
        token_program_id,
    )?;

    if account_info_iter.peek().is_some() {
        let stake_account_info = next_account_info(account_info_iter)?;
        let staking_pool_info = next_account_info(account_info_iter)?;
        let staking_program_id = next_account_info(account_info_iter)?;

        if reserve_deposit_pool.map_or(true, |k| k != *staking_pool_info.key) {
            msg!("Invalid staking pool, not the one corresponded to the reserve");
            return Err(LendingError::InvalidStakingPool.into());
        }
        deposit_to_staking_program(
            program_id,
            collateral_amount,
            lending_market_info,
            lending_market_authority_info,
            clock_info,
            stake_account_info,
            staking_pool_info,
            staking_program_id,
            *obligation_owner_info.key,
        )
    } else {
        Ok(())
    }
}

fn process_update_reserve(
    program_id: &Pubkey,
    config: ReserveConfig,
    accounts: &[AccountInfo],
) -> ProgramResult {
    validate_reserve_config(config)?;

    let account_info_iter = &mut accounts.iter();
    let reserve_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let lending_market_owner_info = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let rent = &Rent::from_account_info(rent_info)?;
    let token_program_id = next_account_info(account_info_iter)?;

    assert_rent_exempt(rent, reserve_info)?;
    let mut reserve = Reserve::unpack(&reserve_info.data.borrow())?;

    if reserve_info.owner != program_id {
        msg!("Reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }

    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }
    if &lending_market.owner != lending_market_owner_info.key {
        msg!("Lending market owner does not match the lending market owner provided");
        return Err(LendingError::InvalidMarketOwner.into());
    }
    if !lending_market_owner_info.is_signer {
        msg!("Lending market owner provided must be a signer");
        return Err(LendingError::InvalidSigner.into());
    }
    if &reserve.lending_market != lending_market_info.key {
        msg!("Invalid reserve lending market account");
        return Err(LendingError::InvalidAccountInput.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    reserve.config = config;
    msg!("Updated reserve config.");

    Reserve::pack(reserve, &mut reserve_info.data.borrow_mut())?;

    Ok(())
}

fn process_withdraw_fee(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let reserve_info = next_account_info(account_info_iter)?;
    let lending_market_info = next_account_info(account_info_iter)?;
    let lending_market_authority_info = next_account_info(account_info_iter)?;
    let lending_market_owner_info = next_account_info(account_info_iter)?;
    let reserve_fee_token_info = next_account_info(account_info_iter)?;
    let destination_token_info = next_account_info(account_info_iter)?;
    let rent_info = next_account_info(account_info_iter)?;
    let rent = &Rent::from_account_info(rent_info)?;
    let token_program_id = next_account_info(account_info_iter)?;

    assert_rent_exempt(rent, reserve_info)?;
    let reserve = Reserve::unpack(&reserve_info.data.borrow())?;

    if reserve_info.owner != program_id {
        msg!("Reserve provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }

    let lending_market = LendingMarket::unpack(&lending_market_info.data.borrow())?;
    if lending_market_info.owner != program_id {
        msg!("Lending market provided is not owned by the lending program");
        return Err(LendingError::InvalidAccountOwner.into());
    }
    if &lending_market.token_program_id != token_program_id.key {
        msg!("Lending market token program does not match the token program provided");
        return Err(LendingError::InvalidTokenProgram.into());
    }
    if &lending_market.owner != lending_market_owner_info.key {
        msg!("Lending market owner does not match the lending market owner provided");
        return Err(LendingError::InvalidMarketOwner.into());
    }
    if !lending_market_owner_info.is_signer {
        msg!("Lending market owner provided must be a signer");
        return Err(LendingError::InvalidSigner.into());
    }
    if &reserve.lending_market != lending_market_info.key {
        msg!("Invalid reserve lending market account");
        return Err(LendingError::InvalidAccountInput.into());
    }
    if reserve.liquidity.fee_receiver != *reserve_fee_token_info.key {
        msg!("Invalid reserve fee token account");
        return Err(LendingError::InvalidReserveFeeAccount.into());
    }

    let authority_signer_seeds = &[
        lending_market_info.key.as_ref(),
        &[lending_market.bump_seed],
    ];
    let lending_market_authority_pubkey =
        Pubkey::create_program_address(authority_signer_seeds, program_id)?;
    if &lending_market_authority_pubkey != lending_market_authority_info.key {
        msg!(
            "Derived lending market authority does not match the lending market authority provided"
        );
        return Err(LendingError::InvalidMarketAuthority.into());
    }

    let fee_token = Account::unpack(&reserve_fee_token_info.data.borrow())?;

    msg!("amount {:?} ", fee_token.amount);
    spl_token_transfer(TokenTransferParams {
        source: reserve_fee_token_info.clone(),
        destination: destination_token_info.clone(),
        authority: lending_market_authority_info.clone(),
        token_program: token_program_id.clone(),
        amount: fee_token.amount,
        authority_signer_seeds,
    })
}

fn assert_rent_exempt(rent: &Rent, account_info: &AccountInfo) -> ProgramResult {
    if !rent.is_exempt(account_info.lamports(), account_info.data_len()) {
        msg!(&rent.minimum_balance(account_info.data_len()).to_string());
        Err(LendingError::NotRentExempt.into())
    } else {
        Ok(())
    }
}

fn assert_uninitialized<T: Pack + IsInitialized>(
    account_info: &AccountInfo,
) -> Result<T, ProgramError> {
    let account: T = T::unpack_unchecked(&account_info.data.borrow())?;
    if account.is_initialized() {
        Err(LendingError::AlreadyInitialized.into())
    } else {
        Ok(account)
    }
}

/// Unpacks a spl_token `Mint`.
fn unpack_mint(data: &[u8]) -> Result<Mint, LendingError> {
    Mint::unpack(data).map_err(|_| LendingError::InvalidTokenMint)
}

fn get_switchboard_price(
    switchboard_feed_account: &AccountInfo,
    clock: &Clock,
) -> Result<Decimal, ProgramError> {
    if is_switchbaord_program_v2(switchboard_feed_account.owner) {
        return get_switchboard_price_v2(switchboard_feed_account, clock);
    }

    const STALE_AFTER_SLOTS_ELAPSED: u64 = 240;
    let account_buf = switchboard_feed_account.try_borrow_data()?;
    if account_buf.len() == 0 {
        msg!("The provided account is empty.");
        return Err(ProgramError::InvalidAccountData);
    }
    let price = if account_buf[0] == SwitchboardAccountType::TYPE_AGGREGATOR as u8 {
        let aggregator = get_aggregator(switchboard_feed_account).map_err(|e| {
            msg!("Aggregator parse failed. Please double check the provided address.");
            e
        })?;
        let round_result = get_aggregator_result(&aggregator).map_err(|e| {
            msg!("Failed to parse an aggregator round. Has update been called on the aggregator?");
            e
        })?;

        round_result
            .round_open_slot
            .and_then(|slot| {
                let slots_elapsed = clock.slot.checked_sub(slot)?;
                if slots_elapsed >= STALE_AFTER_SLOTS_ELAPSED {
                    msg!("Oracle price is stale");
                    return None;
                }
                round_result.result
            })
            .ok_or(LendingError::InvalidOracleConfig)
    } else if account_buf[0] == SwitchboardAccountType::TYPE_AGGREGATOR_RESULT_PARSE_OPTIMIZED as u8
    {
        let feed_data = FastRoundResultAccountData::deserialize(&account_buf).unwrap();
        Ok(feed_data.result.result)
    } else {
        Err(LendingError::InvalidOracleConfig)
    }?;

    if price < 0.0 {
        msg!("Oracle price cannot be negative");
        return Err(LendingError::InvalidOracleConfig.into());
    }

    Ok(Decimal::from(price))
}

fn get_switchboard_price_v2(
    switchboard_feed_info: &AccountInfo,
    clock: &Clock,
) -> Result<Decimal, ProgramError> {
    const STALE_AFTER_SLOTS_ELAPSED: u64 = 240;

    let feed = AggregatorAccountData::new(switchboard_feed_info)?;
    let slots_elapsed = clock
        .slot
        .checked_sub(feed.latest_confirmed_round.round_open_slot)
        .ok_or(LendingError::MathOverflow)?;
    if slots_elapsed >= STALE_AFTER_SLOTS_ELAPSED {
        msg!("Switchboard oracle price is stale");
        return Err(LendingError::InvalidOracleConfig.into());
    }

    let price_switchboard_desc = feed.get_result()?;
    if price_switchboard_desc.mantissa < 0 {
        msg!("Switchboard oracle price is negative which is not allowed");
        return Err(LendingError::InvalidOracleConfig.into());
    }
    let price = Decimal::from(price_switchboard_desc.mantissa as u128);
    let exp = Decimal::from((10u128).checked_pow(price_switchboard_desc.scale).unwrap());
    price.try_div(exp)
}

fn get_pyth_price(pyth_price_info: &AccountInfo, clock: &Clock) -> Result<Decimal, ProgramError> {
    const STALE_AFTER_SLOTS_ELAPSED: u64 = 240;

    let pyth_price_data = pyth_price_info.try_borrow_data()?;
    let pyth_price = pyth::load::<pyth::Price>(&pyth_price_data)
        .map_err(|_| ProgramError::InvalidAccountData)?;

    if pyth_price.ptype != pyth::PriceType::Price {
        msg!("Oracle price type is invalid");
        return Err(LendingError::InvalidOracleConfig.into());
    }

    if !matches!(pyth_price.agg.status, pyth::PriceStatus::Trading) {
        msg!("Oracle price status is not trading");
        return Err(LendingError::InvalidOracleConfig.into());
    }

    let slots_elapsed = clock
        .slot
        .checked_sub(pyth_price.valid_slot)
        .ok_or(LendingError::MathOverflow)?;
    if slots_elapsed >= STALE_AFTER_SLOTS_ELAPSED {
        msg!("Oracle price is stale");
        return Err(LendingError::InvalidOracleConfig.into());
    }

    let price: u64 = pyth_price.agg.price.try_into().map_err(|_| {
        msg!("Oracle price cannot be negative");
        LendingError::InvalidOracleConfig
    })?;

    let market_price = if pyth_price.expo >= 0 {
        let exponent = pyth_price
            .expo
            .try_into()
            .map_err(|_| LendingError::MathOverflow)?;
        let zeros = 10u64
            .checked_pow(exponent)
            .ok_or(LendingError::MathOverflow)?;
        Decimal::from(price).try_mul(zeros)?
    } else {
        let exponent = pyth_price
            .expo
            .checked_abs()
            .ok_or(LendingError::MathOverflow)?
            .try_into()
            .map_err(|_| LendingError::MathOverflow)?;
        let decimals = 10u64
            .checked_pow(exponent)
            .ok_or(LendingError::MathOverflow)?;
        Decimal::from(price).try_div(decimals)?
    };

    Ok(market_price)
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
    result.map_err(|_| LendingError::TokenInitializeAccountFailed.into())
}

/// Issue a spl_token `InitializeMint` instruction.
#[inline(always)]
fn spl_token_init_mint(params: TokenInitializeMintParams<'_, '_>) -> ProgramResult {
    let TokenInitializeMintParams {
        mint,
        rent,
        authority,
        token_program,
        decimals,
    } = params;
    let ix = spl_token::instruction::initialize_mint(
        token_program.key,
        mint.key,
        authority,
        None,
        decimals,
    )?;
    let result = invoke(&ix, &[mint, rent, token_program]);
    result.map_err(|_| LendingError::TokenInitializeMintFailed.into())
}

/// Invoke signed unless signers seeds are empty
#[inline(always)]
fn invoke_optionally_signed(
    instruction: &Instruction,
    account_infos: &[AccountInfo],
    authority_signer_seeds: &[&[u8]],
) -> ProgramResult {
    if authority_signer_seeds.is_empty() {
        invoke(instruction, account_infos)
    } else {
        invoke_signed(instruction, account_infos, &[authority_signer_seeds])
    }
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
    let result = invoke_optionally_signed(
        &spl_token::instruction::transfer(
            token_program.key,
            source.key,
            destination.key,
            authority.key,
            &[],
            amount,
        )?,
        &[source, destination, authority, token_program],
        authority_signer_seeds,
    );
    result.map_err(|_| LendingError::TokenTransferFailed.into())
}

/// Issue a spl_token `MintTo` instruction.
fn spl_token_mint_to(params: TokenMintToParams<'_, '_>) -> ProgramResult {
    let TokenMintToParams {
        mint,
        destination,
        authority,
        token_program,
        amount,
        authority_signer_seeds,
    } = params;
    let result = invoke_optionally_signed(
        &spl_token::instruction::mint_to(
            token_program.key,
            mint.key,
            destination.key,
            authority.key,
            &[],
            amount,
        )?,
        &[mint, destination, authority, token_program],
        authority_signer_seeds,
    );
    result.map_err(|_| LendingError::TokenMintToFailed.into())
}

/// Issue a spl_token `Burn` instruction.
#[inline(always)]
fn spl_token_burn(params: TokenBurnParams<'_, '_>) -> ProgramResult {
    let TokenBurnParams {
        mint,
        source,
        authority,
        token_program,
        amount,
        authority_signer_seeds,
    } = params;
    let result = invoke_optionally_signed(
        &spl_token::instruction::burn(
            token_program.key,
            source.key,
            mint.key,
            authority.key,
            &[],
            amount,
        )?,
        &[source, mint, authority, token_program],
        authority_signer_seeds,
    );
    result.map_err(|_| LendingError::TokenBurnFailed.into())
}

struct TokenInitializeMintParams<'a: 'b, 'b> {
    mint: AccountInfo<'a>,
    rent: AccountInfo<'a>,
    authority: &'b Pubkey,
    decimals: u8,
    token_program: AccountInfo<'a>,
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

struct TokenMintToParams<'a: 'b, 'b> {
    mint: AccountInfo<'a>,
    destination: AccountInfo<'a>,
    amount: u64,
    authority: AccountInfo<'a>,
    authority_signer_seeds: &'b [&'b [u8]],
    token_program: AccountInfo<'a>,
}

struct TokenBurnParams<'a: 'b, 'b> {
    mint: AccountInfo<'a>,
    source: AccountInfo<'a>,
    amount: u64,
    authority: AccountInfo<'a>,
    authority_signer_seeds: &'b [&'b [u8]],
    token_program: AccountInfo<'a>,
}

impl PrintProgramError for LendingError {
    fn print<E>(&self)
    where
        E: 'static + std::error::Error + DecodeError<E> + PrintProgramError + FromPrimitive,
    {
        msg!(&self.to_string());
    }
}
