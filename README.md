# 🛡️ Terminal3 (T3N) ADK Testnet Verification & Confidential TEE Agent Guard

[![Terminal3 Testnet](https://img.shields.io/badge/Network-Terminal3%20Testnet-blue.svg)](https://terminal3.io)
[![WASM WASI](https://img.shields.io/badge/Target-wasm32--wasip1-orange.svg)](https://bytecodealliance.org)
[![WIT Bindgen](https://img.shields.io/badge/WIT--Bindgen-v0.24.0-purple.svg)](https://github.com/bytecodealliance/wit-bindgen)
[![ci](https://github.com/hamza-gouda1/terminal3-adk-deployment/actions/workflows/ci.yml/badge.svg)](https://github.com/hamza-gouda1/terminal3-adk-deployment/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

> End-to-end Terminal3 ADK Testnet attestation, an AI-agent budget guard contract in Rust compiled to WASM/WASI, and the submission package for the **Superteam Earn Terminal3 Bounty**.

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

This repository demonstrates the full integration lifecycle with the **Terminal3 Autonomous Decentralized Kernel (ADK)**:
1. Cryptographic enclave handshake and DID-bound session authentication using `@terminal3/t3n-sdk`.
2. Hardware-enforced Confidential Smart Contract (`agent-guard`) executing within Intel SGX / AMD SEV trusted execution environments (TEE).
3. Provable autonomous agent guardrails that prevent prompt-injections, budget overruns, and unauthorized financial transactions at the hardware memory layer.

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
|   |   |   * set-budget(config)       -> Initialized hardware ceiling  |   |   |
|   |   |   * consume-budget(request)  -> Hardware guardrail check      |   |   |
|   |   |   * get-budget(agent_did)    -> Real-time state query         |   |   |
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

The `agent-guard` contract acts as an immutable hardware escrow and execution gateway for autonomous AI agents.

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

1. **Principle of Least Privilege**: The contract only imports `kv-store` and `logging`. It cannot make arbitrary network calls, read host filesystem paths, or leak enclave entropy.
2. **Integer Arithmetic Safety**: All token and micro-USD balances utilize Rust `checked_add` and `saturating_sub` preventing integer overflow exploits.
3. **Design goal, prompt-injection resistance**: LLMs or autonomous agents can hallucinate or be prompt-injected, but when they trigger an on-chain transaction or API invocation, the hardware enclave intercepts the call and rejects execution if limits are exceeded.

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
