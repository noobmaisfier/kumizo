//! # wk-ipc
//!
//! UNIX domain socket IPC protocol, server, and client for Kumizo.
//!
//! Kumizo is single-instance: one process, one socket ([`socket_path`]).

pub mod client;
pub mod control;
pub mod protocol;
pub mod server;

pub use client::IpcClient;
pub use control::{WindowCmd, WindowControl, WindowSender};
pub use protocol::{socket_path, Request, Response};
pub use server::{IpcServer, RequestHandler};
