// Copyright (c) 2026 Hamza Gouda
// SPDX-License-Identifier: MIT
// Agent budget and execution guard contract for the Terminal3 TEE runtime

wit_bindgen::generate!({
    world: "agent-guard",
    path: "wit",
});

use serde::{Deserialize, Serialize};
use terminal3::agent_guard::kv_store;
use terminal3::agent_guard::logging;

struct AgentGuard;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct StoredBudgetRecord {
    agent_did: String,
    max_tokens_per_epoch: u64,
    max_usd_micro_per_epoch: u64,
    epoch_duration_sec: u64,
    authorized_caller: String,
    tokens_consumed: u64,
    usd_micro_consumed: u64,
    epoch_start_timestamp: u64,
    epoch_end_timestamp: u64,
    is_active: bool,
    audit_counter: u64,
}

impl Guest for AgentGuard {
    fn set_budget(config: BudgetConfig) -> Result<bool, String> {
        let key = format!("agent_guard:budget:{}", config.agent_did);
        logging::log_info(&format!(
            "[TEE Guard] Initializing hardware budget for Agent DID: {}",
            config.agent_did
        ));

        let current_time = 1773720000u64; // Deterministic initial epoch timestamp in TEE enclave
        let end_time = current_time.saturating_add(config.epoch_duration_sec);

        let record = StoredBudgetRecord {
            agent_did: config.agent_did.clone(),
            max_tokens_per_epoch: config.max_tokens_per_epoch,
            max_usd_micro_per_epoch: config.max_usd_micro_per_epoch,
            epoch_duration_sec: config.epoch_duration_sec,
            authorized_caller: config.authorized_caller,
            tokens_consumed: 0,
            usd_micro_consumed: 0,
            epoch_start_timestamp: current_time,
            epoch_end_timestamp: end_time,
            is_active: true,
            audit_counter: 0,
        };

        let serialized = match serde_json::to_string(&record) {
            Ok(s) => s,
            Err(e) => return Err(format!("Serialization error: {}", e)),
        };

        match kv_store::set(&key, &serialized) {
            Ok(_) => {
                logging::log_info(&format!(
                    "[TEE Guard] Budget successfully committed into secure enclave storage for {}",
                    config.agent_did
                ));
                Ok(true)
            }
            Err(e) => Err(format!("Secure KV store write failure: {}", e)),
        }
    }

