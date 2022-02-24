use std::convert::TryInto;

use solana_program::{
    msg,
    program_error::ProgramError,
    program_pack::Pack,
    pubkey::{Pubkey, PUBKEY_BYTES},
};

use crate::math::Decimal;

pub mod stake_account;
pub mod staking_pool;

pub const PROGRAM_VERSION: u8 = 1;
/// Accounts are created with data zeroed out, so uninitialized state instances
/// will have the version set to 0.
pub const UNINITIALIZED_VERSION: u8 = 0;

///pack coption of pubkey into buffer
pub fn pack_option_key(src: &Option<Pubkey>, dst: &mut [u8; 1 + PUBKEY_BYTES]) {
    match src {
        Option::Some(key) => {
            dst[0] = 1;
            dst[1..].copy_from_slice(key.as_ref());
        }
        Option::None => {
            dst[0] = 0;
        }
    }
}

///unpack coption pubkey from buffer
pub fn unpack_option_key(src: &[u8; 1 + PUBKEY_BYTES]) -> Result<Option<Pubkey>, ProgramError> {
    match src[0] {
        0 => Ok(Option::None),
        1 => Ok(Option::Some(Pubkey::new_from_array(
            src[1..].try_into().unwrap(),
        ))),
        _ => {
            msg!("Option<Pubkey> cannot be unpacked");
            Err(ProgramError::InvalidAccountData)
        }
    }
}

///pack coption of pubkey into buffer
pub fn pack_option_decimal(src: &Option<Decimal>, dst: &mut [u8; 1 + Decimal::LEN]) {
    match src {
        Option::Some(x) => {
            dst[0] = 1;
            x.pack_into_slice(&mut dst[1..]);
        }
        Option::None => {
            dst[0] = 0;
        }
    }
}

///unpack coption pubkey from buffer
pub fn unpack_option_decimal(
    src: &[u8; 1 + Decimal::LEN],
) -> Result<Option<Decimal>, ProgramError> {
    match src[0] {
        0 => Ok(Option::None),
        1 => Ok(Option::Some(Decimal::unpack_from_slice(&src[1..])?)),
        _ => {
            msg!("Option<Decimal> cannot be unpacked");
            Err(ProgramError::InvalidAccountData)
        }
    }
}
