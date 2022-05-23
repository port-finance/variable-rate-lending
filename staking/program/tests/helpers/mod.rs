#![allow(dead_code)]

use std::str::FromStr;

use assert_matches::*;
use do_notation::{m, Lift};
use num_traits::abs;
use solana_program::program_option::COption;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use solana_program::system_instruction::create_account;
use solana_program_test::{BanksClient, ProgramTest};
use solana_sdk::account::Account;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::transaction::{Transaction, TransactionError};
use spl_token::state::Account as Token;
use spl_token::state::{AccountState, Mint};

use port_finance_staking::instruction::*;
use port_finance_staking::math::TryMul;
use port_finance_staking::solana_program::clock::Slot;
use port_finance_staking::solana_program::instruction::InstructionError;
use port_finance_staking::solana_program::program_error::ProgramError;
use port_finance_staking::state::stake_account::StakeAccount;
use port_finance_staking::state::staking_pool::{CumulativeRate, StakingPool};
use port_finance_staking::state::PROGRAM_VERSION;

#[macro_export]
macro_rules! staking_test {
    () => {
        solana_program_test::ProgramTest::new(
            "port_finance_staking",
            port_finance_staking::id(),
            solana_program_test::processor!(port_finance_staking::processor::process_instruction),
        )
    };
}
pub const QUOTE_CURRENCY: [u8; 32] =
    *b"USD\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

pub trait AddPacked {
    fn add_packable_account<T: Pack>(
        &mut self,
        pubkey: Pubkey,
        amount: u64,
        data: &T,
        owner: &Pubkey,
    );
}

impl AddPacked for ProgramTest {
    fn add_packable_account<T: Pack>(
        &mut self,
        pubkey: Pubkey,
        amount: u64,
        data: &T,
        owner: &Pubkey,
    ) {
        let mut account = Account::new(amount, T::get_packed_len(), owner);
        data.pack_into_slice(&mut account.data);
        self.add_account(pubkey, account);
    }
}

pub fn add_stake_account(test: &mut ProgramTest, staking_pool: Pubkey) -> TestStakeAccount {
    let owner = Keypair::new();
    let stake_account_pubkey = Pubkey::new_unique();
    let mut stake_account = StakeAccount::default();
    stake_account.init(owner.pubkey(), staking_pool).unwrap();
    test.add_packable_account(
        stake_account_pubkey,
        u32::MAX as u64,
        &stake_account,
        &port_finance_staking::id(),
    );

    TestStakeAccount {
        name: "Solana Staking account".to_owned(),
        pubkey: stake_account_pubkey,
        owner,
        stake_account,
    }
}

#[derive(Debug)]
pub struct TestStakeAccount {
    pub name: String,
    pub pubkey: Pubkey,
    pub owner: Keypair,
    pub stake_account: StakeAccount,
}

impl TestStakeAccount {
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        name: String,
        banks_client: &mut BanksClient,

        staking_pool: Pubkey,

        payer: &Keypair,
    ) -> Result<Self, TransactionError> {
        let stake_account_owner = Keypair::new();
        let stake_account_keypair = Keypair::new();
        let rent = banks_client.get_rent().await.unwrap();
        let mut transaction = Transaction::new_with_payer(
            &[
                create_account(
                    &payer.pubkey(),
                    &stake_account_keypair.pubkey(),
                    rent.minimum_balance(StakeAccount::LEN) + 100,
                    StakeAccount::LEN as u64,
                    &port_finance_staking::id(),
                ),
                create_stake_account(
                    port_finance_staking::id(),
                    stake_account_keypair.pubkey(),
                    staking_pool,
                    stake_account_owner.pubkey(),
                ),
            ],
            Some(&payer.pubkey()),
        );

        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&vec![payer, &stake_account_keypair], recent_blockhash);
        let mut stake_account = StakeAccount::default();
        stake_account
            .init(stake_account_owner.pubkey(), staking_pool)
            .unwrap();
        banks_client
            .process_transaction(transaction)
            .await
            .map(|_| Self {
                name,
                pubkey: stake_account_keypair.pubkey(),
                owner: stake_account_owner,
                stake_account,
            })
            .map_err(|e| e.unwrap())
    }

    pub fn deposit(&mut self, amount: u64, rate: CumulativeRate) -> Result<(), TransactionError> {
        self.stake_account
            .deposit(rate, amount)
            .map_err(program_to_transaction_error(0))
    }

    pub fn withdraw(&mut self, amount: u64, rate: CumulativeRate) -> Result<(), TransactionError> {
        self.stake_account
            .withdraw(rate, amount)
            .map_err(program_to_transaction_error(0))
    }

    pub fn claim_reward(
        &mut self,
        rate: CumulativeRate,
    ) -> Result<(u64, Option<u64>), TransactionError> {
        println!("start rate {:?}", self.stake_account.start_rate);
        println!("current rate {:?}", rate);
        self.stake_account
            .claim_reward(rate)
            .map_err(program_to_transaction_error(0))
    }

    pub async fn get_state(&self, banks_client: &mut BanksClient) -> StakeAccount {
        let stake_account: Account = banks_client
            .get_account(self.pubkey)
            .await
            .unwrap()
            .unwrap();
        StakeAccount::unpack(&stake_account.data[..]).unwrap()
    }

    pub async fn validate_state(&self, banks_client: &mut BanksClient) {
        let stake_account = self.get_state(banks_client).await;
        assert_eq!(PROGRAM_VERSION, stake_account.version);
        assert_eq!(self.stake_account, stake_account);
    }
}

