# Port Finance CLI

The latest program devnet on
`stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq.json`.

Devnet Sol Reserve: `6FeVStQAGPWvfWijDHF7cTWRCi7He6vTT3ubfNhe9SPt` 
Devnet USDC Reserve: `G1CcAWGhfxhHQaivC1Sh5CWVta6P4dc7a5BDSg9ERjV1`

// init staking pool
port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
init-staking-pool \
--authority ~/.config/solana/id.json \
--supply_pubkey 6RWm6C2KPSMQV7r71pm5vQ4kypKKbBgdxjtrtZ9gvfhN \
--mint PoRTjZMPXb9T7dyU7tpLEZRQj7e6ssfAE62j2oQuc6y \
--owner_authority EsQ179Q8ESroBnnmTDmWEV4rZLkRc3yck32PqMxypE5z \
--admin_authority J97XsFfGVkyi1uwy1wBnpJT9mB2KRbF8PZqnd3RihTbr \
--supply 1 \
--duration 5184000 \
--claim-time 0


// increase reward supply

port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-reward-supply \
--staking_pool 8bQ4csqn3fhSKYf5TsoK5djF2xpuGknNHGNXmoUhB4sz \
--source_token_owner ~/.config/solana/id.json \
--supply_change 13000000000000 \
--reward_token_mint fp42UaS3fXwees97JuHnqpr4SY5QfAQYyqELhr1TxuY \
--supply yZ38zxnVrEpp7iRn2x3YpjrfTM63yi9JV7S4vYwwbMs

// decrease reward supply

port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-reward-supply \
--staking_pool 8bQ4csqn3fhSKYf5TsoK5djF2xpuGknNHGNXmoUhB4sz \
--staking_pool_owner ~/.config/solana/id.json \
--supply_change -13000000000000 \
--reward_token_mint fp42UaS3fXwees97JuHnqpr4SY5QfAQYyqELhr1TxuY \
--supply yZ38zxnVrEpp7iRn2x3YpjrfTM63yi9JV7S4vYwwbMs

// change staking pool owner

port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-staking-pool-owner \
--staking_pool 5gy5zmTK4CCJspf7i8Dj1YM4MHjfQSFT853Pj4n4eVyC \
--old_staking_pool_owner ~/.config/solana/id.json \
--new_staking_pool_owner 8x2uay8UgrLiX8AAYyF6AkK9z91nNtN6aLwfqPkf6TAQ


PAI
staking pool FqfZ1ohCvaMqoiTzH45jxnWpUvf6CrKdVBhqPatcQA31
reward pool 4VPtTihuZjnGv8mrgWc3cVxt2GCyfGFrJVZKrnUEFkkH



port-lending-cli \
--program Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR \
--fee-payer ~/.config/solana/id.json \
update-reserve \
--reserve DSw99gXoGzvc4N7cNGU7TJ9bCWFq96NU2Cczi1TabDx2 \
--market 6T4XxKerq744sSuj3jaoV6QiZ8acirf4TrPwQzHAoSy5 \
--market-owner usb://ledger \
--deposit_staking_pool FqfZ1ohCvaMqoiTzH45jxnWpUvf6CrKdVBhqPatcQA31 \
--borrow_fee_wad 1000000000000000 \
--flash_loan_fee_wad 3000000000000000

port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-staking-pool-owner \
--staking_pool FLSf7beYjzVMEoahQfy9xZc5ubcrri1iSeNoAUhSZjym \
--old_staking_pool_owner ~/.config/solana/id.json \
--new_staking_pool_owner 8x2uay8UgrLiX8AAYyF6AkK9z91nNtN6aLwfqPkf6TAQ

port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-reward-supply \
--staking_pool FLSf7beYjzVMEoahQfy9xZc5ubcrri1iSeNoAUhSZjym \
--source_token_owner ~/.config/solana/id.json \
--supply_change 15000000000 \
--reward_token_mint PoRTjZMPXb9T7dyU7tpLEZRQj7e6ssfAE62j2oQuc6y \
--supply 6RWm6C2KPSMQV7r71pm5vQ4kypKKbBgdxjtrtZ9gvfhN

port-lending-cli \
--program Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR \
--fee-payer ~/.config/solana/id.json \
update-reserve \
--reserve GRJyCEezbZQibAEfBKCRAg5YoTPP2UcRSTC7RfzoMypy \
--market 6T4XxKerq744sSuj3jaoV6QiZ8acirf4TrPwQzHAoSy5 \
--market-owner usb://ledger \
--deposit_staking_pool 2ozGANLs2SeRhDJqPP1911atTEQjXqRfCQDmvBCUMLMq \
--borrow_fee_wad 1000000000000000 \
--flash_loan_fee_wad 3000000000000000

port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-staking-pool-owner \
--staking_pool 2ozGANLs2SeRhDJqPP1911atTEQjXqRfCQDmvBCUMLMq \
--old_staking_pool_owner ~/.config/solana/id.json \
--new_staking_pool_owner 8x2uay8UgrLiX8AAYyF6AkK9z91nNtN6aLwfqPkf6TAQ

port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-reward-supply \
--staking_pool 2ozGANLs2SeRhDJqPP1911atTEQjXqRfCQDmvBCUMLMq \
--source_token_owner ~/.config/solana/id.json \
--supply_change 15000000000 \
--reward_token_mint PoRTjZMPXb9T7dyU7tpLEZRQj7e6ssfAE62j2oQuc6y \
--supply 6RWm6C2KPSMQV7r71pm5vQ4kypKKbBgdxjtrtZ9gvfhN
