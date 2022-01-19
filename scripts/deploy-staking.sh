#!/bin/bash
export BPF_FILE=./target/deploy/port_finance_staking.so
echo ---------------------
echo solana program WRITE-BUFFER
echo ---------------------
solana program write-buffer -v $BPF_FILE > buffer.txt
export BUFFER_ADDRESS=$(cat buffer.txt | sed -n 's/.*Buffer://p')
echo solana program show "$BUFFER_ADDRESS"
solana program set-buffer-authority "$BUFFER_ADDRESS" --new-buffer-authority J97XsFfGVkyi1uwy1wBnpJT9mB2KRbF8PZqnd3RihTbr
echo program_id: stkarvwmSzv2BygN5e2LeTwimTczLWHCKPKGC2zVLiq
echo buffer: "$BUFFER_ADDRESS"