pub fn add_staking_pool(
    test: &mut ProgramTest,
    mint: Pubkey,
    duration: u64,
    supply: u64,
    sub_supply: Option<u64>,
    earliest_claim_time: u64,
) -> TestStakingPool {
    let staking_pool_owner = Keypair::new();
    let staking_pool_admin = Keypair::new();
    let staking_pool_pubkey = Pubkey::new_unique();
    let (staking_program_derived, bump_seed) =
        Pubkey::find_program_address(&[staking_pool_pubkey.as_ref()], &port_finance_staking::id());
    let reward_token_pool_pubkey = Pubkey::new_unique();
    test.add_packable_account(
        reward_token_pool_pubkey,
        u32::MAX as u64,
        &Token {
            mint,
            owner: staking_program_derived,
            amount: supply,
            state: AccountState::Initialized,
            ..Token::default()
        },
        &spl_token::id(),
    );
    let sub_reward_token_pool_pubkey = sub_supply.map(|supply| {
        let sub_reward_token_pool_pubkey = Pubkey::new_unique();
        test.add_packable_account(
            sub_reward_token_pool_pubkey,
            u32::MAX as u64,
            &Token {
                mint,
                owner: staking_program_derived,
                amount: supply,
                state: AccountState::Initialized,
                ..Token::default()
            },
            &spl_token::id(),
        );
        sub_reward_token_pool_pubkey
    });
    let mut staking_pool = StakingPool::default();
    staking_pool
        .init(
            staking_pool_owner.pubkey(),
            staking_pool_admin.pubkey(),
            reward_token_pool_pubkey,
            sub_reward_token_pool_pubkey,
            duration,
            supply,
            sub_supply,
            earliest_claim_time,
            bump_seed,
        )
        .unwrap();

    test.add_packable_account(
        staking_pool_pubkey,
        u32::MAX as u64,
        &staking_pool,
        &port_finance_staking::id(),
    );

    TestStakingPool {
        name: "Solana Staking Pool".to_owned(),
        pubkey: staking_pool_pubkey,
        staking_pool,
        staking_pool_owner,
        staking_pool_admin,
    }
}
pub fn program_to_transaction_error(x: u8) -> impl Fn(ProgramError) -> TransactionError {
    move |e| {
        if let ProgramError::Custom(n) = e {
            TransactionError::InstructionError(x, InstructionError::Custom(n))
        } else {
            Err(e).unwrap()
        }
    }
}
#[derive(Debug)]
pub struct TestStakingPool {
    pub name: String,
    pub pubkey: Pubkey,
    pub staking_pool: StakingPool,
    pub staking_pool_owner: Keypair,
    pub staking_pool_admin: Keypair,
}