    fn consume_budget(req: ConsumptionRequest) -> ConsumptionResult {
        let key = format!("agent_guard:budget:{}", req.agent_did);
        logging::log_info(&format!(
            "[TEE Guard] Intercepting execution request for Agent: {} from Caller: {}",
            req.agent_did, req.caller_did
        ));

        let raw_val = match kv_store::get(&key) {
            Some(val) => val,
            None => {
                logging::log_warn(&format!(
                    "[TEE Guard REJECT] No budget registered for DID {}",
                    req.agent_did
                ));
                return ConsumptionResult {
                    approved: false,
                    remaining_tokens: 0,
                    remaining_usd_micro: 0,
                    rejection_reason: Some("No budget configured for target Agent DID".to_string()),
                    audit_log_id: "ERR_UNREGISTERED_AGENT".to_string(),
                };
            }
        };

        let mut record: StoredBudgetRecord = match serde_json::from_str(&raw_val) {
            Ok(r) => r,
            Err(e) => {
                logging::log_error(&format!(
                    "[TEE Guard CRITICAL] Corrupted budget record: {}",
                    e
                ));
                return ConsumptionResult {
                    approved: false,
                    remaining_tokens: 0,
                    remaining_usd_micro: 0,
                    rejection_reason: Some(format!("Data corruption: {}", e)),
                    audit_log_id: "ERR_DATA_CORRUPTION".to_string(),
                };
            }
        };

        // 1. Check if policy is active
        if !record.is_active {
            logging::log_warn(&format!(
                "[TEE Guard REJECT] Budget policy deactivated for {}",
                req.agent_did
            ));
            return ConsumptionResult {
                approved: false,
                remaining_tokens: record
                    .max_tokens_per_epoch
                    .saturating_sub(record.tokens_consumed),
                remaining_usd_micro: record
                    .max_usd_micro_per_epoch
                    .saturating_sub(record.usd_micro_consumed),
                rejection_reason: Some("Agent budget policy is suspended/inactive".to_string()),
                audit_log_id: format!("SUSPENDED_{}", record.audit_counter),
            };
        }

        // 2. Enforce Caller Authorization Guard
        if record.authorized_caller != "*" && record.authorized_caller != req.caller_did {
            logging::log_warn(&format!(
                "[TEE Guard REJECT] Unauthorized caller {} attempted to consume budget for {}",
                req.caller_did, req.agent_did
            ));
            return ConsumptionResult {
                approved: false,
                remaining_tokens: record
                    .max_tokens_per_epoch
                    .saturating_sub(record.tokens_consumed),
                remaining_usd_micro: record
                    .max_usd_micro_per_epoch
                    .saturating_sub(record.usd_micro_consumed),
                rejection_reason: Some(format!(
                    "Unauthorized caller: '{}' does not match mandated controller '{}'",
                    req.caller_did, record.authorized_caller
                )),
                audit_log_id: format!("UNAUTH_CALLER_{}", record.audit_counter),
            };
        }

        // 3. Hardware-Enforced Token Ceiling Check
        let new_tokens = match record.tokens_consumed.checked_add(req.tokens_requested) {
            Some(val) => val,
            None => {
                return ConsumptionResult {
                    approved: false,
                    remaining_tokens: 0,
                    remaining_usd_micro: 0,
                    rejection_reason: Some("Integer overflow in token computation".to_string()),
                    audit_log_id: "OVERFLOW_TOKENS".to_string(),
                };
            }
        };

        if new_tokens > record.max_tokens_per_epoch {
            let available = record
                .max_tokens_per_epoch
                .saturating_sub(record.tokens_consumed);
            logging::log_warn(&format!(
                "[TEE Guard REJECT] Token limit breach! Requested: {}, Available: {}",
                req.tokens_requested, available
            ));
            return ConsumptionResult {
                approved: false,
                remaining_tokens: available,
                remaining_usd_micro: record.max_usd_micro_per_epoch.saturating_sub(record.usd_micro_consumed),
                rejection_reason: Some(format!(
                    "Token budget exceeded: requested {} tokens, but only {} remaining in current epoch",
                    req.tokens_requested, available
                )),
                audit_log_id: format!("EXCEEDED_TOKENS_{}", record.audit_counter),
            };
        }

        // 4. Hardware-Enforced Financial Micro-USD Ceiling Check
        let new_usd = match record
            .usd_micro_consumed
            .checked_add(req.usd_micro_requested)
        {
            Some(val) => val,
            None => {
                return ConsumptionResult {
                    approved: false,
                    remaining_tokens: 0,
                    remaining_usd_micro: 0,
                    rejection_reason: Some("Integer overflow in USD calculation".to_string()),
                    audit_log_id: "OVERFLOW_USD".to_string(),
                };
            }
        };

        if new_usd > record.max_usd_micro_per_epoch {
            let available_usd = record
                .max_usd_micro_per_epoch
                .saturating_sub(record.usd_micro_consumed);
            logging::log_warn(&format!(
                "[TEE Guard REJECT] Financial limit breach! Requested: {} uUSD, Available: {} uUSD",
                req.usd_micro_requested, available_usd
            ));
            return ConsumptionResult {
                approved: false,
                remaining_tokens: record.max_tokens_per_epoch.saturating_sub(record.tokens_consumed),
                remaining_usd_micro: available_usd,
                rejection_reason: Some(format!(
                    "Financial credit ceiling exceeded: requested {} micro-USD, but only {} available",
                    req.usd_micro_requested, available_usd
                )),
                audit_log_id: format!("EXCEEDED_USD_{}", record.audit_counter),
            };
        }

        // Commit state mutation inside TEE boundary
        record.tokens_consumed = new_tokens;
        record.usd_micro_consumed = new_usd;
        record.audit_counter += 1;

        let remaining_tokens = record
            .max_tokens_per_epoch
            .saturating_sub(record.tokens_consumed);
        let remaining_usd_micro = record
            .max_usd_micro_per_epoch
            .saturating_sub(record.usd_micro_consumed);
        let audit_log_id = format!(
            "TEE_TX_{}_{:08x}",
            record.agent_did.chars().take(8).collect::<String>(),
            record.audit_counter
        );

        if let Ok(serialized) = serde_json::to_string(&record) {
            let _ = kv_store::set(&key, &serialized);
        }

        logging::log_info(&format!(
            "[TEE Guard APPROVED] Tx: {} | Consumed {} tokens, {} uUSD | Remaining: {} tokens, {} uUSD",
            audit_log_id, req.tokens_requested, req.usd_micro_requested, remaining_tokens, remaining_usd_micro
        ));

        ConsumptionResult {
            approved: true,
            remaining_tokens,
            remaining_usd_micro,
            rejection_reason: None,
            audit_log_id,
        }
    }

    fn get_budget(agent_did: String) -> Option<BudgetStatus> {
        let key = format!("agent_guard:budget:{}", agent_did);
        let raw_val = kv_store::get(&key)?;
        let record: StoredBudgetRecord = serde_json::from_str(&raw_val).ok()?;

        Some(BudgetStatus {
            agent_did: record.agent_did,
            max_tokens: record.max_tokens_per_epoch,
            max_usd_micro: record.max_usd_micro_per_epoch,
            tokens_consumed: record.tokens_consumed,
            usd_micro_consumed: record.usd_micro_consumed,
            remaining_tokens: record
                .max_tokens_per_epoch
                .saturating_sub(record.tokens_consumed),
            remaining_usd_micro: record
                .max_usd_micro_per_epoch
                .saturating_sub(record.usd_micro_consumed),
            epoch_start_timestamp: record.epoch_start_timestamp,
            epoch_end_timestamp: record.epoch_end_timestamp,
            is_active: record.is_active,
        })
    }
}

export!(AgentGuard);
