use std::sync::mpsc;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use gpui::{AnyWindowHandle, App, AppContext, Bounds, Entity, Pixels, QuitMode, Render};
use tracing::{info, warn};
use wk_gpui_ext::LayerWindow;
use wk_ipc::{IpcServer, Request, Response, WindowCmd, WindowControl, WindowSender};
use wk_services::Service;

use crate::registry::ServiceRegistry;

/// How long the IPC thread waits for the GPUI main thread to answer.
const IPC_REPLY_TIMEOUT: Duration = Duration::from_secs(5);

pub use wk_services::window::{WindowControlExt, WindowControlGlobal};

/// Trait abstracting different ways to build a root window view.
pub trait IntoWindowView<V: 'static + Render> {
    fn build(self, cx: &mut App) -> Entity<V>;
}

// Support `|cx| cx.new(TopBar::new)` (returns Entity<V>)
impl<V, F> IntoWindowView<V> for F
where
    V: 'static + Render,
    F: FnOnce(&mut App) -> Entity<V> + 'static,
{
    fn build(self, cx: &mut App) -> Entity<V> {
        self(cx)
    }
}

/// Opens the window and spawns its control task. Generic over `V` because a
/// window's view type is only known here, and the control task has to be
/// monomorphic over it to be able to re-create the surface.
type WindowInitFn = Box<dyn FnOnce(&mut App)>;
type ServiceInitFn = Box<dyn Fn(&mut App)>;

/// The primary builder and runtime runner for a WidgetKit desktop environment.
pub struct WidgetKit {
    services: Vec<ServiceInitFn>,
    service_names: Vec<String>,
    windows: Vec<WindowInitFn>,
    window_names: Vec<String>,
    window_registry: WindowControl,
    ipc_enabled: bool,
    start_time: Instant,
}