impl TestStakingPool {
    #[allow(clippy::too_many_arguments)]
    pub async fn init(
        name: String,
        banks_client: &mut BanksClient,

        reward_supply_pubkey: Pubkey,
        reward_supply_mint_pubkey: Pubkey,

        supply: u64,   // rate per slot = supply / duration
        duration: u64, // num of slots
        earliest_reward_claim_time: Slot,

        payer: &Keypair,
        supply_accounts_owner: &Keypair,

        sub_supply: Option<u64>,
        sub_reward_supply_pubkey: Option<Pubkey>,
        sub_reward_supply_mint_pubkey: Option<Pubkey>,
    ) -> Result<Self, TransactionError> {
        let staking_pool_keypair = Keypair::new();

        let staking_pool_owner_derived = Keypair::new();
        let staking_pool_admin = Keypair::new();
        let reward_pool_keypair = Keypair::new();
        let sub_reward_pool_keypair = Keypair::new();

        let rent = banks_client.get_rent().await.unwrap();
        let mut transaction = Transaction::new_with_payer(
            &[
                create_account(
                    &payer.pubkey(),
                    &reward_pool_keypair.pubkey(),
                    rent.minimum_balance(Token::LEN),
                    Token::LEN as u64,
                    &spl_token::id(),
                ),
                create_account(
                    &payer.pubkey(),
                    &sub_reward_pool_keypair.pubkey(),
                    rent.minimum_balance(Token::LEN),
                    Token::LEN as u64,
                    &spl_token::id(),
                ),
                create_account(
                    &payer.pubkey(),
                    &staking_pool_keypair.pubkey(),
                    rent.minimum_balance(StakingPool::LEN) + 100,
                    StakingPool::LEN as u64,
                    &port_finance_staking::id(),
                ),
                init_staking_pool(
                    port_finance_staking::id(),
                    supply,
                    sub_supply,
                    duration,
                    earliest_reward_claim_time,
                    supply_accounts_owner.pubkey(),
                    reward_supply_pubkey,
                    reward_pool_keypair.pubkey(),
                    sub_reward_supply_pubkey,
                    sub_supply.map(|_| sub_reward_pool_keypair.pubkey()),
                    staking_pool_keypair.pubkey(),
                    reward_supply_mint_pubkey,
                    sub_reward_supply_mint_pubkey,
                    staking_pool_owner_derived.pubkey(),
                    staking_pool_admin.pubkey(),
                ),
            ],
            Some(&payer.pubkey()),
        );
        let (_, bump_seed) = Pubkey::find_program_address(
            &[&staking_pool_keypair.pubkey().as_ref()],
            &port_finance_staking::id(),
        );
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(
            &vec![
                payer,
                supply_accounts_owner,
                &reward_pool_keypair,
                &staking_pool_keypair,
                &sub_reward_pool_keypair,
            ],
            recent_blockhash,
        );
        let mut staking_pool = StakingPool::default();

        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())?;

        staking_pool
            .init(
                staking_pool_owner_derived.pubkey(),
                staking_pool_admin.pubkey(),
                reward_pool_keypair.pubkey(),
                sub_supply.map(|_| sub_reward_pool_keypair.pubkey()),
                duration,
                supply,
                sub_supply,
                earliest_reward_claim_time,
                bump_seed,
            )
            .unwrap();

