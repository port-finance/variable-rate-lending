#!/bin/bash
port-staking-cli \
--program stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq \
--fee-payer    ~/.config/solana/id.json \
add-sub-reward \
--pool "$1" \
--admin_authority usb://ledger \
--supply "$2" \
--supply_pubkey "$3" \
--mint "$4" \
--transfer_authority "$5"
