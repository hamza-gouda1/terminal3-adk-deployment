// Copyright (c) 2026 Hamza Gouda
// SPDX-License-Identifier: MIT
// Agent budget and execution guard contract for the Terminal3 TEE runtime

wit_bindgen::generate!({
    world: "agent-guard",
    path: "wit",
});

use serde::{Deserialize, Serialize};
#[cfg(not(test))]
use terminal3::agent_guard::kv_store;
#[cfg(not(test))]
use terminal3::agent_guard::logging;

// The documented T3N clock is system-only. Do not substitute caller input or
// WASI wall time: neither is a verified tenant clock in this deployment.
#[cfg(all(not(test), not(feature = "wasi-clock")))]
fn current_time() -> Result<u64, &'static str> {
    Err("Trusted runtime clock unavailable")
}

#[cfg(all(not(test), feature = "wasi-clock", target_os = "wasi"))]
fn current_time() -> Result<u64, &'static str> {
    #[link(wasm_import_module = "wasi_snapshot_preview1")]
    extern "C" {
        fn clock_time_get(id: u32, precision: u64, time: *mut u64) -> u16;
    }
    let mut nanos = 0;
    // WASI realtime clock, in nanoseconds since the Unix epoch.
    let status = unsafe { clock_time_get(0, 1, &mut nanos) };
    if status != 0 {
        return Err("Runtime clock unavailable");
    }
    Ok(nanos / 1_000_000_000)
}

#[cfg(all(not(test), feature = "wasi-clock", not(target_os = "wasi")))]
fn current_time() -> Result<u64, &'static str> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| "Runtime clock precedes Unix epoch")
}

#[cfg(test)]
fn current_time() -> Result<u64, &'static str> {
    TEST_TIME.with(|time| *time.borrow())
}

