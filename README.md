# TrustFlow

TrustFlow is a Soroban smart contract on Stellar that transforms social trust into a programmable funding primitive. Inspired by the **Federated Byzantine Agreement (FBA)** model in the Stellar Consensus Protocol, TrustFlow maps relationships between users as a trust graph where each node defines its own trusted peers. Funding is then distributed not just directly, but across trust proximity — enabling mechanisms like "fund everyone within two degrees of trust" or "prioritize contributors trusted by my network."

## Connection to the Stellar Whitepaper

The Stellar Consensus Protocol (SCP) is built on FBA, where:

- Each **node** independently declares a **quorum slice** — the set of peers it trusts.
- Agreement propagates through **overlapping slices**, not a central authority.
- Safety and liveness emerge from the structure of the trust graph itself.

TrustFlow applies this same model to capital allocation:

| SCP / FBA concept | TrustFlow equivalent |
|---|---|
| Node declares quorum slice | Address calls `set_trust(peers)` |
| Agreement propagates through overlapping slices | Funds flow via BFS over trust edges |
| Quorum intersection ensures safety | Bounded hop depth (max 5) prevents runaway distribution |
| Decentralized, no central coordinator | Any address can deposit and distribute independently |

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                  TrustFlow Contract                  │
│                                                      │
│  Trust Graph (persistent storage)                    │
│  ┌──────┐  trusts  ┌──────┐  trusts  ┌──────┐       │
│  │Alice │ ───────► │ Bob  │ ───────► │Carol │       │
│  └──────┘          └──────┘          └──────┘       │
│      │                                   ▲           │
│      └──────────── trusts ───────────────┘           │
│                                                      │
│  Deposits (persistent storage)                       │
│  Alice → 1000 XLM                                    │
│                                                      │
│  distribute(alice, token, 900, max_hops=2)           │
│  → BFS finds: Bob (hop1), Carol (hop2) = 2 nodes     │
│  → Each receives 450 XLM                             │
└─────────────────────────────────────────────────────┘
```

### Contract Functions

| Function | Description |
|---|---|
| `set_trust(caller, peers)` | Declare your quorum slice — the addresses you trust |
| `get_trust(node)` | Read the quorum slice for any address |
| `deposit(funder, token_id, amount)` | Deposit tokens into the contract for future distribution |
| `distribute(funder, token_id, amount, max_hops)` | Distribute funds to all addresses reachable within `max_hops` hops |
| `balance(funder)` | Check a funder's deposited balance |

### Storage

- `TrustSlice(Address)` → `Vec<Address>` — persistent, mirrors an FBA quorum slice declaration
- `Deposit(Address)` → `i128` — persistent, tracks deposited funds per funder

### Distribution Algorithm

`distribute` performs a **BFS traversal** of the trust graph starting from the funder, up to `max_hops` (1–5). All reachable addresses (excluding the funder) receive an equal share of the distributed amount. Remainder from integer division goes to the first recipient.

## Build

```bash
# Prerequisites: Rust + wasm32v1-none target + stellar-cli
curl -sSL https://sh.rustup.rs | sh
rustup target add wasm32v1-none
curl -sSL https://github.com/stellar/stellar-cli/releases/download/v26.0.0/stellar-cli-26.0.0-x86_64-unknown-linux-gnu.tar.gz | tar -xz -C ~/.local/bin/

# Build
stellar contract build
# Output: target/wasm32v1-none/release/trustflow.wasm (6757 bytes)
```

## Test

```bash
cargo test
# 6 tests: trust set/get, deposit, 1-hop distribute, 2-hop distribute,
#          overdraw panic, no-peers panic
```

## Deploy & Invoke (Testnet)

```bash
# Configure testnet identity
stellar keys generate alice --network testnet

# Deploy
stellar contract deploy \
  --wasm target/wasm32v1-none/release/trustflow.wasm \
  --source alice \
  --network testnet \
  --alias trustflow

# Set trust slice
stellar contract invoke --id trustflow --source alice --network testnet -- \
  set_trust --caller <ALICE_ADDRESS> --peers '[<BOB_ADDRESS>, <CAROL_ADDRESS>]'

# Deposit (requires prior token approval)
stellar contract invoke --id trustflow --source alice --network testnet -- \
  deposit --funder <ALICE_ADDRESS> --token_id <TOKEN_ADDRESS> --amount 1000

# Distribute to all peers within 2 hops
stellar contract invoke --id trustflow --source alice --network testnet -- \
  distribute --funder <ALICE_ADDRESS> --token_id <TOKEN_ADDRESS> --amount 1000 --max_hops 2
```

## Use Cases

- **Open-source funding**: Fund contributors trusted by your network, not just direct collaborators.
- **DAOs**: Allocate treasury funds proportionally across trust-weighted membership.
- **Public goods**: Route capital to builders within N degrees of a trusted seed set.

## WASM Artifact

| Property | Value |
|---|---|
| File | `target/wasm32v1-none/release/trustflow.wasm` |
| Size | 6757 bytes |
| Hash | `953f86c2fb99757c09d3c1be1fec968f8998d2fa0e21f7f574855733a6a5d889` |
| SDK | soroban-sdk 22.0.0 |
