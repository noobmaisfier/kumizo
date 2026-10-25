use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use crate::compositor::types::{ActiveWindow, CompositorEvent, Workspace};

/// Hyprland IPC client and event listener.
pub struct HyprlandIpc {
    instance_sig: String,
    runtime_dir: PathBuf,
}

impl HyprlandIpc {
    /// Detect and create a new Hyprland IPC instance if running under Hyprland.
    pub fn try_new() -> Option<Self> {
        let instance_sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
        let runtime = std::env::var("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/tmp"));

        let event_socket = runtime.join("hypr").join(&instance_sig).join(".socket2.sock");
        if event_socket.exists() {
            Some(Self {
                instance_sig,
                runtime_dir: runtime,
            })
        } else {
            None
        }
    }

    /// Path to command socket (.socket.sock).
    fn command_socket_path(&self) -> PathBuf {
        self.runtime_dir
            .join("hypr")
            .join(&self.instance_sig)
            .join(".socket.sock")
    }

    /// Path to event socket (.socket2.sock).
    fn event_socket_path(&self) -> PathBuf {
        self.runtime_dir
            .join("hypr")
            .join(&self.instance_sig)
            .join(".socket2.sock")
    }

    /// Dispatch a command to Hyprland socket and return raw response string.
    pub fn send_command(&self, cmd: &str) -> anyhow::Result<String> {
        let mut stream = UnixStream::connect(self.command_socket_path())?;
        stream.write_all(cmd.as_bytes())?;
        stream.write_all(b"\n")?;
        stream.flush()?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    }

    /// Switch active workspace.
    pub fn switch_workspace(&self, id: i32) -> anyhow::Result<()> {
        let cmd = format!("dispatch workspace {}", id);
        let _ = self.send_command(&cmd)?;
        Ok(())
    }

    /// Close active window.
    pub fn close_window(&self) -> anyhow::Result<()> {
        let _ = self.send_command("dispatch killactive")?;
        Ok(())
    }

    /// Fetch list of current workspaces via JSON command `j/workspaces`.
    pub fn fetch_workspaces(&self) -> anyhow::Result<Vec<Workspace>> {
        let json_str = self.send_command("j/workspaces")?;
        #[derive(serde::Deserialize)]
        struct Hw {
            id: i32,
            name: String,
            monitor: Option<String>,
            windows: Option<usize>,
        }

        let items: Vec<Hw> = serde_json::from_str(&json_str)?;
        let active_ws = self.fetch_active_workspace_id().unwrap_or(1);

        let workspaces = items
            .into_iter()
            .map(|w| Workspace {
                id: w.id,
                name: w.name,
                is_active: w.id == active_ws,
                is_urgent: false,
                window_count: w.windows.unwrap_or(0),
                monitor: w.monitor.unwrap_or_else(|| "default".into()),
            })
            .collect();

        Ok(workspaces)
    }

    /// Fetch active workspace ID via `j/activeworkspace`.
    pub fn fetch_active_workspace_id(&self) -> anyhow::Result<i32> {
        let json_str = self.send_command("j/activeworkspace")?;
        #[derive(serde::Deserialize)]
        struct Haw {
            id: i32,
        }
        let aw: Haw = serde_json::from_str(&json_str)?;
        Ok(aw.id)
    }

    /// Fetch active window metadata via `j/activewindow`.
    pub fn fetch_active_window(&self) -> anyhow::Result<Option<ActiveWindow>> {
        let json_str = self.send_command("j/activewindow")?;
        if json_str.trim() == "{}" || json_str.trim().is_empty() {
            return Ok(None);
        }

        #[derive(serde::Deserialize)]
        struct HawWindow {
            class: Option<String>,
            title: Option<String>,
            workspace: Option<HawWorkspace>,
        }
        #[derive(serde::Deserialize)]
        struct HawWorkspace {
            id: i32,
        }

        let w: HawWindow = serde_json::from_str(&json_str)?;
        Ok(Some(ActiveWindow {
            title: w.title.unwrap_or_default(),
            class: w.class.unwrap_or_default(),
            workspace_id: w.workspace.map(|ws| ws.id).unwrap_or(1),
        }))
    }

    /// Connect to `.socket2.sock` and stream events continuously.
    pub fn start_event_loop(
        &self,
        tx: async_channel::Sender<CompositorEvent>,
    ) -> anyhow::Result<()> {
        let sock_path = self.event_socket_path();
        let stream = UnixStream::connect(sock_path)?;
        let mut reader = BufReader::new(stream);

        std::thread::spawn(move || {
            let mut line = String::new();
            while let Ok(bytes) = reader.read_line(&mut line) {
                if bytes == 0 {
                    break;
                }

                let trimmed = line.trim();
                if let Some((event, args)) = trimmed.split_once(">>") {
                    match event {
                        "workspace" => {
                            if let Ok(id) = args.parse::<i32>() {
                                let _ = tx.send_blocking(CompositorEvent::WorkspaceActivated(id));
                            }
                        }
                        "activewindow" => {
                            let (class, title) = args.split_once(',').unwrap_or((args, ""));
                            let _ = tx.send_blocking(CompositorEvent::WindowFocused(ActiveWindow {
                                title: title.to_string(),
                                class: class.to_string(),
                                workspace_id: 0,
                            }));
                        }
                        "createworkspace" | "destroyworkspace" => {
                            // Trigger full sync
                            if let Ok(id) = args.parse::<i32>() {
                                let _ = tx.send_blocking(CompositorEvent::WorkspaceActivated(id));
                            }
                        }
                        _ => {}
                    }
                }
                line.clear();
            }
        });

        Ok(())
    }
}
