//! # wk-ipc
//!
//! UNIX domain socket IPC daemon protocol, server, and client for WidgetKit.

pub mod client;
pub mod control;
pub mod protocol;
pub mod server;

pub use client::IpcClient;
pub use control::{WindowCmd, WindowControl, WindowSender};
pub use protocol::{default_socket_path, discover_daemons, socket_path, Request, Response};
pub use server::{IpcServer, RequestHandler};
