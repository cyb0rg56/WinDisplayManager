use crate::config::{build_hotkey_map, AppConfig, HotkeyActionSpec};
use cosmic::iced::futures::SinkExt;
use cosmic::iced::{stream, Subscription};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HotkeyManager – registers hotkeys and keeps the ID → action-chain mapping
// ---------------------------------------------------------------------------

pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    action_map: Arc<HashMap<u32, Vec<HotkeyActionSpec>>>,
    registered_hotkeys: Vec<HotKey>,
    /// Per-config-hotkey (keyed by `Hotkey::id`) registration status, used to
    /// drive the active/inactive indicator in the UI.
    status: HashMap<String, bool>,
}

impl HotkeyManager {
    /// Create a new hotkey manager and register all hotkeys from the given config.
    /// Returns `None` if the `GlobalHotKeyManager` cannot be created.
    pub fn new(config: &AppConfig) -> Option<Self> {
        let manager = GlobalHotKeyManager::new().ok()?;
        let mut me = Self {
            manager,
            action_map: Arc::new(HashMap::new()),
            registered_hotkeys: Vec::new(),
            status: HashMap::new(),
        };
        me.update(config);
        Some(me)
    }

    /// Update the hotkey registrations with a new config.
    /// This unregisters old hotkeys and registers new ones.
    pub fn update(&mut self, config: &AppConfig) {
        // Unregister all current hotkeys
        for hotkey in &self.registered_hotkeys {
            if let Err(e) = self.manager.unregister(*hotkey) {
                log::warn!("Failed to unregister hotkey {:?}: {e}", hotkey);
            }
        }
        self.registered_hotkeys.clear();

        // Build new hotkey map and register each entry.
        let hk_map = build_hotkey_map(&config.hotkeys);
        let mut action_map = HashMap::new();
        let mut registered_ids: HashSet<u32> = HashSet::new();

        for (id, (hotkey, actions)) in &hk_map {
            match self.manager.register(*hotkey) {
                Ok(_) => {
                    action_map.insert(*id, actions.clone());
                    self.registered_hotkeys.push(*hotkey);
                    registered_ids.insert(*id);
                    log::info!("Registered hotkey {:?} with id {}", hotkey, id);
                }
                Err(e) => {
                    log::warn!("Failed to register hotkey {:?}: {e}", hotkey);
                }
            }
        }

        self.action_map = Arc::new(action_map);

        // Recompute per-config-hotkey active status by re-deriving the OS id
        // from each binding and checking whether it registered successfully.
        let mut status = HashMap::new();
        for cfg_hotkey in &config.hotkeys.hotkeys {
            let active = cfg_hotkey
                .binding
                .to_hotkey()
                .is_some_and(|hk| registered_ids.contains(&hk.id()));
            status.insert(cfg_hotkey.id.clone(), active);
        }
        self.status = status;
    }

    /// Rebuild only the in-memory action-chain payload map from a new config,
    /// WITHOUT touching OS hotkey registration. Cheap enough to call on every
    /// UI edit of an action's fields. Preserves the exact key set of the
    /// currently-registered hotkeys so the polling subscription is not
    /// restarted.
    pub fn rebuild_action_map(&mut self, config: &AppConfig) {
        let registered: HashSet<u32> =
            self.registered_hotkeys.iter().map(|h| h.id()).collect();
        let hk_map = build_hotkey_map(&config.hotkeys);
        let mut action_map = HashMap::new();
        for (id, (_hotkey, actions)) in &hk_map {
            if registered.contains(id) {
                action_map.insert(*id, actions.clone());
            }
        }
        self.action_map = Arc::new(action_map);
    }

    /// Get a clone of the action map (for use in the subscription).
    pub fn action_map(&self) -> Arc<HashMap<u32, Vec<HotkeyActionSpec>>> {
        Arc::clone(&self.action_map)
    }

    /// Get a clone of the per-hotkey active/inactive status map.
    pub fn status(&self) -> HashMap<String, bool> {
        self.status.clone()
    }
}

// ---------------------------------------------------------------------------
// Subscription – polls global hotkey events and emits action chains
// ---------------------------------------------------------------------------

/// Identity/data wrapper for the hotkey subscription. Hashing the set of
/// registered hotkey ids ensures the subscription restarts when the bindings
/// change.
struct HotkeyData(Arc<HashMap<u32, Vec<HotkeyActionSpec>>>);

impl Hash for HotkeyData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut ids: Vec<u32> = self.0.keys().copied().collect();
        ids.sort_unstable();
        ids.hash(state);
    }
}

/// Create an iced `Subscription` that polls global hotkey events.
///
/// The subscription emits the triggered hotkey's action chain whenever a
/// registered hotkey is pressed. The caller is responsible for mapping these
/// into the application's `Message` type.
pub fn hotkey_subscription(
    action_map: Arc<HashMap<u32, Vec<HotkeyActionSpec>>>,
) -> Subscription<Vec<HotkeyActionSpec>> {
    Subscription::run_with(HotkeyData(action_map), |data| {
        let map = Arc::clone(&data.0);
        stream::channel(
            16,
            move |mut emitter: cosmic::iced::futures::channel::mpsc::Sender<Vec<HotkeyActionSpec>>| async move {
                let receiver = GlobalHotKeyEvent::receiver();
                loop {
                    // Drain all pending events
                    while let Ok(event) = receiver.try_recv() {
                        if let Some(actions) = map.get(&event.id()) {
                            let _ = emitter.send(actions.clone()).await;
                        }
                    }
                    // Poll at 100ms intervals to reduce overhead while maintaining responsiveness
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
        )
    })
}

