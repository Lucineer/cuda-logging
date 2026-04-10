/*!
# cuda-logging

Structured logging for agents.

Logs are how agents remember what happened. Structured logs with
context propagation let you trace decisions across time, agents,
and subsystems.

- Leveled logging (trace/debug/info/warn/error/fatal)
- Structured fields (key-value pairs)
- Context propagation (agent ID, request ID, span)
- Log buffering and rotation
- Log filtering by level, agent, tag
- Log search and export
*/

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Log level
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LogLevel { Trace, Debug, Info, Warn, Error, Fatal }

/// A structured log entry
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: u64,
    pub level: LogLevel,
    pub message: String,
    pub agent_id: Option<String>,
    pub request_id: Option<String>,
    pub span_id: Option<String>,
    pub fields: HashMap<String, String>,
    pub source: String,
}

impl LogEntry {
    pub fn new(level: LogLevel, message: &str) -> Self {
        LogEntry { timestamp: now(), level, message: message.to_string(), agent_id: None, request_id: None, span_id: None, fields: HashMap::new(), source: String::new() }
    }

    pub fn with_agent(mut self, id: &str) -> Self { self.agent_id = Some(id.to_string()); self }
    pub fn with_request(mut self, id: &str) -> Self { self.request_id = Some(id.to_string()); self }
    pub fn with_field(mut self, key: &str, val: &str) -> Self { self.fields.insert(key.to_string(), val.to_string()); self }
    pub fn with_source(mut self, src: &str) -> Self { self.source = src.to_string(); self }
}

/// Log rotation config
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotationConfig {
    pub max_entries: usize,
    pub max_bytes: usize,
}

impl Default for RotationConfig {
    fn default() -> Self { RotationConfig { max_entries: 10_000, max_bytes: 10 * 1024 * 1024 } }
}

/// A log buffer (in-memory)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogBuffer {
    pub entries: Vec<LogEntry>,
    pub config: RotationConfig,
    pub total_bytes: usize,
    pub dropped: u64,
    pub level_counts: HashMap<String, u64>,
}

impl LogBuffer {
    pub fn new() -> Self { LogBuffer { entries: Vec::new(), config: RotationConfig::default(), total_bytes: 0, dropped: 0, level_counts: HashMap::new() } }

    pub fn push(&mut self, entry: LogEntry) {
        let est_size = entry.message.len() + entry.fields.len() * 20;
        // Rotate if needed
        if self.entries.len() >= self.config.max_entries || self.total_bytes + est_size > self.config.max_bytes {
            let removed = self.entries.drain(0..self.entries.len() / 4);
            self.dropped += removed.len() as u64;
        }
        self.total_bytes += est_size;
        let level_name = format!("{:?}", entry.level);
        *self.level_counts.entry(level_name).or_insert(0) += 1;
        self.entries.push(entry);
    }

    /// Search logs
    pub fn search(&self, query: &str, limit: usize) -> Vec<&LogEntry> {
        self.entries.iter().rev().filter(|e| {
            e.message.contains(query) || e.fields.values().any(|v| v.contains(query)) || e.source.contains(query)
        }).take(limit).collect()
    }

    /// Filter by level
    pub fn by_level(&self, level: LogLevel) -> Vec<&LogEntry> {
        self.entries.iter().filter(|e| e.level == level).collect()
    }

    /// Filter by agent
    pub fn by_agent(&self, agent_id: &str) -> Vec<&LogEntry> {
        self.entries.iter().filter(|e| e.agent_id.as_deref() == Some(agent_id)).collect()
    }

    /// Filter by time range
    pub fn by_time_range(&self, start_ms: u64, end_ms: u64) -> Vec<&LogEntry> {
        self.entries.iter().filter(|e| e.timestamp >= start_ms && e.timestamp <= end_ms).collect()
    }

    pub fn len(&self) -> usize { self.entries.len() }

    /// Recent entries
    pub fn recent(&self, n: usize) -> Vec<&LogEntry> {
        self.entries.iter().rev().take(n).collect()
    }
}

/// The logging system
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Logger {
    pub buffer: LogBuffer,
    pub min_level: LogLevel,
    pub default_agent: Option<String>,
    pub default_request: Option<String>,
    pub total_logged: u64,
}

impl Logger {
    pub fn new() -> Self { Logger { buffer: LogBuffer::new(), min_level: LogLevel::Info, default_agent: None, default_request: None, total_logged: 0 } }

    fn log(&mut self, level: LogLevel, message: &str) {
        if level < self.min_level { return; }
        let mut entry = LogEntry::new(level, message);
        if let Some(ref agent) = self.default_agent { entry.agent_id = Some(agent.clone()); }
        if let Some(ref req) = self.default_request { entry.request_id = Some(req.clone()); }
        self.buffer.push(entry);
        self.total_logged += 1;
    }

