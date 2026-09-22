use crate::ccd;
use crate::config::{ActionTarget, ActionType, HotkeyActionSpec, MonitorTarget, TurnOffBehavior};
use crate::ddc::{
    self, InputSource, MonitorInfo, MonitorKey, MonitorState, PowerMode, VCP_BRIGHTNESS,
    VCP_CONTRAST, VCP_INPUT_SOURCE, VCP_POWER_MODE,
};
use crate::profiles;
use std::collections::VecDeque;
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HardwareJob {
    SetBrightness {
        monitor_id: u32,
        value: u16,
    },
    OffsetBrightness {
        monitor_id: u32,
        offset: i32,
    },
    SetContrast {
        monitor_id: u32,
        value: u16,
    },
    OffsetContrast {
        monitor_id: u32,
        offset: i32,
    },
    SetInputSource {
        monitor_id: u32,
        source: InputSource,
    },
    SetPowerMode {
        monitor_id: u32,
        mode: PowerMode,
    },
    SetCustomVcp {
        monitor_id: u32,
        code: u8,
        value: u16,
    },
    OffsetCustomVcp {
        monitor_id: u32,
        code: u8,
        offset: i32,
    },
    ApplyProfile {
        name: String,
    },
    SaveProfile {
        name: String,
        replace: bool,
    },
    SoftTurnOff {
        monitor_ids: Vec<u32>,
    },
}

/// Bind display numbers at enqueue time. Numbers remain only for UI previews;
/// the blocking task resolves hardware exclusively through the captured keys.
#[derive(Clone, Debug)]
pub struct PreparedJob {
    pub job: HardwareJob,
    pub generation: u64,
    key: Option<MonitorKey>,
    required: Vec<MonitorKey>,
    starts_preflight: bool,
    preflight_passed: Arc<OnceLock<()>>,
}

