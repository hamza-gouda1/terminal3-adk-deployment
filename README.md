# 🛡️ Terminal3 (T3N) ADK Testnet Verification & Confidential TEE Agent Guard

[![Terminal3 Testnet](https://img.shields.io/badge/Network-Terminal3%20Testnet-blue.svg)](https://terminal3.io)
[![WASM WASI](https://img.shields.io/badge/Target-wasm32--wasip1-orange.svg)](https://bytecodealliance.org)
[![WIT Bindgen](https://img.shields.io/badge/WIT--Bindgen-v0.24.0-purple.svg)](https://github.com/bytecodealliance/wit-bindgen)
[![ci](https://github.com/hamza-gouda1/terminal3-adk-deployment/actions/workflows/ci.yml/badge.svg)](https://github.com/hamza-gouda1/terminal3-adk-deployment/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

> Terminal3 ADK testnet exploration and an experimental Rust/WASM budget guard prototype for the **Superteam Earn Terminal3 Bounty**.

**Status:** a submission for a Superteam Earn bounty (August 2026). The handshake and authentication
script was run against the Terminal3 testnet; the contract compiles to
`wasm32-wasip1`. It has not been audited and has not run on mainnet. Treat it as
a worked example, not a finished product.

**Quick start**

```bash
git clone https://github.com/hamza-gouda1/terminal3-adk-deployment.git
cd terminal3-adk-deployment
npm install && cp .env.example .env   # then fill in your own testnet credentials
npm run quickstart                    # testnet handshake and authentication
rustup target add wasm32-wasip1 && npm run build:contract
```

---

## 📑 Table of Contents
1. [Executive Summary](#executive-summary)
2. [Architecture & Hardware TEE Security Model](#architecture--hardware-tee-security-model)
3. [Repository Structure](#repository-structure)
4. [Prerequisites & Setup](#prerequisites--setup)
5. [Step-by-Step Reproduction Guide](#step-by-step-reproduction-guide)
6. [Rust TEE Contract Specification (`agent-guard`)](#rust-tee-contract-specification-agent-guard)
7. [Security Analysis & Capability Bounds](#security-analysis--capability-bounds)
8. [Developer Experience (DX) & Bug Findings Audit](#developer-experience-dx--bug-findings-audit)

---

## 🚀 Executive Summary

- **Builder**: Hamza Gouda
- **Authenticated Tenant DID**: `did:t3n:6ae883218ad096feffe76cb0f40b153c2deb5480`
- **Wallet Address**: Derived dynamically from secp256k1 key via `@terminal3/t3n-sdk`
- **Network**: Terminal3 (T3N) Testnet
- **Contract Architecture**: Rust (WASI 0.2 / `wasm32-wasip1`) compiled with `wit-bindgen 0.24.0` in opt-level `z` for confidential TEE execution.

The **Terminal3 Autonomous Decentralized Kernel (ADK)** work covers:
1. Cryptographic enclave handshake and DID-bound session authentication using `@terminal3/t3n-sdk`.
2. An experimental `agent-guard` contract intended for a confidential execution environment.
3. A budget accounting prototype; enforcement of external actions, authentication, and deployment behavior remain unverified.

---

## 🏛️ Architecture & Hardware TEE Security Model

```
+-------------------------------------------------------------------------------+
|                            Host Application / Client                          |
|  (Node.js ESM / TypeScript with @terminal3/t3n-sdk v0.1.x)                    |
+---------------------------------------+---------------------------------------+
                                        |
                 1. loadWasmComponent() | 2. Handshake & EIP-712 Auth
                                        v
+-------------------------------------------------------------------------------+
|                         Terminal3 Enclave Gateway                             |
|  - Remote Attestation Verification                                            |
|  - DID Session Nonce & Challenge Issuance                                     |
+---------------------------------------+---------------------------------------+
                                        |
                   Secure Channel (TLS) | Enclave Bytecode Dispatch
                                        v
+-------------------------------------------------------------------------------+
|                  Hardware Trusted Execution Environment (TEE)                 |
|             (Intel SGX / AMD SEV Confidential Compute Enclave)                |
|                                                                               |
|   +-----------------------------------------------------------------------+   |
|   |                       WASM Component Sandbox                          |   |
|   |   +---------------------------------------------------------------+   |   |
|   |   |            agent-budget-guard (wasm32-wasip1)                 |   |   |
|   |   |                                                               |   |   |
|   |   |   * set-budget(config)       -> Initialize accounting policy  |   |   |
|   |   |   * consume-budget(request)  -> Accounting limit check      |   |   |
|   |   |   * get-budget(agent_did)    -> Last committed state         |   |   |
|   |   +-------------------------------+-------------------------------+   |   |
|   |                                   | WIT Capability Imports            |   |
|   |                                   v                                   |   |
|   |               +-----------------------+-----------------------+       |   |
|   |               | terminal3:adk/kv-store| terminal3:adk/logging |       |   |
|   |               +-----------------------+-----------------------+       |   |
|   +-----------------------------------+-----------------------------------+   |
|                                       |                                       |
|                                       v                                       |
|                   Encrypted Enclave Memory / Sealed State                     |
+-------------------------------------------------------------------------------+
```

---

## 📂 Repository Structure

```
.
├── .env.example              # Template for API credentials and DID configuration
├── Cargo.toml                # Rust contract manifest (wasm32-wasip1, cdylib, opt-level=z)
├── package.json              # ESM Node.js manifest and npm scripts
├── scripts/
│   ├── quickstart.ts         # SDK testnet handshake and authentication check
│   └── register_contract.ts  # Loads the compiled contract and registers it
├── wit/
│   └── world.wit             # WIT interface definition for the agent-guard world
├── src/
│   └── lib.rs                # Rust contract source
└── docs/
    └── SUBMISSION_REPORT.md  # Bounty submission write-up and SDK developer-experience notes
```

---

## ⚙️ Prerequisites & Setup

### 1. Toolchains Required
- **Node.js**: `v20.x` or `v22.x+` (ESM module support)
- **Rust**: `1.80+` with `wasm32-wasip1` target
  ```bash
  rustup target add wasm32-wasip1
  ```
- **Cargo**: Included with Rust toolchain

### 2. Installation
Clone the repository and install dependencies:
```bash
git clone https://github.com/hamza-gouda1/terminal3-adk-deployment.git
cd terminal3-adk-deployment
npm install
```

### 3. Environment Configuration
Create a `.env` file from the template:
```bash
cp .env.example .env
```
Ensure your `.env` contains:
```env
T3N_API_KEY=0xYOUR_T3N_PRIVATE_KEY_HERE
T3N_DID=did:t3n:YOUR_TENANT_DID
T3N_ENVIRONMENT=testnet
```

---

## 🧪 Step-by-Step Reproduction Guide

### Step 1: Run Testnet Quickstart Handshake
Verify network connectivity, WASM runtime loading, and DID authentication:
```bash
npm run quickstart
# or
npx tsx scripts/quickstart.ts
```

**Expected Output Highlights:**
- `setEnvironment("testnet")` initialized.
- WASM component loaded into memory.
- `eth_get_address` derives controller address.
- `t3n.handshake()` completes with remote attestation payload.
- `t3n.authenticate()` produces valid tenant DID session.

### Step 2: Compile the Rust TEE Contract
Compile the contract to WASM targeted at WASI preview 1:
```bash
npm run build:contract
# or
cargo build --target wasm32-wasip1 --release
```
The compiled artifact is generated at:
`target/wasm32-wasip1/release/agent_budget_guard.wasm`

### Step 3: Register Contract & Simulate Guardrails
Execute the contract registration and interactive guardrail demonstration:
```bash
npm run register
# or
npx tsx scripts/register_contract.ts
```

---

## 🦀 Rust TEE Contract Specification (`agent-guard`)

The `agent-guard` contract is a budget accounting prototype, not an escrow or an independently enforced execution gateway. **Do not use it to protect real funds or production workloads.**

The default build fails closed on budget creation and authorized consumption because the [Terminal3 host API](https://docs.terminal3.io/t3n/how-t3n-works/host-api) documents `time` / `clock` as system-only and no tenant clock is established. The optional `wasi-clock` Cargo feature uses the WASI realtime clock for a separate host that explicitly supplies and trusts it; it is not evidence that Terminal3 tenants can use WASI clocks. A missing clock import can prevent module instantiation. Clock rollback within an epoch is not detectable from the existing stored record.

The WIT accepts a caller DID as ordinary input; it does not authenticate that identity, and `set-budget` itself has no authorization check. A host must authenticate and restrict calls, bind identities to requests, and serialize conflicting read-modify-write operations before this prototype can enforce a budget. The local tests include the compiled WASM running in Node with a controlled clock and in-memory host simulation; they do not exercise Terminal3 or a concurrent transaction runtime. `get-budget` reports the last committed epoch and is refreshed by a successful consumption after expiry; it is not a live rollover query. Existing stored records retain their format. The project WIT host imports have not been verified against the current Terminal3 host ABI.

### Run the budget tests

With Rust and Node.js 22 installed, run the native tests and exercise both compiled
WASM builds. The Node host supplies deterministic clock and storage imports so the
tests can cross epoch boundaries and simulate clock or write errors without an
account or network connection.

```sh
rustup target add wasm32-wasip1
cargo test --locked
cargo build --locked --release --target wasm32-wasip1
node tests/wasm-runtime.mjs target/wasm32-wasip1/release/agent_budget_guard.wasm --default
cargo build --locked --release --target wasm32-wasip1 --features wasi-clock
node tests/wasm-runtime.mjs
```

### WIT Interface (`wit/world.wit`)
```wit
package terminal3:agent-guard;

interface types {
    record budget-config {
        agent-did: string,
        max-tokens-per-epoch: u64,
        max-usd-micro-per-epoch: u64,
        epoch-duration-sec: u64,
        authorized-caller: string,
    }

    record budget-status {
        agent-did: string,
        max-tokens: u64,
        max-usd-micro: u64,
        tokens-consumed: u64,
        usd-micro-consumed: u64,
        remaining-tokens: u64,
        remaining-usd-micro: u64,
        epoch-start-timestamp: u64,
        epoch-end-timestamp: u64,
        is-active: bool,
    }

    record consumption-request {
        agent-did: string,
        caller-did: string,
        tokens-requested: u64,
        usd-micro-requested: u64,
        operation-metadata: string,
    }

    record consumption-result {
        approved: bool,
        remaining-tokens: u64,
        remaining-usd-micro: u64,
        rejection-reason: option<string>,
        audit-log-id: string,
    }
}

interface kv-store {
    get: func(key: string) -> option<string>;
    set: func(key: string, value: string) -> result<_, string>;
    delete: func(key: string) -> result<_, string>;
}

interface logging {
    log-info: func(msg: string);
    log-warn: func(msg: string);
    log-error: func(msg: string);
}

world agent-guard {
    import logging;
    import kv-store;

    export set-budget: func(config: types.budget-config) -> result<bool, string>;
    export consume-budget: func(req: types.consumption-request) -> types.consumption-result;
    export get-budget: func(agent-did: string) -> option<types.budget-status>;
}
```

---

## 🔒 Security Analysis & Capability Bounds

1. **Host capabilities**: The application uses the WIT `kv-store` and `logging` interfaces. The optional `wasi-clock` feature also reads the WASI realtime clock. Host isolation and import permissions are deployment requirements.
2. **Integer Arithmetic Safety**: All token and micro-USD balances utilize Rust `checked_add` and `saturating_sub` preventing integer overflow exploits.
3. **Design goal, prompt-injection resistance**: A separately integrated enforcement layer would need to intercept external actions and require a successful budget check.

---

## 🧑‍💻 Developer Experience (DX) & Bug Findings Audit

See [`docs/SUBMISSION_REPORT.md`](docs/SUBMISSION_REPORT.md) for full diagnostic breakdowns:
- **Bug 1**: WASM Bundling & Top-Level Await Hazards in modern bundlers (Next.js Turbopack / Vite).
- **Bug 2**: Ambiguity in Key Nomenclature (`T3N_API_KEY` vs secp256k1 private key).
- **Bug 3**: Silent Fallback when `setEnvironment` is omitted.
- **Bonus Architecture**: "Confidential AI Agent PII Redaction & Mandate Enforcer".

---

## 📜 License
MIT License. Copyright (c) 2026 Hamza Gouda.
