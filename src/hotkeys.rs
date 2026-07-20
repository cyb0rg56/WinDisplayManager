use crate::config::{build_hotkey_map, AppConfig, HotkeyAction};
use cosmic::iced::futures::SinkExt;
use cosmic::iced::{stream, Subscription};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

// ---------------------------------------------------------------------------
// HotkeyManager – registers hotkeys and keeps the ID → action mapping
// ---------------------------------------------------------------------------

pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    action_map: Arc<HashMap<u32, HotkeyAction>>,
    registered_hotkeys: Vec<HotKey>,
}

impl HotkeyManager {
    /// Create a new hotkey manager and register all hotkeys from the given config.
    /// Returns `None` if the `GlobalHotKeyManager` cannot be created.
    pub fn new(config: &AppConfig) -> Option<Self> {
        let manager = GlobalHotKeyManager::new().ok()?;
        let hk_map = build_hotkey_map(&config.hotkeys);

        let mut action_map = HashMap::new();
        let mut registered_hotkeys = Vec::new();
        
        for (id, (hotkey, action)) in &hk_map {
            match manager.register(*hotkey) {
                Ok(_) => {
                    action_map.insert(*id, action.clone());
                    registered_hotkeys.push(*hotkey);
                    log::info!("Registered hotkey {:?} with id {}", hotkey, id);
                }
                Err(e) => {
                    log::warn!("Failed to register hotkey {:?}: {e}", hotkey);
                }
            }
        }

        Some(Self {
            manager,
            action_map: Arc::new(action_map),
            registered_hotkeys,
        })
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
        
        // Build new hotkey map
        let hk_map = build_hotkey_map(&config.hotkeys);
        let mut action_map = HashMap::new();
        
        for (id, (hotkey, action)) in &hk_map {
            match self.manager.register(*hotkey) {
                Ok(_) => {
                    action_map.insert(*id, action.clone());
                    self.registered_hotkeys.push(*hotkey);
                    log::info!("Registered hotkey {:?} with id {}", hotkey, id);
                }
                Err(e) => {
                    log::warn!("Failed to register hotkey {:?}: {e}", hotkey);
                }
            }
        }
        
        self.action_map = Arc::new(action_map);
    }

    /// Get a clone of the action map (for use in the subscription).
    pub fn action_map(&self) -> Arc<HashMap<u32, HotkeyAction>> {
        Arc::clone(&self.action_map)
    }
}

// ---------------------------------------------------------------------------
// Subscription – polls global hotkey events and emits HotkeyAction messages
// ---------------------------------------------------------------------------

/// Identity/data wrapper for the hotkey subscription. Hashing the set of
/// registered hotkey ids ensures the subscription restarts when the bindings
/// change.
struct HotkeyData(Arc<HashMap<u32, HotkeyAction>>);

impl Hash for HotkeyData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut ids: Vec<u32> = self.0.keys().copied().collect();
        ids.sort_unstable();
        ids.hash(state);
    }
}

/// Create an iced `Subscription` that polls global hotkey events.
///
/// The subscription emits `HotkeyAction` values whenever a registered hotkey
/// is pressed. The caller is responsible for mapping these into the
/// application's `Message` type.
pub fn hotkey_subscription(
    action_map: Arc<HashMap<u32, HotkeyAction>>,
) -> Subscription<HotkeyAction> {
    Subscription::run_with(HotkeyData(action_map), |data| {
        let map = Arc::clone(&data.0);
        stream::channel(
            16,
            move |mut emitter: cosmic::iced::futures::channel::mpsc::Sender<HotkeyAction>| async move {
                let receiver = GlobalHotKeyEvent::receiver();
                loop {
                    // Drain all pending events
                    while let Ok(event) = receiver.try_recv() {
                        if let Some(action) = map.get(&event.id()) {
                            let _ = emitter.send(action.clone()).await;
                        }
                    }
                    // Poll at 100ms intervals to reduce overhead while maintaining responsiveness
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            },
        )
    })
}