        Ok(Self {
            name,
            pubkey: staking_pool_keypair.pubkey(),
            staking_pool,
            staking_pool_owner: staking_pool_owner_derived,
            staking_pool_admin,
        })
    }

    pub async fn get_state(&self, banks_client: &mut BanksClient) -> StakingPool {
        let staking_pool: Account = banks_client
            .get_account(self.pubkey)
            .await
            .unwrap()
            .unwrap();
        StakingPool::unpack(&staking_pool.data[..]).unwrap()
    }

    pub async fn deposit(
        &mut self,
        banks_client: &mut BanksClient,
        amount: u64,
        slot: Slot,
        payer: &Keypair,
        authority: Option<&Keypair>,
        stake_account: Pubkey,
    ) -> Result<CumulativeRate, TransactionError> {
        let pool_owner = authority.unwrap_or(&self.staking_pool_owner);

        let mut transaction = Transaction::new_with_payer(
            &[deposit(
                port_finance_staking::id(),
                amount,
                pool_owner.pubkey(),
                stake_account,
                self.pubkey,
            )],
            Some(&payer.pubkey()),
        );

        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&vec![payer, pool_owner], recent_blockhash);
        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())
            .map(|_| self.staking_pool.deposit(slot, amount).unwrap())
    }

    pub async fn withdraw(
        &mut self,
        banks_client: &mut BanksClient,
        amount: u64,
        slot: Slot,
        payer: &Keypair,
        authority: Option<&Keypair>,
        stake_account: Pubkey,
    ) -> Result<CumulativeRate, TransactionError> {
        let pool_owner = authority.unwrap_or(&self.staking_pool_owner);

        let mut transaction = Transaction::new_with_payer(
            &[withdraw(
                port_finance_staking::id(),
                amount,
                pool_owner.pubkey(),
                stake_account,
                self.pubkey,
            )],
            Some(&payer.pubkey()),
        );

        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&vec![payer, pool_owner], recent_blockhash);
        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())
            .map(|_| self.staking_pool.withdraw(slot, amount).unwrap())
    }

    pub async fn add_sub_reward(
        &mut self,
        banks_client: &mut BanksClient,
        amount: u64,
        current_slot: Slot,
        sub_reward_token_source: Pubkey,
        payer: &Keypair,
    ) -> Result<(), TransactionError> {
        let sub_reward_token_pool = Keypair::new();
        let sub_reward_token_mint = spl_token::native_mint::id();
        let rent = banks_client.get_rent().await.unwrap();
        let mut transaction = Transaction::new_with_payer(
            &[
                create_account(
                    &payer.pubkey(),
                    &sub_reward_token_pool.pubkey(),
                    // Hack to make sure there is SOL to be rent exempt
                    rent.minimum_balance(Token::LEN) + 100,
                    Token::LEN as u64,
                    &spl_token::id(),
                ),
                add_sub_reward_pool(
                    port_finance_staking::id(),
                    amount,
                    self.staking_pool_admin.pubkey(),
                    self.staking_pool_admin.pubkey(),
                    sub_reward_token_source,
                    sub_reward_token_mint,
                    self.pubkey,
                    sub_reward_token_pool.pubkey(),
                ),
            ],
            Some(&payer.pubkey()),
        );
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(
            &vec![payer, &self.staking_pool_admin, &sub_reward_token_pool],
            recent_blockhash,
        );
        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())
            .map(|_| {
                self.staking_pool
                    .add_sub_reward(amount, current_slot, sub_reward_token_pool.pubkey())
                    .unwrap()
            })
    }

    pub async fn claim_reward(
        &mut self,
        banks_client: &mut BanksClient,
        slot: Slot,
        payer: &Keypair,
        account_owner: &Keypair,
        stake_account: Pubkey,
        dest_reward: Pubkey,
        dest_sub_reward: Option<Pubkey>,
    ) -> Result<CumulativeRate, TransactionError> {
        let mut transaction = Transaction::new_with_payer(
            &[claim_reward(
                port_finance_staking::id(),
                account_owner.pubkey(),
                stake_account,
                self.pubkey,
                self.staking_pool.reward_token_pool,
                self.staking_pool.sub_reward_token_pool,
                dest_reward,
                dest_sub_reward,
            )],
            Some(&payer.pubkey()),
        );

        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&vec![payer, account_owner], recent_blockhash);
        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())
            .map(|_| self.staking_pool.claim_reward(slot).unwrap())
    }

    pub async fn change_owner(
        &mut self,
        banks_client: &mut BanksClient,
        new_owner: Pubkey,
        payer: &Keypair,
        correct_owner: bool,
    ) -> Result<(), TransactionError> {
        let tmp_keypair = Keypair::new();
        let current_owner = if correct_owner {
            &self.staking_pool_owner
        } else {
            &tmp_keypair
        };
        let mut transaction = Transaction::new_with_payer(
            &[change_owner(
                port_finance_staking::id(),
                new_owner,
                current_owner.pubkey(),
                self.pubkey,
            )],
            Some(&payer.pubkey()),
        );
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&[&payer, current_owner], recent_blockhash);

        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())
            .map(|_| self.staking_pool.owner_authority = new_owner)
    }

    pub async fn change_admin(
        &mut self,
        banks_client: &mut BanksClient,
        new_admin: Pubkey,
        payer: &Keypair,
        correct_admin: bool,
    ) -> Result<(), TransactionError> {
        let tmp_keypair = Keypair::new();
        let current_admin = if correct_admin {
            &self.staking_pool_admin
        } else {
            &tmp_keypair
        };
        let mut transaction = Transaction::new_with_payer(
            &[change_admin(
                port_finance_staking::id(),
                new_admin,
                current_admin.pubkey(),
                self.pubkey,
            )],
            Some(&payer.pubkey()),
        );
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&[&payer, current_admin], recent_blockhash);

        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())
            .map(|_| self.staking_pool.admin_authority = new_admin)
    }
    pub async fn change_duration(
        &mut self,
        banks_client: &mut BanksClient,
        amount: i64,
        payer: &Keypair,
        slot: Slot,
        correct_admin: bool,
    ) -> Result<(), TransactionError> {
        let tmp_keypair = Keypair::new();
        let current_admin = if correct_admin {
            &self.staking_pool_admin
        } else {
            &tmp_keypair
        };
        let mut transaction = Transaction::new_with_payer(
            &[change_duration(
                port_finance_staking::id(),
                amount,
                current_admin.pubkey(),
                self.pubkey,
            )],
            Some(&payer.pubkey()),
        );
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&[&payer, current_admin], recent_blockhash);

        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())
            .map(|_| self.staking_pool.extend_duration(amount, slot).unwrap())
    }

    pub async fn update_earliest_claim_time(
        &mut self,
        banks_client: &mut BanksClient,
        time: Slot,
        payer: &Keypair,
    ) -> Result<(), TransactionError> {
        let mut transaction = Transaction::new_with_payer(
            &[update_earliest_reward_claim_time(
                port_finance_staking::id(),
                time,
                self.staking_pool_admin.pubkey(),
                self.pubkey,
            )],
            Some(&payer.pubkey()),
        );

        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&[&payer, &self.staking_pool_admin], recent_blockhash);

        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())
            .map(|_| self.staking_pool.earliest_reward_claim_time = time)
    }

    pub async fn change_reward_supply(
        &mut self,
        banks_client: &mut BanksClient,
        amount: i64,
        sub_amount: Option<i64>,
        current_slot: Slot,
        reward_token_mint: Pubkey,
        sub_reward_token_mint: Option<Pubkey>,
        payer: &Keypair,
    ) -> Result<(), TransactionError> {
        let supply_accounts_owner = Keypair::new();
        let reward_supplier = create_and_mint_to_token_account(
            banks_client,
            reward_token_mint,
            None,
            &payer,
            supply_accounts_owner.pubkey(),
            abs(amount) as u64,
        )
        .await;

        let sub_reward_supplier = if let Some(amount) = sub_amount {
            Some(
                create_and_mint_to_token_account(
                    banks_client,
                    sub_reward_token_mint.unwrap(),
                    None,
                    &payer,
                    supply_accounts_owner.pubkey(),
                    abs(amount) as u64,
                )
                .await,
            )
        } else {
            None
        };

        let authority = if amount < 0 {
            &self.staking_pool_admin
        } else {
            &supply_accounts_owner
        };

        let mut transaction = Transaction::new_with_payer(
            &[change_reward_supply(
                port_finance_staking::id(),
                amount,
                sub_amount,
                authority.pubkey(),
                reward_supplier,
                reward_token_mint,
                self.pubkey,
                self.staking_pool.reward_token_pool,
                sub_reward_supplier,
                sub_reward_token_mint,
                self.staking_pool.sub_reward_token_pool,
            )],
            Some(&payer.pubkey()),
        );
        let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
        transaction.sign(&[payer, &authority], recent_blockhash);
        self.staking_pool
            .update_reward_supply(amount, sub_amount, current_slot)
            .unwrap_or(eprintln!("failed to change reward"));
        banks_client
            .process_transaction(transaction)
            .await
            .map_err(|e| e.unwrap())
    }

    pub async fn validate_state(&self, banks_client: &mut BanksClient) {
        let staking_pool = self.get_state(banks_client).await;
        assert_eq!(PROGRAM_VERSION, staking_pool.version);
        assert_eq!(self.staking_pool, staking_pool);
        assert!(staking_pool.last_update <= staking_pool.end_time);
        if self.staking_pool.end_time != 0u64 {
            let time_to_end = self
                .staking_pool
                .end_time
                .checked_sub(self.staking_pool.last_update)
                .unwrap();
            let amount = self
                .staking_pool
                .rate_per_slot
                .try_mul(time_to_end)
                .and_then(|d| {
                    m! {
                        reward <- d.reward.try_ceil_u64();
                        sub_reward <- d.sub_reward.map_or(Ok(None), |x| x.try_ceil_u64().map(Some));
                        Lift::lift((reward, sub_reward))
                    }
                })
                .unwrap();

            let reward_balance = banks_client
                .get_balance(self.staking_pool.reward_token_pool)
                .await
                .unwrap();
            assert!(amount.0 <= reward_balance);
        }
    }
}

