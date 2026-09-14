use crate::config::{AppConfig, HotkeyActionSpec, build_hotkey_map};
use cosmic::iced::futures::SinkExt;
use cosmic::iced::{Subscription, stream};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
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

        if !config.hotkeys_enabled {
            self.action_map = Arc::new(HashMap::new());
            self.status = config
                .hotkeys
                .hotkeys
                .iter()
                .map(|hotkey| (hotkey.id.clone(), false))
                .collect();
            return;
        }

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
        if !config.hotkeys_enabled {
            self.action_map = Arc::new(HashMap::new());
            return;
        }
        let registered: HashSet<u32> = self.registered_hotkeys.iter().map(|h| h.id()).collect();
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
struct HotkeyData {
    generation: u64,
    registered_ids: Vec<u32>,
}

impl Hash for HotkeyData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.generation.hash(state);
        self.registered_ids.hash(state);
    }
}

fn should_emit(registered_ids: &[u32], event_id: u32, state: HotKeyState) -> bool {
    state == HotKeyState::Pressed && registered_ids.binary_search(&event_id).is_ok()
}

/// Create an iced `Subscription` that polls global hotkey events.
///
/// The subscription emits the triggered hotkey's OS id whenever a registered
/// hotkey is pressed. The caller resolves the current action chain by id.
pub fn hotkey_subscription(mut registered_ids: Vec<u32>, generation: u64) -> Subscription<u32> {
    registered_ids.sort_unstable();
    registered_ids.dedup();

    Subscription::run_with(
        HotkeyData {
            generation,
            registered_ids,
        },
        |data| {
            let registered_ids = data.registered_ids.clone();
            stream::channel(
                16,
                move |mut emitter: cosmic::iced::futures::channel::mpsc::Sender<u32>| async move {
                    let receiver = GlobalHotKeyEvent::receiver();
                    for _ in receiver.try_iter() {}
                    loop {
                        while let Ok(event) = receiver.try_recv() {
                            if should_emit(&registered_ids, event.id(), event.state)
                                && emitter.send(event.id()).await.is_err()
                            {
                                return;
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_emits_only_registered_press_events() {
        let ids = [7, 42];
        assert!(should_emit(&ids, 7, HotKeyState::Pressed));
        assert!(!should_emit(&ids, 7, HotKeyState::Released));
        assert!(!should_emit(&ids, 8, HotKeyState::Pressed));
    }
}
