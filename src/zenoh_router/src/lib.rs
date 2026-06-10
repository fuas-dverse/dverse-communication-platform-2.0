//! dverse Zenoh router, as an embeddable library.
//!
//! The headless core — [`state`], [`constants`], [`discovery`], [`router`],
//! [`logging`] — is always available so other binaries (e.g. the Tauri client
//! launcher) can run the router in-process via [`router::run`] over a shared
//! [`state::AppState`], instead of reimplementing a Zenoh client.
//!
//! The egui desktop GUI ([`gui`]) lives behind the `egui-gui` feature so the
//! library doesn't pull eframe/egui into embedders that supply their own UI.

pub mod admission_handler;
pub mod constants;
pub mod discovery;
pub mod logging;
pub mod router;
pub mod state;

#[cfg(feature = "egui-gui")]
pub mod gui;