pub struct TestMint {
    pub pubkey: Pubkey,
    pub authority: Keypair,
    pub decimals: u8,
}

pub fn add_usdc_mint(test: &mut ProgramTest) -> TestMint {
    let authority = Keypair::new();
    let pubkey = Pubkey::from_str(USDC_MINT).unwrap();
    let decimals = 6;
    test.add_packable_account(
        pubkey,
        u32::MAX as u64,
        &Mint {
            is_initialized: true,
            mint_authority: COption::Some(authority.pubkey()),
            decimals,
            ..Mint::default()
        },
        &spl_token::id(),
    );
    TestMint {
        pubkey,
        authority,
        decimals,
    }
}

pub async fn create_and_mint_to_token_account(
    banks_client: &mut BanksClient,
    mint_pubkey: Pubkey,
    mint_authority: Option<&Keypair>,
    payer: &Keypair,
    authority: Pubkey,
    amount: u64,
) -> Pubkey {
    if let Some(mint_authority) = mint_authority {
        let account_pubkey =
            create_token_account(banks_client, mint_pubkey, &payer, Some(authority), None).await;

        mint_to(
            banks_client,
            mint_pubkey,
            &payer,
            account_pubkey,
            mint_authority,
            amount,
        )
        .await;

        account_pubkey
    } else {
        create_token_account(
            banks_client,
            mint_pubkey,
            &payer,
            Some(authority),
            Some(amount),
        )
        .await
    }
}