pub fn prepare_jobs(
    jobs: Vec<HardwareJob>,
    available: &[MonitorInfo],
    generation: u64,
) -> Result<Vec<PreparedJob>, String> {
    let resolve = |id| {
        let mut matches = available.iter().filter(|m| m.id == id);
        let info = matches
            .next()
            .ok_or_else(|| format!("Monitor {id} unavailable"))?;
        if matches.next().is_some() {
            return Err(format!("Ambiguous display number {id}"));
        }
        crate::config::MonitorTarget::Stable(info.key.clone()).resolve(available)?;
        Ok(info.key.clone())
    };
    let mut prepared = Vec::new();
    let mut selection = Vec::new();
    let preflight_passed = Arc::new(OnceLock::new());
    for job in jobs {
        let mut required = Vec::new();
        let key = match &job {
            HardwareJob::SetBrightness { monitor_id, .. }
            | HardwareJob::OffsetBrightness { monitor_id, .. }
            | HardwareJob::SetContrast { monitor_id, .. }
            | HardwareJob::OffsetContrast { monitor_id, .. }
            | HardwareJob::SetInputSource { monitor_id, .. }
            | HardwareJob::SetPowerMode { monitor_id, .. }
            | HardwareJob::SetCustomVcp { monitor_id, .. }
            | HardwareJob::OffsetCustomVcp { monitor_id, .. } => Some(resolve(*monitor_id)?),
            HardwareJob::SoftTurnOff { monitor_ids } => {
                if monitor_ids.is_empty() {
                    return Err("No monitors selected for Windows sleep".into());
                }
                for id in monitor_ids {
                    required.push(resolve(*id)?);
                }
                None
            }
            _ => None,
        };
        if let Some(key) = &key {
            required.push(key.clone());
        }
        for key in &required {
            if !selection.contains(key) {
                selection.push(key.clone());
            }
        }
        prepared.push(PreparedJob {
            job,
            generation,
            key,
            required,
            starts_preflight: false,
            preflight_passed: Arc::clone(&preflight_passed),
        });
    }
    // Check the full selection once, before the first monitor operation (not a
    // profile operation). Later jobs must tolerate earlier targets deliberately
    // disconnecting after an input switch or power-off.
    if let Some(first) = prepared.iter_mut().find(|job| !job.required.is_empty()) {
        first.required = selection;
        first.starts_preflight = true;
    }
    Ok(prepared)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HardwareOutcome {
    BrightnessApplied {
        monitor_id: MonitorKey,
        value: u16,
    },
    ContrastApplied {
        monitor_id: MonitorKey,
        value: u16,
    },
    InputSourceApplied {
        monitor_id: MonitorKey,
        source: InputSource,
    },
    PowerModeApplied {
        monitor_id: MonitorKey,
        mode: PowerMode,
    },
    CustomVcpApplied {
        monitor_id: MonitorKey,
        code: u8,
        value: u16,
    },
    ProfileApplied {
        name: String,
    },
    ProfileSaved {
        name: String,
    },
    MonitorsPoweredOff,
}

impl PreparedJob {
    pub(super) fn key(&self) -> Option<&MonitorKey> {
        self.key.as_ref()
    }

    pub fn preview_for(&self, info: &MonitorInfo, generation: u64) -> Option<&HardwareJob> {
        (self.generation == generation && self.key.as_ref() == Some(&info.key)).then_some(&self.job)
    }

    pub fn execute(self) -> Result<HardwareOutcome, String> {
        // Profile operations do not target DDC endpoints.
        if let HardwareJob::ApplyProfile { name } = self.job {
            profiles::apply_profile(&name).map_err(|e| format!("Apply profile error: {e}"))?;
            return Ok(HardwareOutcome::ProfileApplied { name });
        }
        if let HardwareJob::SaveProfile { name, replace } = self.job {
            profiles::save_current(&name, replace)
                .map_err(|e| format!("Save profile error: {e}"))?;
            return Ok(HardwareOutcome::ProfileSaved { name });
        }
        self.execute_monitor_job(ddc::Session::open, ccd::turn_off_monitors)
    }

    pub(super) fn execute_monitor_job<M: ddc::Vcp>(
        self,
        open: impl FnOnce(&[MonitorKey]) -> ddc::Result<ddc::Session<M>>,
        soft_off: impl FnOnce(),
    ) -> Result<HardwareOutcome, String> {
        // The FIFO queue continues after errors. A failed initial preflight
        // must therefore block the remaining monitor jobs in this batch.
        if !self.starts_preflight && self.preflight_passed.get().is_none() {
            return Err(
                "Monitor batch blocked: initial selection preflight did not succeed".into(),
            );
        }
        let mut session = open(&self.required).map_err(|e| e.to_string())?;
        if self.starts_preflight {
            let _ = self.preflight_passed.set(());
        }
        if matches!(self.job, HardwareJob::SoftTurnOff { .. }) {
            soft_off();
            return Ok(HardwareOutcome::MonitorsPoweredOff);
        }
        let monitor_id = self.key.ok_or("Missing stable monitor binding")?;
        match self.job {
            HardwareJob::SetBrightness { value, .. } => {
                session
                    .set_vcp(&monitor_id, VCP_BRIGHTNESS, value)
                    .map_err(|error| format!("Brightness error: {error}"))?;
                Ok(HardwareOutcome::BrightnessApplied { monitor_id, value })
            }
            HardwareJob::OffsetBrightness { offset, .. } => {
                let value = session
                    .offset_vcp(&monitor_id, VCP_BRIGHTNESS, offset)
                    .map_err(|error| format!("Brightness error: {error}"))?;
                Ok(HardwareOutcome::BrightnessApplied { monitor_id, value })
            }
            HardwareJob::SetContrast { value, .. } => {
                session
                    .set_vcp(&monitor_id, VCP_CONTRAST, value)
                    .map_err(|error| format!("Contrast error: {error}"))?;
                Ok(HardwareOutcome::ContrastApplied { monitor_id, value })
            }
            HardwareJob::OffsetContrast { offset, .. } => {
                let value = session
                    .offset_vcp(&monitor_id, VCP_CONTRAST, offset)
                    .map_err(|error| format!("Contrast error: {error}"))?;
                Ok(HardwareOutcome::ContrastApplied { monitor_id, value })
            }
            HardwareJob::SetInputSource { source, .. } => {
                session
                    .set_vcp(&monitor_id, VCP_INPUT_SOURCE, source.vcp_value())
                    .map_err(|error| format!("Input source error: {error}"))?;
                Ok(HardwareOutcome::InputSourceApplied { monitor_id, source })
            }
            HardwareJob::SetPowerMode { mode, .. } => {
                session
                    .set_vcp(&monitor_id, VCP_POWER_MODE, mode.vcp_value())
                    .map_err(|error| format!("Power mode error: {error}"))?;
                Ok(HardwareOutcome::PowerModeApplied { monitor_id, mode })
            }
            HardwareJob::SetCustomVcp { code, value, .. } => {
                session
                    .set_vcp(&monitor_id, code, value)
                    .map_err(|error| format!("Custom VCP error: {error}"))?;
                Ok(HardwareOutcome::CustomVcpApplied {
                    monitor_id,
                    code,
                    value,
                })
            }
            HardwareJob::OffsetCustomVcp { code, offset, .. } => {
                let value = session
                    .offset_vcp(&monitor_id, code, offset)
                    .map_err(|error| format!("Custom VCP error: {error}"))?;
                Ok(HardwareOutcome::CustomVcpApplied {
                    monitor_id,
                    code,
                    value,
                })
            }
            _ => unreachable!("non-DDC jobs handled above"),
        }
    }
}

pub(super) fn is_profile(action: &HotkeyActionSpec) -> bool {
    action.target == ActionTarget::Profile && action.action_type != ActionType::Off
}

/// Reject bindings that can never resolve before *any* action has side effects.
/// Presence of stable targets is checked per segment, after any profile barrier.
pub(super) fn validate_action_bindings(actions: &[HotkeyActionSpec]) -> Result<(), String> {
    for action in actions {
        if is_profile(action) || action.all_monitors {
            continue;
        }
        let targets = action.explicit_targets();
        if targets.is_empty() {
            return Err("No target monitors selected".into());
        }
        let mut seen = std::collections::HashSet::new();
        for target in targets {
            match &target {
                MonitorTarget::LegacyIndex(_) => {
                    return Err(format!(
                        "{target}: select Rebind or Remove in the hotkey editor"
                    ));
                }
                MonitorTarget::Stable(key) if !key.is_supported() => {
                    return Err(format!("Unsupported monitor key: {key}"));
                }
                _ => {}
            }
            if !seen.insert(target.clone()) {
                return Err(format!("Duplicate monitor assignment: {target}"));
            }
        }
    }
    Ok(())
}

pub(super) fn turn_off_jobs(behavior: TurnOffBehavior, monitor_ids: &[u32]) -> Vec<HardwareJob> {
    let mut jobs = Vec::new();
    if behavior.uses_soft() {
        jobs.push(HardwareJob::SoftTurnOff {
            monitor_ids: monitor_ids.to_vec(),
        });
    }
    if behavior.uses_ddc() {
        jobs.extend(
            monitor_ids
                .iter()
                .map(|&monitor_id| HardwareJob::SetPowerMode {
                    monitor_id,
                    mode: PowerMode::Off,
                }),
        );
    }
    jobs
}

/// Resolve one segment only. The coordinator splits chains at profile applications.
pub(super) fn resolve_actions(
    actions: Vec<HotkeyActionSpec>,
    behavior: TurnOffBehavior,
    available: &[MonitorInfo],
    states: &[MonitorState],
) -> Result<Vec<HardwareJob>, String> {
    let mut jobs = Vec::new();
    for action in actions {
        if is_profile(&action) {
            jobs.push(HardwareJob::ApplyProfile {
                name: action.profile_name,
            });
            continue;
        }
        let ids = action.resolve_monitors(available)?;
        if action.action_type == ActionType::Off {
            jobs.extend(turn_off_jobs(behavior, &ids));
            continue;
        }
        for (index, monitor_id) in ids.into_iter().enumerate() {
            let maximum = states
                .iter()
                .find(|state| {
                    available
                        .iter()
                        .any(|info| info.id == monitor_id && info.key == state.info.key)
                })
                .map(|state| match action.target {
                    ActionTarget::Brightness => state.brightness_max,
                    ActionTarget::Contrast => state.contrast_max,
                    _ => u16::MAX,
                })
                .unwrap_or(100);
            let job = match (action.target, action.action_type) {
                (ActionTarget::Brightness, ActionType::Set) => HardwareJob::SetBrightness {
                    monitor_id,
                    value: action.value.clamp(0, i32::from(maximum)) as u16,
                },
                (ActionTarget::Brightness, ActionType::Offset) => HardwareJob::OffsetBrightness {
                    monitor_id,
                    offset: action.value,
                },
                (ActionTarget::Contrast, ActionType::Set) => HardwareJob::SetContrast {
                    monitor_id,
                    value: action.value.clamp(0, i32::from(maximum)) as u16,
                },
                (ActionTarget::Contrast, ActionType::Offset) => HardwareJob::OffsetContrast {
                    monitor_id,
                    offset: action.value,
                },
                (ActionTarget::InputSource, _) => HardwareJob::SetInputSource {
                    monitor_id,
                    source: if action.all_monitors || action.monitor_inputs.is_empty() {
                        action.input_source
                    } else {
                        action.monitor_inputs[index].input_source
                    },
                },
                (ActionTarget::PowerMode, _) => HardwareJob::SetPowerMode {
                    monitor_id,
                    mode: action.power_mode,
                },
                (ActionTarget::CustomVcp, ActionType::Set) => HardwareJob::SetCustomVcp {
                    monitor_id,
                    code: action.vcp_code,
                    value: action.value.clamp(0, i32::from(u16::MAX)) as u16,
                },
                (ActionTarget::CustomVcp, ActionType::Offset) => HardwareJob::OffsetCustomVcp {
                    monitor_id,
                    code: action.vcp_code,
                    offset: action.value,
                },
                _ => unreachable!("profile and off actions handled above"),
            };
            jobs.push(job);
        }
    }
    Ok(jobs)
}

#[derive(Debug)]
pub struct ActionExecutor<Job> {
    pending: VecDeque<Job>,
    active: Option<Job>,
}

impl<Job> Default for ActionExecutor<Job> {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            active: None,
        }
    }
}

