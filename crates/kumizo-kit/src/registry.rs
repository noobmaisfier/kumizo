use std::any::{Any, TypeId};
use gpui::{App, Context, Entity, Global};
use wk_services::Service;

/// Object-safe view over a registered service.
///
/// Services are stored as `Entity<S>` where `S` is only known at registration
/// time, so IPC actions are dispatched through this trait rather than by
/// downcasting at every call site.
trait ErasedService: Any {
    /// Upcast so a concrete `Entity<S>` can be recovered by `TypeId`.
    fn as_any(&self) -> &dyn Any;

    /// Apply an action requested through `widgetctl service <name> <action>`.
    ///
    /// Returns `true` when the action was recognised.
    fn ipc_action(&mut self, cx: &mut App, action: &str, payload: Option<&str>) -> bool;
}

impl<S: Service> ErasedService for Entity<S> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn ipc_action(&mut self, cx: &mut App, action: &str, payload: Option<&str>) -> bool {
        self.update(cx, |service, cx| {
            let handled = service.ipc_action(cx, action, payload);
            if handled {
                // Re-render whoever observes this service.
                cx.notify();
            }
            handled
        })
    }
}

struct ServiceEntry {
    type_id: TypeId,
    /// Short, lowercase name used on the CLI (`audio`, `mpris`, ...).
    short_name: String,
    service: Box<dyn ErasedService>,
}

/// Central registry storing instantiated `Entity<S>` services in GPUI's AppContext.
#[derive(Default)]
pub struct ServiceRegistry {
    services: Vec<ServiceEntry>,
}

impl Global for ServiceRegistry {}

impl ServiceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register an instantiated service entity into the global registry.
    pub fn register<S: Service>(&mut self, entity: Entity<S>) {
        self.services.push(ServiceEntry {
            type_id: TypeId::of::<S>(),
            short_name: short_service_name(std::any::type_name::<S>()),
            service: Box::new(entity),
        });
    }

    /// Retrieve an entity by type.
    pub fn get<S: Service>(&self) -> Option<Entity<S>> {
        // Registration is keyed by `TypeId` and `Entity<S>` is the only value
        // ever stored for it, so the downcast is sound. The registry holds a
        // handful of entries at most, so a linear scan is fine.
        self.services
            .iter()
            .find(|entry| entry.type_id == TypeId::of::<S>())
            .and_then(|entry| entry.service.as_any().downcast_ref::<Entity<S>>())
            .map(|entity| entity.clone())
    }

    /// List registered service names (short form, e.g. `audio`, `mpris`).
    pub fn list_services(&self) -> Vec<String> {
        self.services.iter().map(|e| e.short_name.clone()).collect()
    }

    /// Apply an IPC action to a registered service, addressed by short name.
    ///
    /// Returns `Ok(())` when both the service and the action were recognised,
    /// and an error describing the problem otherwise so the CLI can report it.
    pub fn dispatch(
        &mut self,
        cx: &mut App,
        service: &str,
        action: &str,
        payload: Option<&str>,
    ) -> Result<(), String> {
        let wanted = short_service_name(service);

        let Some(entry) = self
            .services
            .iter_mut()
            .find(|entry| entry.short_name == wanted)
        else {
            let known = self.list_services();
            return Err(format!(
                "unknown service '{}' (registered: {})",
                service,
                if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                }
            ));
        };

        if entry.service.ipc_action(cx, action, payload) {
            Ok(())
        } else {
            Err(format!(
                "service '{}' does not support action '{}'",
                service, action
            ))
        }
    }
}

/// Extension trait enabling seamless access to registered services from any GPUI Context.
pub trait ServiceContextExt<V: 'static> {
    /// Retrieve a registered background service entity.
    /// Automatically attaches an observer to re-render the view whenever the service notifies of changes.
    fn service<S: Service>(&mut self) -> Entity<S>;

    /// Try to retrieve a registered service without panicking if unregistered.
    fn try_service<S: Service>(&mut self) -> Option<Entity<S>>;
}

impl<V: 'static + gpui::Render> ServiceContextExt<V> for Context<'_, V> {
    fn service<S: Service>(&mut self) -> Entity<S> {
        self.try_service::<S>().unwrap_or_else(|| {
            panic!(
                "Service '{}' was requested but not registered on WidgetKit builder. Did you call .service::<{}>()?",
                std::any::type_name::<S>(),
                std::any::type_name::<S>()
            )
        })
    }

    fn try_service<S: Service>(&mut self) -> Option<Entity<S>> {
        let entity = self.try_global::<ServiceRegistry>()?.get::<S>()?;
        // Automatically observe the service entity so any service update triggers cx.notify() on this view
        self.observe(&entity, |_this, _entity, cx| {
            cx.notify();
        })
        .detach();
        Some(entity)
    }
}

/// Extension trait for `App` to read services without view auto-observing.
pub trait AppContextServiceExt {
    /// Read a registered service entity from the `App`.
    fn service<S: Service>(&self) -> Entity<S>;

    /// Try reading a service entity from the `App`.
    fn try_service<S: Service>(&self) -> Option<Entity<S>>;
}

impl AppContextServiceExt for App {
    fn service<S: Service>(&self) -> Entity<S> {
        self.try_service::<S>().unwrap_or_else(|| {
            panic!(
                "Service '{}' was requested but not registered on WidgetKit builder.",
                std::any::type_name::<S>()
            )
        })
    }

    fn try_service<S: Service>(&self) -> Option<Entity<S>> {
        self.try_global::<ServiceRegistry>()?.get::<S>()
    }
}

/// Turn a fully qualified type name into the short form used on the CLI.
///
/// `wk_services::audio::Audio` becomes `audio`, so `widgetctl service audio …`
/// works without repeating crate paths.
fn short_service_name(full: &str) -> String {
    full.rsplit("::").next().unwrap_or(full).to_ascii_lowercase()
}