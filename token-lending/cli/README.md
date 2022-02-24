
# Port Finance CLI

The latest program is ran on `Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR`.


### Update Program Run Book
```bash
cargo build-bpf
solana program write-buffer <compiled_so_file_path>
solana program set-buffer-authority <buffer-pubkey> --new-buffer-authority <program_upgrade_authority>
solana program deploy --buffer <buffer-pubkey> --program-id <program-id-json> --keypair usb://ledger
```