impl<Job> ActionExecutor<Job> {
    pub fn extend(&mut self, jobs: impl IntoIterator<Item = Job>) {
        self.pending.extend(jobs);
    }

    pub(super) fn prepend(&mut self, jobs: impl IntoIterator<Item = Job>) {
        let mut front: VecDeque<_> = jobs.into_iter().collect();
        front.append(&mut self.pending);
        self.pending = front;
    }

    pub(super) fn clear_pending(&mut self) {
        self.pending.clear();
    }

    pub fn start_next(&mut self) -> Option<Job>
    where
        Job: Clone,
    {
        if self.active.is_some() {
            return None;
        }
        let job = self.pending.pop_front()?;
        self.active = Some(job.clone());
        Some(job)
    }

    /// Outstanding jobs in execution order, including the in-flight job.
    pub fn outstanding(&self) -> impl Iterator<Item = &Job> {
        self.active.iter().chain(self.pending.iter())
    }

    pub fn complete_active(&mut self) {
        debug_assert!(
            self.active.is_some(),
            "completed a hardware job while none was active"
        );
        self.active = None;
    }

    pub fn is_idle(&self) -> bool {
        self.active.is_none() && self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddc::tests::{FakeVcp, fake_session, key, monitor_state};

    #[derive(Default)]
    struct FakeHardware {
        completed: Vec<&'static str>,
    }

    impl FakeHardware {
        fn run(&mut self, job: &'static str) {
            self.completed.push(job);
        }
    }

    #[test]
    fn enqueue_binds_keys_and_pending_previews_cannot_follow_reused_numbers() {
        let info = monitor_state().info;
        let jobs = prepare_jobs(
            vec![HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 80,
            }],
            std::slice::from_ref(&info),
            7,
        )
        .unwrap();
        assert_eq!(jobs[0].key, Some(key(1)));
        assert_eq!(jobs[0].required, [key(1)]);
        assert!(jobs[0].preview_for(&info, 7).is_some());
        assert!(jobs[0].preview_for(&info, 8).is_none());
        let mut replacement = info;
        replacement.key = key(2);
        assert!(jobs[0].preview_for(&replacement, 7).is_none());
        assert_eq!(jobs[0].key, Some(key(1)));
    }

