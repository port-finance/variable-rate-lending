#!/bin/bash
port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-duration \
--pool "$1" \
--admin usb://ledger \
--amount "$2"

port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-reward-supply \
--staking_pool "$1" \
--reward_token_mint PoRTjZMPXb9T7dyU7tpLEZRQj7e6ssfAE62j2oQuc6y \
--source_token_owner ~/.config/solana/id.json \
--supply "$3" \
--supply_change "$4" \
--sub_reward_token_mint \
--sub_supply \
--sub_supply_change

port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
change-reward-supply \
--staking_pool GoNeG1rhoZMuLqy6aUNfzyHjkCe9dwFuWTFwENPfU3q8 \
--reward_token_mint PoRTjZMPXb9T7dyU7tpLEZRQj7e6ssfAE62j2oQuc6y \
--source_token_owner ~/.config/solana/id.json \
--supply 8Zc6QT2ZwKJCWkYpuytRBDso2W9fP6hw6gzaGnQiuUM8 \
--supply_change 1000000000 \
--sub_reward_token_mint MNDEFzGvMt87ueuHvVU9VcTqsAP5b3fTGPsHuuPA5ey \
--sub_supply GN1sLw6b6hnS6jkUDNYfiU9hS38T675omL9Kyn7zpBns \
--sub_supply_change 1000000000000