impl WidgetKit {
    /// Create a new WidgetKit application instance.
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
            service_names: Vec::new(),
            windows: Vec::new(),
            window_names: Vec::new(),
            window_registry: WindowControl::new(),
            ipc_enabled: true,
            start_time: Instant::now(),
        }
    }

    /// Register an asynchronous system service (e.g. `Clock`, `Battery`, `Audio`, `Mpris`, `Compositor`).
    ///
    /// The service will be instantiated during startup as an `Entity<S>` and
    /// made globally accessible to all views via `cx.service::<S>()`.
    pub fn service<S: Service>(mut self) -> Self {
        self.service_names.push(short_service_name(std::any::type_name::<S>()));
        self.services.push(Box::new(|cx: &mut App| {
            let entity = cx.new(S::init);
            // `default_global` creates the registry on demand, so services stay
            // order-independent w.r.t. the `set_global` call in `run`.
            cx.default_global::<ServiceRegistry>().register(entity);
        }));
        self
    }

    /// Register a Wayland Layer Shell window.
    ///
    /// Accepts a `LayerWindow` configuration and a view constructor function.
    pub fn window<V: 'static + Render>(
        mut self,
        config: LayerWindow,
        builder: impl IntoWindowView<V> + 'static,
    ) -> Self {
        let id = config.id.clone();
        let (tx, rx) = async_channel::bounded::<WindowCmd>(16);

        self.window_registry.register(id.clone(), tx);
        self.window_names.push(id.clone());

        let cfg = config.clone();
        self.windows.push(Box::new(move |cx: &mut App| {
            // Resolve the surface size against the target output: a layer-shell
            // surface is not stretched by the compositor, so a bar anchored to
            // LEFT|RIGHT has to request the full width itself.
            //
            // `App::primary_display()` returns `None` unconditionally on Wayland
            // (hardcoded in gpui_linux's wayland client), so the bounds have to
            // come from `displays()`, which is implemented there.
            let display_bounds = cx
                .displays()
                .first()
                .map(|display| display.bounds());
            let options = cfg.to_gpui_options_with_display(display_bounds);
            let entity = builder.build(cx);
            let root = entity.clone();

            match cx.open_window(options, move |_window, _cx| root) {
                Ok(handle) => {
                    // A popup (e.g. the tray menu) starts with no surface:
                    // it is created on demand by `show` and destroyed by
                    // `hide`, so it never reserves input area while closed.
                    let initial = if cfg.start_hidden {
                        let _ = cx.update_window(handle.into(), |_, window, _| {
                            window.remove_window();
                        });
                        None
                    } else {
                        Some(handle.into())
                    };
                    spawn_window_control::<V>(
                        cx,
                        id.clone(),
                        cfg.clone(),
                        entity,
                        initial,
                        display_bounds,
                        rx,
                    )
                }
                Err(err) => warn!("WidgetKit: failed to open window '{}': {:#}", id, err),
            }
        }));

        self
    }

    /// Enable or disable the UNIX domain socket IPC server (enabled by default).
    pub fn ipc(mut self, enabled: bool) -> Self {
        self.ipc_enabled = enabled;
        self
    }

    /// Launch the WidgetKit application and enter the GPUI event loop.
    ///
    /// This initializes all background services, binds the IPC server, opens
    /// configured LayerShell windows, and executes with Zero Idle CPU.
    pub fn run(mut self) {
        // Logs previously went nowhere: no subscriber was ever installed, so
        // every `warn!`/`info!` in the services was silently dropped (this
        // cost two blind diagnoses). Stderr lands in the daemon's log file.
        // `RUST_LOG=debug` raises the level; anything else stays at info.
        // (No EnvFilter on purpose: it would pull regex, unverified offline.)
        let level = match std::env::var("RUST_LOG").as_deref() {
            Ok("debug") | Ok("trace") => tracing::Level::DEBUG,
            Ok("warn") => tracing::Level::WARN,
            Ok("error") => tracing::Level::ERROR,
            _ => tracing::Level::INFO,
        };
        let _ = tracing_subscriber::fmt().with_max_level(level).try_init();
        let start_time = self.start_time;

        let socket_path = wk_ipc::socket_path();

        let window_names = self.window_names.clone();
        let service_names = self.service_names.clone();
        let window_registry = self.window_registry.clone();

        // Service actions are dispatched by a single generic task: services are
        // type-erased in the registry, so one task can serve them all.
        let (service_tx, service_rx) = async_channel::bounded::<WindowCmd>(16);

        // Bind before the event loop starts. `_ipc_server` must stay alive for
        // the whole run, which it does because `run` below never returns.
        let _ipc_server = if self.ipc_enabled {
            match IpcServer::bind(&socket_path, {
                let window_registry = window_registry.clone();
                move |req| handle_ipc_request(req, &window_registry, &service_tx, &start_time, &window_names, &service_names)
            }) {
                Ok(srv) => Some(srv),
                Err(err) => {
                    // Single instance: if one is alive, starting a second GPU
                    // stack would only duplicate every surface. Leave quietly.
                    if wk_ipc::IpcClient::new(&socket_path).is_alive() {
                        eprintln!("WidgetKit is already running ({}).", socket_path.display());
                        std::process::exit(1);
                    }
                    warn!("WidgetKit: failed to bind IPC server on {}: {:#}", socket_path.display(), err);
                    None
                }
            }
        } else {
            None
        };

        info!(
            "Starting WidgetKit desktop shell with {} window(s) and {} service(s)",
            self.windows.len(),
            self.services.len()
        );

        // `QuitMode::Explicit` is essential for a desktop shell: with the default
        // mode GPUI quits as soon as the last window closes, so `widgetctl hide`
        // on the only window would kill the daemon.
        gpui_platform::application()
            .with_quit_mode(QuitMode::Explicit)
            .run(move |cx: &mut App| {
                cx.set_global(ServiceRegistry::new());
                // In-process window control for services (tray menu, ...).
                // Set before services start so views can hand it to them.
                cx.set_global(WindowControlGlobal(window_registry.clone()));

                for init_service in self.services {
                    init_service(cx);
                }

                cx.spawn(async move |cx| {
                    service_loop(cx, service_rx).await;
                })
                .detach();

                // Windows are opened only once the platform reports an output:
                // on Wayland the `wl_output` globals arrive asynchronously, so
                // `primary_display()` is still `None` during this startup
                // closure and a bar anchored LEFT|RIGHT would silently fall back
                // to its default width.
                let windows = std::mem::take(&mut self.windows);
                cx.spawn(async move |cx| {
                    wait_for_displays(cx).await;
                    cx.update(|cx| {
                        for open_win in windows {
                            open_win(cx);
                        }
                    });
                })
                .detach();
            });
    }
}

