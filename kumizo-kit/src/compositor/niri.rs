use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Command;
use crate::compositor::types::{CompositorEvent, Workspace};

/// Niri scrollable tiling Wayland compositor IPC client.
pub struct NiriIpc;

impl NiriIpc {
    /// Detect if running under Niri.
    pub fn try_new() -> Option<Self> {
        let sock = std::env::var("NIRI_SOCKET").ok()?;
        let path = PathBuf::from(sock);
        if path.exists() {
            Some(Self)
        } else {
            None
        }
    }

    /// Switch workspace using `niri msg action focus-workspace <idx>`.
    pub fn switch_workspace(&self, idx: i32) -> anyhow::Result<()> {
        let _ = Command::new("niri")
            .args(["msg", "action", "focus-workspace", &idx.to_string()])
            .status()?;
        Ok(())
    }

    /// Fetch workspaces via `niri msg -j workspaces`.
    pub fn fetch_workspaces(&self) -> anyhow::Result<Vec<Workspace>> {
        let output = Command::new("niri")
            .args(["msg", "-j", "workspaces"])
            .output()?;

        if output.status.success() {
            #[derive(serde::Deserialize)]
            struct NiriWs {
                idx: i32,
                name: Option<String>,
                is_active: bool,
                output: Option<String>,
            }

            let items: Vec<NiriWs> = serde_json::from_slice(&output.stdout)?;
            let workspaces = items
                .into_iter()
                .map(|w| Workspace {
                    id: w.idx,
                    name: w.name.unwrap_or_else(|| w.idx.to_string()),
                    is_active: w.is_active,
                    is_urgent: false,
                    window_count: 1,
                    monitor: w.output.unwrap_or_else(|| "default".into()),
                })
                .collect();

            Ok(workspaces)
        } else {
            anyhow::bail!("Failed to query niri workspaces")
        }
    }

    /// Start listening for Niri event stream `niri msg -j event-stream`.
    pub fn start_event_loop(&self, tx: async_channel::Sender<CompositorEvent>) -> anyhow::Result<()> {
        std::thread::spawn(move || {
            use std::process::Stdio;
            if let Ok(mut child) = Command::new("niri")
                .args(["msg", "-j", "event-stream"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                if let Some(stdout) = child.stdout.take() {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        if let Ok(l) = line {
                            #[derive(serde::Deserialize)]
                            #[serde(untagged)]
                            enum NEvent {
                                WorkspaceActivated {
                                    #[serde(rename = "WorkspaceActivated")]
                                    info: WsAct,
                                },
                                // Catch-all arm for the untagged deserialization of every
                                // event kind we do not handle; its payload is never read.
                                Other(#[allow(dead_code)] serde_json::Value),
                            }
                            #[derive(serde::Deserialize)]
                            struct WsAct {
                                id: i32,
                            }

                            if let Ok(NEvent::WorkspaceActivated { info }) = serde_json::from_str::<NEvent>(&l) {
                                let _ = tx.send_blocking(CompositorEvent::WorkspaceActivated(info.id));
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }
}
