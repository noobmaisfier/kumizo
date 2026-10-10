//! Moteur audio : un processus `mpv` sans fenêtre, piloté par son socket IPC JSON.
//!
//! * Tout se passe sur des threads dédiés : l'UI n'attend jamais mpv.
//! * La vue lit un `Snapshot` (position, durée, pause, compteur de fins de piste).
//! * Si le shell plante, `PDEATHSIG` tue mpv : pas de processus orphelin.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

#[derive(Clone, Default)]
pub struct Snapshot {
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    /// Incrémenté à chaque fin NATURELLE de piste (pas lors d'un changement manuel).
    pub ended: u64,
    /// Message d'erreur affichable (mpv absent, socket introuvable…).
    pub error: Option<String>,
}

enum Cmd {
    Load(PathBuf),
    Pause(bool),
    Seek(f64),
    Volume(u8),
    Stop,
    Quit,
}

pub struct Engine {
    tx: mpsc::Sender<Cmd>,
    shared: Arc<Mutex<Snapshot>>,
}

impl Engine {
    /// Démarre le worker ; retourne immédiatement (mpv se lance en arrière-plan).
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let worker_shared = shared.clone();
        thread::Builder::new()
            .name("aurora-mpv".into())
            .spawn(move || worker(rx, worker_shared))
            .ok();
        Self { tx, shared }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.shared.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn load(&self, path: &Path) {
        // La pause est levée par l'évènement `file-loaded`.
        let _ = self.tx.send(Cmd::Load(path.to_path_buf()));
    }
    pub fn set_paused(&self, paused: bool) {
        let _ = self.tx.send(Cmd::Pause(paused));
    }
    pub fn seek(&self, secs: f64) {
        let _ = self.tx.send(Cmd::Seek(secs.max(0.0)));
    }
    pub fn set_volume(&self, pct: u8) {
        let _ = self.tx.send(Cmd::Volume(pct.min(130)));
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Cmd::Stop);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
    }
}

fn set_error(shared: &Arc<Mutex<Snapshot>>, msg: &str) {
    if let Ok(mut s) = shared.lock() {
        s.error = Some(msg.to_string());
    }
}

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("aurora-mpv-{}.sock", std::process::id()))
}

fn send(stream: &mut UnixStream, command: Value) -> bool {
    let mut line = json!({ "command": command }).to_string();
    line.push('\n');
    stream.write_all(line.as_bytes()).is_ok()
}

fn worker(rx: mpsc::Receiver<Cmd>, shared: Arc<Mutex<Snapshot>>) {
    let sock = socket_path();
    let _ = std::fs::remove_file(&sock);

    let mut cmd = Command::new("mpv");
    cmd.args([
        "--no-video",
        "--idle=yes",
        "--no-terminal",
        "--audio-display=no",
        "--force-window=no",
        "--really-quiet",
        "--load-scripts=no",
    ])
    .arg(format!("--input-ipc-server={}", sock.display()))
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    // SAFETY: appel async-signal-safe entre fork et exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(_) => {
            set_error(&shared, "mpv introuvable — installe le paquet « mpv »");
            return;
        }
    };

    // Le socket apparaît quelques dizaines de ms après le lancement.
    let mut stream = None;
    for _ in 0..75 {
        if let Ok(s) = UnixStream::connect(&sock) {
            stream = Some(s);
            break;
        }
        thread::sleep(Duration::from_millis(40));
    }
    let Some(mut stream) = stream else {
        set_error(&shared, "impossible de joindre mpv (socket IPC)");
        let _ = child.kill();
        let _ = child.wait();
        return;
    };

    // Lecteur d'évènements.
    if let Ok(read_half) = stream.try_clone() {
        let reader_shared = shared.clone();
        thread::Builder::new()
            .name("aurora-mpv-events".into())
            .spawn(move || {
                for line in BufReader::new(read_half).lines().map_while(Result::ok) {
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        handle_event(&v, &reader_shared);
                    }
                }
            })
            .ok();
    }

    for (id, prop) in [(1, "time-pos"), (2, "duration"), (3, "pause")] {
        send(&mut stream, json!(["observe_property", id, prop]));
    }
    if let Ok(mut s) = shared.lock() {
        s.paused = true;
    }

    while let Ok(c) = rx.recv() {
        let ok = match c {
            Cmd::Load(p) => send(
                &mut stream,
                json!(["loadfile", p.to_string_lossy(), "replace"]),
            ),
            Cmd::Pause(b) => send(&mut stream, json!(["set_property", "pause", b])),
            Cmd::Seek(t) => send(&mut stream, json!(["seek", t, "absolute"])),
            Cmd::Volume(v) => send(&mut stream, json!(["set_property", "volume", v])),
            Cmd::Stop => send(&mut stream, json!(["stop"])),
            Cmd::Quit => break,
        };
        if !ok {
            set_error(&shared, "connexion à mpv perdue");
            break;
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_file(&sock);
}

fn handle_event(v: &Value, shared: &Arc<Mutex<Snapshot>>) {
    let Some(event) = v.get("event").and_then(Value::as_str) else {
        return;
    };
    let Ok(mut s) = shared.lock() else { return };
    match event {
        "property-change" => {
            let data = v.get("data");
            match v.get("name").and_then(Value::as_str) {
                Some("time-pos") => s.position = data.and_then(Value::as_f64).unwrap_or(0.0),
                Some("duration") => s.duration = data.and_then(Value::as_f64).unwrap_or(0.0),
                Some("pause") => s.paused = data.and_then(Value::as_bool).unwrap_or(true),
                _ => {}
            }
        }
        "file-loaded" => {
            s.position = 0.0;
            s.paused = false;
        }
        "end-file" => {
            // « stop » / « redirect » = changement manuel : ne pas enchaîner.
            if v.get("reason").and_then(Value::as_str) == Some("eof") {
                s.ended += 1;
                s.position = 0.0;
            }
        }
        _ => {}
    }
}