/// Wait until the platform reports at least one output, or ~2s elapse.
///
/// Wayland delivers `wl_output` globals asynchronously, so the output list is
/// still empty during the startup closure. Bounded so a broken or headless
/// platform cannot hang startup; the caller then just gets unsized windows.
async fn wait_for_displays(cx: &mut gpui::AsyncApp) {
    for _ in 0..100 {
        if cx.update(|cx| !cx.displays().is_empty()) {
            return;
        }
        cx.background_executor()
            .timer(Duration::from_millis(20))
            .await;
    }
    warn!("WidgetKit: platform reported no output; windows may be mis-sized");
}

impl Default for WidgetKit {
    fn default() -> Self {
        Self::new()
    }
}

/// Main-thread task applying service actions requested over IPC.
async fn service_loop(cx: &mut gpui::AsyncApp, rx: Receiver<WindowCmd>) {
    while let Ok((req, reply)) = rx.recv().await {
        let response = match req {
            Request::ServiceAction {
                service,
                action,
                payload,
            } => cx.update(|cx| {
                // `dispatch` needs both `&mut ServiceRegistry` and `&mut App`, so
                // the registry is moved out for the duration of the call.
                let mut taken = std::mem::take(cx.global_mut::<ServiceRegistry>());
                let outcome = taken.dispatch(cx, &service, &action, payload.as_deref());
                *cx.global_mut::<ServiceRegistry>() = taken;

                match outcome {
                    Ok(()) => Response::Ok {
                        message: format!("action '{}' applied to service '{}'", action, service),
                    },
                    Err(message) => Response::Error { message },
                }
            }),
            Request::Reload => Response::Ok {
                message: "reload handled by the window tasks".into(),
            },
            other => Response::Error {
                message: format!("request '{}' is not a service action", describe(&other)),
            },
        };

        let _ = reply.send(response);
    }
}

/// Main-thread task owning one window's lifecycle so `show`/`hide`/`toggle`
/// actually do something.
fn spawn_window_control<V: 'static + Render>(
    cx: &mut App,
    window_name: String,
    config: LayerWindow,
    entity: Entity<V>,
    initial: Option<AnyWindowHandle>,
    display_bounds: Option<Bounds<Pixels>>,
    rx: Receiver<WindowCmd>,
) {
    // `None` means the window is currently destroyed/hidden.
    let mut current: Option<AnyWindowHandle> = initial;

    cx.spawn(async move |cx| {
        while let Ok((req, reply)) = rx.recv().await {
            let response = match req {
                Request::ShowWindow { name: requested } if requested == window_name => {
                    show_window(cx, &config, &entity, &mut current, display_bounds).await
                }
                Request::HideWindow { name: requested } if requested == window_name => {
                    hide_window(cx, &mut current).await
                }
                Request::ToggleWindow { name: requested } if requested == window_name => {
                    if current.is_some() {
                        hide_window(cx, &mut current).await
                    } else {
                        show_window(cx, &config, &entity, &mut current, display_bounds).await
                    }
                }
                Request::Reload => reload_window(cx, current).await,
                // Routed to the wrong window: drop it. The caller's reply
                // channel is never answered, so it times out rather than
                // reporting a bogus success.
                _ => continue,
            };

            let _ = reply.send(response);
        }
    })
    .detach();
}

async fn show_window<V: Render + 'static>(
    cx: &mut gpui::AsyncApp,
    config: &LayerWindow,
    entity: &Entity<V>,
    current: &mut Option<AnyWindowHandle>,
    display_bounds: Option<Bounds<Pixels>>,
) -> Response {
    // A handle can outlive its window: GPUI drops the window once
    // `remove_window` has been processed, but our `AnyWindowHandle` is just an
    // id. So verify against the live window list before trusting it, otherwise
    // `show` would silently "succeed" while nothing appears on screen.
    if let Some(handle) = *current {
        let alive = cx.update(|cx| cx.windows().contains(&handle));
        if alive {
            let _ = cx.update_window(handle, |_view, window, _cx| window.activate_window());
            return Response::Ok {
                message: format!("Window '{}' shown", config.id),
            };
        }
        // Stale handle: fall through and re-create the surface.
    }

    let options = config.to_gpui_options_with_display(display_bounds);
    let cloned = entity.clone();

    match cx.open_window(options, move |_window, _cx| cloned) {
        Ok(handle) => {
            *current = Some(handle.into());
            Response::Ok {
                message: format!("Window '{}' shown", config.id),
            }
        }
        Err(err) => Response::Error {
            message: format!("Failed to show window '{}': {:#}", config.id, err),
        },
    }
}

