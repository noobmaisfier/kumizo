//! # wk-ipc
//!
//! UNIX domain socket IPC protocol, server, and client for Kumizo.
//!
//! Kumizo is single-instance: one process, one socket ([`socket_path`]).

/// IPC client for sending requests to the running Kumizo shell.
pub mod client;
/// Window control commands sent over IPC.
pub mod control;
/// IPC request/response protocol types.
pub mod protocol;
/// UNIX domain socket IPC server.
pub mod server;

pub use client::IpcClient;
pub use control::{WindowCmd, WindowControl, WindowSender};
pub use protocol::{socket_path, Request, Response};
pub use server::{IpcServer, RequestHandler};