pub async fn create_token_account(
    banks_client: &mut BanksClient,
    mint_pubkey: Pubkey,
    payer: &Keypair,
    authority: Option<Pubkey>,
    native_amount: Option<u64>,
) -> Pubkey {
    let token_keypair = Keypair::new();
    let token_pubkey = token_keypair.pubkey();
    let authority_pubkey = authority.unwrap_or_else(|| payer.pubkey());

    let rent = banks_client.get_rent().await.unwrap();
    let lamports = rent.minimum_balance(Token::LEN) + native_amount.unwrap_or_default();
    let mut transaction = Transaction::new_with_payer(
        &[
            create_account(
                &payer.pubkey(),
                &token_pubkey,
                lamports,
                Token::LEN as u64,
                &spl_token::id(),
            ),
            spl_token::instruction::initialize_account(
                &spl_token::id(),
                &token_pubkey,
                &mint_pubkey,
                &authority_pubkey,
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
    );

    let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
    transaction.sign(&[&payer, &token_keypair], recent_blockhash);

    assert_matches!(banks_client.process_transaction(transaction).await, Ok(()));

    token_pubkey
}

pub async fn mint_to(
    banks_client: &mut BanksClient,
    mint_pubkey: Pubkey,
    payer: &Keypair,
    account_pubkey: Pubkey,
    authority: &Keypair,
    amount: u64,
) {
    let mut transaction = Transaction::new_with_payer(
        &[spl_token::instruction::mint_to(
            &spl_token::id(),
            &mint_pubkey,
            &account_pubkey,
            &authority.pubkey(),
            &[],
            amount,
        )
        .unwrap()],
        Some(&payer.pubkey()),
    );

    let recent_blockhash = banks_client.get_recent_blockhash().await.unwrap();
    transaction.sign(&[payer, authority], recent_blockhash);

    assert_matches!(banks_client.process_transaction(transaction).await, Ok(()));
}

pub async fn get_token_balance(banks_client: &mut BanksClient, pubkey: Pubkey) -> u64 {
    let token: Account = banks_client.get_account(pubkey).await.unwrap().unwrap();

    spl_token::state::Account::unpack(&token.data[..])
        .unwrap()
        .amount
}