    #[test]
    fn full_dispatch_preflight_includes_soft_off_and_does_not_partially_enqueue() {
        let info = monitor_state().info;
        let jobs = vec![
            HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 80,
            },
            HardwareJob::SoftTurnOff {
                monitor_ids: vec![1, 2],
            },
        ];
        assert!(prepare_jobs(jobs, std::slice::from_ref(&info), 7).is_err());
        assert!(
            prepare_jobs(
                vec![HardwareJob::SoftTurnOff {
                    monitor_ids: vec![]
                }],
                std::slice::from_ref(&info),
                7
            )
            .is_err()
        );
        let mut other = info.clone();
        other.id = 2;
        other.key = key(2);
        let prepared = prepare_jobs(
            vec![HardwareJob::SoftTurnOff {
                monitor_ids: vec![1, 2],
            }],
            &[info, other],
            7,
        )
        .unwrap();
        assert_eq!(prepared[0].required, [key(1), key(2)]);
        // No execute() calls: Windows sleep and physical APIs are never invoked.
    }

    fn fake_monitors() -> Vec<(MonitorInfo, FakeVcp)> {
        (1..=3)
            .map(|id| {
                let mut info = monitor_state().info;
                info.id = id;
                info.key = key(id);
                (info, FakeVcp::default())
            })
            .collect()
    }

    #[test]
    fn completed_target_can_disconnect_without_blocking_the_next_monitor() {
        for first in [
            HardwareJob::SetInputSource {
                monitor_id: 1,
                source: InputSource::Dp1,
            },
            HardwareJob::SetPowerMode {
                monitor_id: 1,
                mode: PowerMode::Off,
            },
        ] {
            let mut live = fake_monitors();
            let available: Vec<_> = live.iter().map(|(info, _)| info.clone()).collect();
            let writes_a = live[0].1.writes.clone();
            let writes_b = live[1].1.writes.clone();
            let prepared = prepare_jobs(
                vec![
                    first,
                    HardwareJob::SetInputSource {
                        monitor_id: 2,
                        source: InputSource::Hdmi2,
                    },
                ],
                &available,
                7,
            )
            .unwrap();
            assert_eq!(prepared[0].required, [key(1), key(2)]);
            assert_eq!(prepared[1].required, [key(2)]);
            let mut executor = ActionExecutor::default();
            executor.extend(prepared);
            executor
                .start_next()
                .unwrap()
                .execute_monitor_job(
                    |required| fake_session(&live, required),
                    || panic!("unexpected Windows sleep"),
                )
                .unwrap();
            executor.complete_active();
            assert_eq!(writes_a.borrow().len(), 1);
            live.remove(0); // A's successful input/power change removes A.
            live[0].0.id = 1; // B may now have A's old display number.
            let outcome = executor
                .start_next()
                .unwrap()
                .execute_monitor_job(
                    |required| fake_session(&live, required),
                    || panic!("unexpected Windows sleep"),
                )
                .unwrap();
            executor.complete_active();
            assert_eq!(
                outcome,
                HardwareOutcome::InputSourceApplied {
                    monitor_id: key(2),
                    source: InputSource::Hdmi2,
                }
            );
            assert_eq!(
                *writes_b.borrow(),
                [(VCP_INPUT_SOURCE, InputSource::Hdmi2.vcp_value())]
            );
            assert!(executor.is_idle());
        }
    }

    #[test]
    fn missing_target_before_initial_preflight_blocks_all_monitor_writes() {
        // Exercise both a missing first target and a present first target with
        // a missing later target: neither may produce any write in this batch.
        for order in [[1, 2], [2, 1]] {
            let mut live = fake_monitors();
            let available: Vec<_> = live.iter().map(|(info, _)| info.clone()).collect();
            let writes: Vec<_> = live
                .iter()
                .map(|(_, monitor)| monitor.writes.clone())
                .collect();
            let jobs = order
                .into_iter()
                .map(|monitor_id| HardwareJob::SetInputSource {
                    monitor_id,
                    source: InputSource::Dp1,
                })
                .collect();
            let prepared = prepare_jobs(jobs, &available, 7).unwrap();
            let mut executor = ActionExecutor::default();
            executor.extend(prepared);
            live.remove(0); // A disappears after UI preparation, before execution.
            let mut opens = 0;
            while let Some(job) = executor.start_next() {
                assert!(
                    job.execute_monitor_job(
                        |required| {
                            opens += 1;
                            fake_session(&live, required)
                        },
                        || panic!("unexpected Windows sleep"),
                    )
                    .is_err()
                );
                executor.complete_active();
            }
            assert_eq!(opens, 1, "a failed first preflight must gate later jobs");
            assert!(writes.iter().all(|writes| writes.borrow().is_empty()));
        }
    }

    #[test]
    fn profiles_do_not_consume_initial_preflight_and_later_soft_off_keeps_its_group() {
        let live = fake_monitors();
        let available: Vec<_> = live.iter().map(|(info, _)| info.clone()).collect();
        let prepared = prepare_jobs(
            vec![
                HardwareJob::SaveProfile {
                    name: "saved".into(),
                    replace: false,
                },
                HardwareJob::ApplyProfile {
                    name: "saved".into(),
                },
                HardwareJob::SetInputSource {
                    monitor_id: 1,
                    source: InputSource::Dp1,
                },
                HardwareJob::SoftTurnOff {
                    monitor_ids: vec![2, 3],
                },
                HardwareJob::SetBrightness {
                    monitor_id: 2,
                    value: 80,
                },
            ],
            &available,
            7,
        )
        .unwrap();
        for profile in &prepared[..2] {
            assert!(profile.required.is_empty());
            assert!(!profile.starts_preflight);
        }
        assert!(prepared[2].starts_preflight);
        assert_eq!(prepared[2].required, [key(1), key(2), key(3)]);
        assert_eq!(prepared[3].required, [key(2), key(3)]);
        assert!(!prepared[3].starts_preflight);
        assert_eq!(prepared[4].required, [key(2)]);
        // Profiles are not executed: their topology barriers remain separate work.
        prepared[2]
            .clone()
            .execute_monitor_job(
                |required| fake_session(&live, required),
                || panic!("unexpected Windows sleep"),
            )
            .unwrap();
        let sleep_calls = std::cell::Cell::new(0);
        assert!(
            prepared[3]
                .clone()
                .execute_monitor_job(
                    |required| fake_session(&live[1..2], required), // C missing: no sleep.
                    || sleep_calls.set(sleep_calls.get() + 1),
                )
                .is_err()
        );
        assert_eq!(sleep_calls.get(), 0);
        prepared[3]
            .clone()
            .execute_monitor_job(
                |required| fake_session(&live[1..], required), // A absent, B+C present.
                || sleep_calls.set(sleep_calls.get() + 1),
            )
            .unwrap();
        assert_eq!(sleep_calls.get(), 1);
        assert!(
            live[1..]
                .iter()
                .all(|(_, monitor)| monitor.writes.borrow().is_empty())
        );
    }

    #[test]
    fn soft_off_first_preflights_future_monitor_jobs_before_sleep() {
        let live = fake_monitors();
        let available: Vec<_> = live.iter().map(|(info, _)| info.clone()).collect();
        let prepared = prepare_jobs(
            vec![
                HardwareJob::SoftTurnOff {
                    monitor_ids: vec![1],
                },
                HardwareJob::SetBrightness {
                    monitor_id: 2,
                    value: 80,
                },
            ],
            &available,
            7,
        )
        .unwrap();
        assert!(prepared[0].starts_preflight);
        assert_eq!(prepared[0].required, [key(1), key(2)]);
        assert_eq!(prepared[1].required, [key(2)]);
        assert!(
            prepared[0]
                .clone()
                .execute_monitor_job(
                    |required| fake_session(&live[..1], required),
                    || panic!("sleep must not occur when B is missing"),
                )
                .is_err()
        );
        assert!(
            prepared[1]
                .clone()
                .execute_monitor_job::<FakeVcp>(
                    |_| panic!("initial preflight never succeeded"),
                    || panic!("unexpected Windows sleep"),
                )
                .is_err()
        );
    }

    #[test]
    fn executor_runs_one_job_at_a_time_in_fifo_order() {
        let mut executor = ActionExecutor::default();
        let mut hardware = FakeHardware::default();
        executor.extend(["first", "second"]);

        let first = executor.start_next().unwrap();
        assert!(executor.start_next().is_none());
        executor.extend(["third"]);
        hardware.run(first);
        executor.complete_active();

        while let Some(job) = executor.start_next() {
            hardware.run(job);
            executor.complete_active();
        }

        assert_eq!(hardware.completed, ["first", "second", "third"]);
        assert!(executor.is_idle());
    }
}