#[cfg(test)]
thread_local! {
    static TEST_TIME: std::cell::RefCell<Result<u64, &'static str>> = const { std::cell::RefCell::new(Err("Trusted runtime clock unavailable")) };
    static TEST_KV: std::cell::RefCell<std::collections::HashMap<String, String>> = std::cell::RefCell::new(std::collections::HashMap::new());
    static TEST_WRITE_FAIL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
mod kv_store {
    pub fn get(key: &str) -> Option<String> {
        super::TEST_KV.with(|kv| kv.borrow().get(key).cloned())
    }
    pub fn set(key: &str, value: &str) -> Result<(), String> {
        if super::TEST_WRITE_FAIL.with(|fail| fail.get()) {
            return Err("test write failure".into());
        }
        super::TEST_KV.with(|kv| {
            kv.borrow_mut().insert(key.into(), value.into());
        });
        Ok(())
    }
}
#[cfg(test)]
mod logging {
    pub fn log_info(_: &str) {}
    pub fn log_warn(_: &str) {}
    pub fn log_error(_: &str) {}
}

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

fn guard_unavailable(reason: &str) -> ConsumptionResult {
    ConsumptionResult {
        approved: false,
        remaining_tokens: 0,
        remaining_usd_micro: 0,
        rejection_reason: Some(reason.into()),
        audit_log_id: "ERR_GUARD_UNAVAILABLE".into(),
    }
}

impl Guest for AgentGuard {
    fn set_budget(config: BudgetConfig) -> Result<bool, String> {
        let key = format!("agent_guard:budget:{}", config.agent_did);
        logging::log_info(&format!(
            "[TEE Guard] Initializing hardware budget for Agent DID: {}",
            config.agent_did
        ));

        if config.epoch_duration_sec == 0 {
            return Err("Epoch duration must be greater than zero".into());
        }
        let current_time = current_time().map_err(str::to_string)?;
        let end_time = current_time
            .checked_add(config.epoch_duration_sec)
            .ok_or("Epoch end timestamp overflow")?;

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

        // Reset on a fixed epoch boundary, retaining the original schedule even
        // when several epochs pass between calls. Reject malformed/rolled-back time.
        let now = match current_time() {
            Ok(now) => now,
            Err(reason) => return guard_unavailable(reason),
        };
        if record.epoch_duration_sec == 0
            || record
                .epoch_start_timestamp
                .checked_add(record.epoch_duration_sec)
                != Some(record.epoch_end_timestamp)
            || now < record.epoch_start_timestamp
        {
            return guard_unavailable("Invalid epoch or runtime clock moved backwards");
        }
        if now >= record.epoch_end_timestamp {
            let start = now - (now - record.epoch_start_timestamp) % record.epoch_duration_sec;
            let Some(end) = start.checked_add(record.epoch_duration_sec) else {
                return guard_unavailable("Epoch timestamp overflow");
            };
            record.epoch_start_timestamp = start;
            record.epoch_end_timestamp = end;
            record.tokens_consumed = 0;
            record.usd_micro_consumed = 0;
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
        record.audit_counter = match record.audit_counter.checked_add(1) {
            Some(counter) => counter,
            None => return guard_unavailable("Audit counter overflow"),
        };

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

        let serialized = match serde_json::to_string(&record) {
            Ok(serialized) => serialized,
            Err(_) => return guard_unavailable("Budget serialization failed"),
        };
        if kv_store::set(&key, &serialized).is_err() {
            return guard_unavailable("Budget storage write failed");
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

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        TEST_KV.with(|kv| kv.borrow_mut().clear());
        TEST_WRITE_FAIL.with(|fail| fail.set(false));
        set_time(100);
    }
    fn set_time(time: u64) {
        TEST_TIME.with(|clock| *clock.borrow_mut() = Ok(time));
    }
    fn config(duration: u64) -> BudgetConfig {
        BudgetConfig {
            agent_did: "did:agent".into(),
            max_tokens_per_epoch: 10,
            max_usd_micro_per_epoch: 20,
            epoch_duration_sec: duration,
            authorized_caller: "did:owner".into(),
        }
    }
    fn consume(caller: &str, tokens: u64, usd: u64) -> ConsumptionResult {
        AgentGuard::consume_budget(ConsumptionRequest {
            agent_did: "did:agent".into(),
            caller_did: caller.into(),
            tokens_requested: tokens,
            usd_micro_requested: usd,
            operation_metadata: String::new(),
        })
    }
    #[test]
    fn rollover_limits_and_authorization_through_guest() {
        setup();
        assert!(AgentGuard::set_budget(config(10)).unwrap());
        assert!(!consume("attacker", 1, 1).approved);
        assert!(consume("did:owner", 10, 20).approved);
        assert!(!consume("did:owner", 1, 0).approved);
        set_time(109);
        assert!(!consume("did:owner", 0, 1).approved);
        set_time(110);
        assert!(!consume("attacker", 1, 1).approved);
        let status = AgentGuard::get_budget("did:agent".into()).unwrap();
        assert_eq!(status.tokens_consumed, 10); // unauthorized calls do not mutate
        let result = consume("did:owner", 2, 3);
        assert!(result.approved);
        assert_eq!(
            (result.remaining_tokens, result.remaining_usd_micro),
            (8, 17)
        );
        let status = AgentGuard::get_budget("did:agent".into()).unwrap();
        assert_eq!(
            (status.epoch_start_timestamp, status.epoch_end_timestamp),
            (110, 120)
        );
        set_time(145);
        assert!(consume("did:owner", 1, 1).approved);
        let status = AgentGuard::get_budget("did:agent".into()).unwrap();
        assert_eq!(
            (
                status.epoch_start_timestamp,
                status.epoch_end_timestamp,
                status.tokens_consumed
            ),
            (140, 150, 1)
        );
    }
    #[test]
    fn failures_do_not_approve_or_mutate() {
        setup();
        assert!(AgentGuard::set_budget(config(0)).is_err());
        assert!(!consume("did:owner", 1, 1).approved);
        AgentGuard::set_budget(config(10)).unwrap();
        assert!(!consume("did:owner", 0, 21).approved);
        TEST_TIME.with(|clock| *clock.borrow_mut() = Err("no clock"));
        assert!(!consume("did:owner", 1, 1).approved);
        set_time(99);
        assert!(!consume("did:owner", 1, 1).approved);
        set_time(110);
        TEST_WRITE_FAIL.with(|fail| fail.set(true));
        assert!(!consume("did:owner", 1, 1).approved);
        let status = AgentGuard::get_budget("did:agent".into()).unwrap();
        assert_eq!(
            (status.epoch_start_timestamp, status.tokens_consumed),
            (100, 0)
        );
        TEST_TIME.with(|clock| *clock.borrow_mut() = Err("no clock"));
        assert!(AgentGuard::set_budget(config(10)).is_err());
    }
}