    pub fn trace(&mut self, msg: &str) { self.log(LogLevel::Trace, msg); }
    pub fn debug(&mut self, msg: &str) { self.log(LogLevel::Debug, msg); }
    pub fn info(&mut self, msg: &str) { self.log(LogLevel::Info, msg); }
    pub fn warn(&mut self, msg: &str) { self.log(LogLevel::Warn, msg); }
    pub fn error(&mut self, msg: &str) { self.log(LogLevel::Error, msg); }
    pub fn fatal(&mut self, msg: &str) { self.log(LogLevel::Fatal, msg); }

    /// Log with structured fields
    pub fn info_f(&mut self, msg: &str, fields: &[(&str, &str)]) {
        if LogLevel::Info < self.min_level { return; }
        let mut entry = LogEntry::new(LogLevel::Info, msg);
        if let Some(ref agent) = self.default_agent { entry.agent_id = Some(agent.clone()); }
        for (k, v) in fields { entry.fields.insert(k.to_string(), v.to_string()); }
        self.buffer.push(entry);
        self.total_logged += 1;
    }

    /// Set context for subsequent logs
    pub fn set_context(&mut self, agent: &str, request: &str) {
        self.default_agent = Some(agent.to_string());
        self.default_request = Some(request.to_string());
    }

    /// Clear context
    pub fn clear_context(&mut self) { self.default_agent = None; self.default_request = None; }

    /// Export logs as plain text
    pub fn export_text(&self) -> String {
        self.buffer.entries.iter().map(|e| {
            let ctx = match (&e.agent_id, &e.request_id) {
                (Some(a), Some(r)) => format!("[{}/{}]", a, r),
                (Some(a), None) => format!("[{}]", a),
                _ => String::new(),
            };
            let fields_str: String = e.fields.iter().map(|(k,v)| format!("{}={}", k, v)).collect::<Vec<_>>().join(" ");
            let fields_suffix = if fields_str.is_empty() { String::new() } else { format!(" | {}", fields_str) };
            format!("[{}] {}{}{}", format!("{:?}", e.level).to_uppercase(), ctx, e.message, fields_suffix)
        }).collect::<Vec<_>>().join("\n")
    }

    /// Summary
    pub fn summary(&self) -> String {
        format!("Logger: {} entries ({} total, {} dropped), min_level={:?}, buffer={}",
            self.buffer.len(), self.total_logged, self.buffer.dropped, self.min_level, self.buffer.total_bytes)
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_logging() {
        let mut log = Logger::new();
        log.info("hello world");
        assert_eq!(log.buffer.len(), 1);
    }

    #[test]
    fn test_level_filtering() {
        let mut log = Logger::new();
        log.min_level = LogLevel::Warn;
        log.info("filtered");
        log.warn("visible");
        assert_eq!(log.buffer.len(), 1);
    }

    #[test]
    fn test_structured_fields() {
        let mut log = Logger::new();
        log.info_f("navigate", &[("lat", "12.3"), ("lon", "45.6")]);
        assert_eq!(log.buffer.entries[0].fields.len(), 2);
    }

    #[test]
    fn test_context_propagation() {
        let mut log = Logger::new();
        log.set_context("agent1", "req_123");
        log.info("with context");
        assert_eq!(log.buffer.entries[0].agent_id.as_deref(), Some("agent1"));
        assert_eq!(log.buffer.entries[0].request_id.as_deref(), Some("req_123"));
    }

    #[test]
    fn test_search() {
        let mut log = Logger::new();
        log.info("navigating to waypoint");
        log.info("checking sensors");
        log.info("navigation complete");
        let results = log.search("navigat", 10);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_filter_by_agent() {
        let mut log = Logger::new();
        log.set_context("a1", "r1"); log.info("msg1");
        log.set_context("a2", "r2"); log.info("msg2");
        log.set_context("a1", "r3"); log.info("msg3");
        assert_eq!(log.buffer.by_agent("a1").len(), 2);
    }

    #[test]
    fn test_rotation() {
        let mut log = Logger::new();
        log.buffer.config.max_entries = 10;
        for i in 0..20 { log.info(&format!("msg {}", i)); }
        assert!(log.buffer.len() <= 10);
        assert!(log.buffer.dropped > 0);
    }

    #[test]
    fn test_export() {
        let mut log = Logger::new();
        log.set_context("a1", "r1");
        log.info_f("move", &[("speed", "5")]);
        let text = log.export_text();
        assert!(text.contains("INFO"));
        assert!(text.contains("a1"));
        assert!(text.contains("speed=5"));
    }

    #[test]
    fn test_recent() {
        let mut log = Logger::new();
        for i in 0..10 { log.info(&format!("msg {}", i)); }
        let recent = log.buffer.recent(3);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn test_summary() {
        let log = Logger::new();
        let s = log.summary();
        assert!(s.contains("INFO"));
    }
}
