//! The app's view of the background service: the latest status, settings and
//! activity, refreshed every second and handed to whoever subscribed. Lives on
//! the GTK thread.

use crate::ipc;
use gtk::glib;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type StatusFn = Box<dyn Fn(&Hub)>;
type ActivityFn = Box<dyn Fn(&Hub, &Value)>;

#[derive(Default)]
pub struct Inner {
    pub status: RefCell<Value>,
    pub settings: RefCell<Value>,
    online: Cell<Option<bool>>,
    last_activity: Cell<u64>,
    busy: Cell<bool>,
    ticks: Cell<u32>,
    status_fns: RefCell<Vec<StatusFn>>,
    activity_fns: RefCell<Vec<ActivityFn>>,
}

#[derive(Clone, Default)]
pub struct Hub(Rc<Inner>);

impl std::ops::Deref for Hub {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl Hub {
    /// Called after every status refresh (and once the service goes on or offline).
    pub fn on_status(&self, f: impl Fn(&Hub) + 'static) {
        self.status_fns.borrow_mut().push(Box::new(f));
    }

    /// Called once for every new activity entry, oldest first.
    pub fn on_activity(&self, f: impl Fn(&Hub, &Value) + 'static) {
        self.activity_fns.borrow_mut().push(Box::new(f));
    }

    pub fn online(&self) -> bool {
        self.online.get().unwrap_or(false)
    }

    pub fn local_id(&self) -> String {
        self.status.borrow()["local_device_id"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    pub fn peers(&self) -> Vec<Value> {
        self.status.borrow()["peers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// Paired devices that are connected right now.
    pub fn connected(&self) -> Vec<Value> {
        self.peers()
            .into_iter()
            .filter(|p| p["trusted"] == true && p["status"] == "connected")
            .collect()
    }

    pub fn peer(&self, id: &str) -> Option<Value> {
        self.peers().into_iter().find(|p| p["id"] == id)
    }

    pub fn setting(&self, key: &str) -> Value {
        self.settings.borrow()[key].clone()
    }

    /// Starts polling. The feed starts at the newest entry, so old activity
    /// isn't replayed as fresh notifications.
    pub fn start(&self) {
        let hub = self.clone();
        glib::spawn_future_local(async move {
            if let Ok(recent) = ipc::call(json!({ "cmd": "activity_recent", "limit": 1 })).await {
                let newest = recent
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|e| e["id"].as_u64())
                    .unwrap_or(0);
                hub.last_activity.set(newest);
            }
            hub.refresh().await;
        });
        let hub = self.clone();
        glib::timeout_add_seconds_local(1, move || {
            let hub = hub.clone();
            glib::spawn_future_local(async move { hub.refresh().await });
            glib::ControlFlow::Continue
        });
    }

    /// Fetches everything now, e.g. right after an action, instead of waiting for the next tick.
    pub fn poke(&self) {
        let hub = self.clone();
        glib::spawn_future_local(async move { hub.refresh().await });
    }

    async fn refresh(&self) {
        if self.busy.replace(true) {
            return;
        }
        let tick = self.ticks.get();
        self.ticks.set(tick.wrapping_add(1));
        match ipc::call(json!({ "cmd": "status" })).await {
            Ok(status) => {
                *self.status.borrow_mut() = status;
                if tick.is_multiple_of(5) || self.settings.borrow().is_null() {
                    if let Ok(settings) = ipc::call(json!({ "cmd": "get_settings" })).await {
                        *self.settings.borrow_mut() = settings;
                    }
                }
                let since = self.last_activity.get();
                if let Ok(Value::Array(entries)) =
                    ipc::call(json!({ "cmd": "activity_since", "since_id": since })).await
                {
                    let mut entries = entries;
                    entries.sort_by_key(|e| e["id"].as_u64().unwrap_or(0));
                    for entry in entries
                        .iter()
                        .filter(|e| e["id"].as_u64().unwrap_or(0) > since)
                    {
                        self.last_activity
                            .set(entry["id"].as_u64().unwrap_or(since));
                        for f in self.activity_fns.borrow().iter() {
                            f(self, entry);
                        }
                    }
                }
                self.online.set(Some(true));
            }
            Err(_) => self.online.set(Some(false)),
        }
        for f in self.status_fns.borrow().iter() {
            f(self);
        }
        self.busy.set(false);
    }
}
