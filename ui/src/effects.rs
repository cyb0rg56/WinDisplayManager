use crate::app::{AppModel, Message, Page};
use cosmic::widget::nav_bar;
use data::state::Effect;

impl AppModel {
    /// Carries out state effects, returning the tasks that feed completions back.
    pub(crate) fn run_effects(&mut self, effects: Vec<Effect>) -> cosmic::app::Task<Message> {
        let mut tasks = Vec::new();
        for effect in effects {
            match effect {
                Effect::Spawn(job) => {
                    let on_failure = job.on_failure();
                    tasks.push(cosmic::app::Task::perform(
                        async move { tokio::task::spawn_blocking(move || job.run()).await },
                        move |result| {
                            cosmic::Action::App(Message::Data(
                                result.unwrap_or_else(|error| on_failure(error.to_string())),
                            ))
                        },
                    ));
                }
                Effect::Delay(delay, input) => {
                    tasks.push(cosmic::app::Task::perform(
                        async move {
                            tokio::time::sleep(delay).await;
                            input
                        },
                        |input| cosmic::Action::App(Message::Data(input)),
                    ));
                }
                Effect::MonitorsDetected => self.rebuild_nav(),
                Effect::ProfilesUpdated => {
                    if let Some((tray, _)) = &self.tray {
                        tray.update_menu(self.state.profiles());
                    }
                }
                Effect::ShowHotkeys => self.activate_page(Page::Hotkeys),
            }
        }
        cosmic::app::Task::batch(tasks)
    }

    fn rebuild_nav(&mut self) {
        self.nav = nav_bar::Model::default();
        for info in self.state.detected_monitors() {
            let label = if info.name.is_empty() {
                format!("Monitor {}", info.id)
            } else {
                format!("{} ({}x{})", info.name, info.width, info.height)
            };
            self.nav
                .insert()
                .text(label)
                .data::<Page>(Page::Monitor(info.id));
        }
        self.nav
            .insert()
            .text("Hotkeys")
            .data::<Page>(Page::Hotkeys);
        self.nav
            .insert()
            .text("Profiles")
            .data::<Page>(Page::Profiles);
        self.nav.insert().text("About").data::<Page>(Page::About);

        // Activate first monitor
        self.nav.activate_position(0);
    }
}
