use cosmic::iced::futures::SinkExt;
use cosmic::iced::futures::channel::mpsc::Sender;
use cosmic::iced::{Subscription, stream};
use data::tray::{TrayMessage, TrayStream};
use std::hash::{Hash, Hasher};
use std::time::Duration;

/// Identity for the hotkey subscription; it restarts when bindings change.
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

/// Emits the OS id of each registered hotkey press. The caller resolves the
/// current action chain by id.
pub(crate) fn hotkeys(mut registered_ids: Vec<u32>, generation: u64) -> Subscription<u32> {
    registered_ids.sort_unstable();
    registered_ids.dedup();

    Subscription::run_with(
        HotkeyData {
            generation,
            registered_ids,
        },
        |subscription| {
            let registered_ids = subscription.registered_ids.clone();
            stream::channel(16, move |mut emitter: Sender<u32>| async move {
                data::hotkeys::discard_pending_events();
                loop {
                    for id in data::hotkeys::take_pressed(&registered_ids) {
                        if emitter.send(id).await.is_err() {
                            return;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            })
        },
    )
}

/// Identity wrapper so a `TrayStream` can be used as `Subscription` data.
/// There is only ever one tray, so the identity is constant.
struct TrayId(TrayStream);

impl Hash for TrayId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        "system-tray".hash(state);
    }
}

pub(crate) fn tray(tray_stream: TrayStream) -> Subscription<TrayMessage> {
    Subscription::run_with(TrayId(tray_stream), |data| {
        let receiver = data.0.clone();
        stream::channel(1, move |mut sender: Sender<TrayMessage>| async move {
            while let Some(msg) = receiver.recv().await {
                if sender.send(msg).await.is_err() {
                    break;
                }
            }
        })
    })
}
