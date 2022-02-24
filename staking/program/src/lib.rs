pub mod entrypoint;
pub mod error;
pub mod instruction;
pub mod math;
pub mod processor;
pub mod state;

// pub mod state;
pub use solana_program;

solana_program::declare_id!("stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq");

#[macro_export]
macro_rules! dummy_id {
    () => {
        solana_program::pubkey::Pubkey::default()
    };
}
