#!/bin/bash
export BPF_FILE=target/deploy/port_finance_variable_rate_lending.so
echo ---------------------
echo solana program WRITE-BUFFER
echo ---------------------
solana program write-buffer -v -u https://marinade.rpcpool.com $BPF_FILE > temp/buffer.txt
export BUFFER_ADDRESS=$(cat temp/buffer.txt | sed -n 's/.*Buffer://p')
echo solana program show "$BUFFER_ADDRESS"
solana program set-buffer-authority --new-buffer-authority J97XsFfGVkyi1uwy1wBnpJT9mB2KRbF8PZqnd3RihTbr "$BUFFER_ADDRESS"
echo use https://aankor.github.io/multisig-ui/#/magrsHFQxkkioAy45VWnZnFBBdKVdy2ZiRoRGYT9Wed to create mulisig-upgrade txn
echo program_id: MarBmsSgKXdrN1egZf5sqe1TMai9K1rChYNDJgjq7aD
echo buffer: "$BUFFER_ADDRESS"