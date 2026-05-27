use bot_framework::config::DverseConfig;

pub struct AppState {
    pub router_status: RouterStatus,
    /// CNs that have been auto-admitted (all valid-cert nodes).
    pub admitted: Vec<String>,
    /// Log lines shown in the GUI.
    pub log: Vec<String>,
    /// Config written by the GUI; background thread consumes it to start/restart.
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

impl AppState {
    pub fn new(initial_config: Option<DverseConfig>) -> Self {
        Self {
            router_status: RouterStatus::Idle,
            admitted: Vec::new(),
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