async fn hide_window(cx: &mut gpui::AsyncApp, current: &mut Option<AnyWindowHandle>) -> Response {
    let Some(handle) = current.take() else {
        return Response::Ok {
            message: "Window is already hidden".into(),
        };
    };

    match cx.update_window(handle, |_view, window, _cx| window.remove_window()) {
        Ok(()) => Response::Ok {
            message: "Window hidden".into(),
        },
        Err(err) => Response::Error {
            message: format!("Failed to hide window: {:#}", err),
        },
    }
}

async fn reload_window(
    cx: &mut gpui::AsyncApp,
    current: Option<AnyWindowHandle>,
) -> Response {
    let Some(handle) = current else {
        return Response::Ok {
            message: "Window is hidden; nothing to reload".into(),
        };
    };

    match cx.update_window(handle, |_view, window, _cx| window.refresh()) {
        Ok(()) => Response::Ok {
            message: "Window reloaded".into(),
        },
        Err(err) => Response::Error {
            message: format!("Failed to reload window: {:#}", err),
        },
    }
}

/// Route an IPC request to the right main-thread task and wait for its answer.
fn handle_ipc_request(
    req: Request,
    window_registry: &WindowControl,
    service_tx: &Sender<WindowCmd>,
    start_time: &Instant,
    window_names: &[String],
    service_names: &[String],
) -> Response {
    match req {
        Request::Ping => Response::Pong,

        Request::Status => {
            // Report live window visibility rather than the static registration list.
            let windows = window_registry.names();

            Response::Status {
                uptime_secs: start_time.elapsed().as_secs(),
                windows: if windows.is_empty() {
                    window_names.to_vec()
                } else {
                    windows
                },
                services: service_names.to_vec(),
            }
        }

        Request::ShowWindow { ref name } | Request::HideWindow { ref name } | Request::ToggleWindow { ref name } => {
            if (matches!(req, Request::HideWindow { .. }) || matches!(req, Request::ToggleWindow { .. })) && name == "lock" {
                return Response::Error {
                    message: "The lock window cannot be dismissed via IPC for security reasons".into(),
                };
            }

            let Some(tx) = window_registry.lookup(name) else {
                let known = window_registry.names();
                return Response::Error {
                    message: format!(
                        "unknown window '{}' (available: {})",
                        name,
                        if known.is_empty() { "none".into() } else { known.join(", ") }
                    ),
                };
            };

            forward(tx, req).unwrap_or_else(|message| Response::Error { message })
        }

        Request::Reload => {
            let senders = window_registry.senders();

            if senders.is_empty() {
                return Response::Error {
                    message: "no windows to reload".into(),
                };
            }

            let mut errors = Vec::new();
            for tx in senders {
                if let Err(err) = forward(tx, Request::Reload) {
                    errors.push(err);
                }
            }

            if errors.is_empty() {
                Response::Ok {
                    message: "Reload requested".into(),
                }
            } else {
                Response::Error {
                    message: errors.join("; "),
                }
            }
        }

        Request::ServiceAction { .. } => forward(service_tx.clone(), req)
                .unwrap_or_else(|message| Response::Error { message }),
    }
}

/// Hand a request to a main-thread task and block until it answers.
fn forward(tx: WindowSender, req: Request) -> Result<Response, String> {
    let (reply_tx, reply_rx) = mpsc::channel();

    tx.send_blocking((req, reply_tx))
        .map_err(|_| "the main thread task is no longer running".to_string())?;

    reply_rx
        .recv_timeout(IPC_REPLY_TIMEOUT)
        .map_err(|err| format!("timed out waiting for the GPUI main thread ({err})"))
}

/// Human readable request kind, for error messages.
fn describe(req: &Request) -> &'static str {
    match req {
        Request::Ping => "Ping",
        Request::Status => "Status",
        Request::ToggleWindow { .. } => "ToggleWindow",
        Request::ShowWindow { .. } => "ShowWindow",
        Request::HideWindow { .. } => "HideWindow",
        Request::Reload => "Reload",
        Request::ServiceAction { .. } => "ServiceAction",
    }
}

/// Turn a fully qualified type name into the short form used on the CLI.
fn short_service_name(full: &str) -> String {
    full.rsplit("::").next().unwrap_or(full).to_ascii_lowercase()
}