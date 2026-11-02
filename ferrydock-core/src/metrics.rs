use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderMetrics {
    pub id: String,
    pub total: u64,
    pub success: u64,
    pub error: u64,
    pub latency_ms: f64,
    pub last_error: Option<String>,
    pub last_success_ts: Option<u64>,
    pub healthy: bool,
    pub circuit_open: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Circuit {
    pub failures: u32,
    pub opened_at: Option<u64>,
    pub cooldown_ms: u64,
}

impl Circuit {
    pub fn new() -> Self {
        Circuit {
            failures: 0,
            opened_at: None,
            cooldown_ms: 30_000,
        }
    }
    /// Threshold of consecutive failures before the circuit opens.
    const THRESHOLD: u32 = 5;

    pub fn is_open(&self) -> bool {
        match self.opened_at {
            Some(ts) => now_ms().saturating_sub(ts) < self.cooldown_ms,
            None => false,
        }
    }

    pub fn register_failure(&mut self) {
        self.failures += 1;
        if self.failures >= Self::THRESHOLD {
            self.opened_at = Some(now_ms());
        }
    }

    pub fn register_success(&mut self) {
        self.failures = 0;
        self.opened_at = None;
    }
}

pub struct MetricsStore {
    inner: Mutex<HashMap<String, ProviderMetrics>>,
}

impl MetricsStore {
    pub fn new() -> Self {
        MetricsStore {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn blank(id: &str) -> ProviderMetrics {
        ProviderMetrics {
            id: id.to_string(),
            total: 0,
            success: 0,
            error: 0,
            latency_ms: 0.0,
            last_error: None,
            last_success_ts: None,
            healthy: true,
            circuit_open: false,
        }
    }

    pub fn ensure(&self, id: &str) {
        let mut m = self.inner.lock().unwrap();
        m.entry(id.to_string()).or_insert_with(|| Self::blank(id));
    }

    pub fn record_success(&self, id: &str, latency_ms: f64) {
        let mut m = self.inner.lock().unwrap();
        let e = m.entry(id.to_string()).or_insert_with(|| Self::blank(id));
        e.total += 1;
        e.success += 1;
        // Exponential moving average of latency.
        e.latency_ms = if e.latency_ms == 0.0 {
            latency_ms
        } else {
            e.latency_ms * 0.8 + latency_ms * 0.2
        };
        e.last_error = None;
        e.last_success_ts = Some(now_ms());
        e.healthy = true;
        e.circuit_open = false;
    }

    pub fn record_error(&self, id: &str, err: &str) {
        let mut m = self.inner.lock().unwrap();
        let e = m.entry(id.to_string()).or_insert_with(|| Self::blank(id));
        e.total += 1;
        e.error += 1;
        e.last_error = Some(err.to_string());
        e.healthy = false;
    }

    pub fn set_circuit(&self, id: &str, open: bool) {
        let mut m = self.inner.lock().unwrap();
        let e = m.entry(id.to_string()).or_insert_with(|| Self::blank(id));
        e.circuit_open = open;
        if open {
            e.healthy = false;
        }
    }

    pub fn mark_healthy(&self, id: &str, healthy: bool) {
        let mut m = self.inner.lock().unwrap();
        if let Some(e) = m.get_mut(id) {
            e.healthy = healthy;
        }
    }

    pub fn snapshot(&self) -> Vec<ProviderMetrics> {
        let m = self.inner.lock().unwrap();
        let mut v: Vec<ProviderMetrics> = m.values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }
}
