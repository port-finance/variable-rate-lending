# Port Variable Rate Lending

Forked from [Solana Token Lending](https://github.com/solana-labs/solana-program-library/)

## Bug Bounty

![Logo black@4x (1)](https://user-images.githubusercontent.com/9982417/149652968-819cbc9e-06b7-41fe-b0d3-aa016843b570.png)

We have partnered with Immunefi to offer bug bounty up to 500K:
https://immunefi.com/bounty/portfinance/

## Development

### Environment Setup

1. Install the latest Rust stable from https://rustup.rs/
2. Install Solana v1.8.0 or later from https://docs.solana.com/cli/install-solana-cli-tools
3. Install the `libudev` development package for your distribution (`libudev-dev` on Debian-derived distros, `libudev-devel` on Redhat-derived).

### Build

The normal cargo build is available for building programs against your host machine:
```
$ cargo build
```

To build BPF Program:
```
$ cargo build-bpf
```

### Test

Unit tests contained within all projects can be run with:
```bash
$ cargo test      # <-- runs host-based tests
$ cargo test-bpf  # <-- runs BPF program tests
```


### Verify Build
```
anchor verify Port7uDYB3wk6GJAw4KT1WpTeMtSu9bTcChBHkX2LfR
```



