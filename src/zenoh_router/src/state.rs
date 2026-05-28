use bot_framework::config::{DverseConfig, SessionRole};

pub struct AppState {
    pub router_status: RouterStatus,
    /// CNs that have been auto-admitted (all valid-cert nodes).
    pub admitted: Vec<String>,
    /// Log lines shown in the GUI.
    pub log: Vec<String>,
    /// Config written by the GUI; background thread consumes it to start/restart.
    pub staged_config: Option<DverseConfig>,
    /// Whether this router hosts the session (Admin) or joined one (Client).
    /// Copied from the accepted config; the GUI reads it for the session badge.
    pub session_role: SessionRole,
    /// Cached `cfg.session_id()` — copied once at config-accept time so the
    /// GUI and DNS-SD threads don't re-derive it.  Empty string until a config
    /// has been accepted.
    pub session_id: String,
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

impl AppState {
    pub fn new(initial_config: Option<DverseConfig>) -> Self {
        let (session_role, session_id) = match &initial_config {
            Some(cfg) => (cfg.session_role.clone(), cfg.session_id()),
            None => (SessionRole::Admin, String::new()),
        };
        Self {
            router_status: RouterStatus::Idle,
            admitted: Vec::new(),
            log: Vec::new(),
            staged_config: initial_config,
            session_role,
            session_id,
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
