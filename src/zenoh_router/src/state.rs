use std::collections::HashMap;
use std::time::Instant;

use bot_framework::config::DverseConfig;

pub struct AppState {
    pub router_status: RouterStatus,
    /// Nodes that announced themselves but are not yet admitted.
    pub pending: HashMap<String, Instant>,
    /// Admitted CNs — the ACL allowlist.
    pub admitted: Vec<String>,
    /// Denied CNs — permanently rejected, ignored on re-announce.
    pub denied: Vec<String>,
    /// Actions queued from the GUI for the background thread to process.
    pub action_queue: Vec<Action>,
    /// Log lines shown in the GUI.
    pub log: Vec<String>,
    /// Config written here by the GUI; the background thread consumes it to
    /// (re-)start the router.
    pub staged_config: Option<DverseConfig>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RouterStatus {
    Idle,
    Acquiring,
    Starting,
    Running,
    Reloading,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum Action {
    Admit(String),
    Deny(String),
}

impl AppState {
    pub fn new(pre_admitted: Vec<String>, initial_config: Option<DverseConfig>) -> Self {
        Self {
            router_status: RouterStatus::Idle,
            pending: HashMap::new(),
            admitted: pre_admitted,
            denied: Vec::new(),
            action_queue: Vec::new(),
            log: Vec::new(),
            staged_config: initial_config,
        }
    }

    pub fn push_log(&mut self, msg: impl Into<String>) {
        let entry = msg.into();
        eprintln!("{entry}");
        if self.log.len() >= 200 {
            self.log.remove(0);
        }
        self.log.push(entry);
    }
}
