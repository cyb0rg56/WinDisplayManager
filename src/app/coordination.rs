use super::actions::{self, ActionExecutor, HardwareJob, HardwareOutcome, PreparedJob};
use crate::config::{HotkeyActionSpec, MonitorTarget, TurnOffBehavior};
use crate::ddc::{self, MonitorInfo, MonitorKey, MonitorState};
use std::collections::HashMap;

#[derive(Clone, Debug)]
enum Queued {
    Discover(u64),
    Read(u64, MonitorInfo),
    Write(PreparedJob),
    Segment(Vec<HotkeyActionSpec>, TurnOffBehavior),
    Rejected(String),
}

#[derive(Clone, Debug)]
enum Operation {
    Discover,
    Read(MonitorInfo),
    Write(PreparedJob),
    Rejected(String),
}

#[derive(Clone, Debug)]
pub(super) struct Work {
    pub id: u64,
    generation: u64,
    revision: Option<(MonitorKey, u64)>,
    operation: Operation,
}

#[derive(Clone, Debug)]
pub enum HardwareResult {
    Discovered(Vec<MonitorInfo>),
    Read(Box<MonitorState>),
    Applied(HardwareOutcome),
}

/// The seam covers the same operations that run inside the single blocking task.
/// Implementations own any OS handles locally; none enter the coordinator.
pub(super) trait HardwareBackend {
    fn discover(&mut self) -> Result<Vec<MonitorInfo>, String>;
    fn read(&mut self, info: MonitorInfo) -> Result<MonitorState, String>;
    fn apply(&mut self, job: PreparedJob) -> Result<HardwareOutcome, String>;
}

pub(super) struct WindowsHardware;

impl HardwareBackend for WindowsHardware {
    fn discover(&mut self) -> Result<Vec<MonitorInfo>, String> {
        ddc::detect_monitors().map_err(|error| error.to_string())
    }
    fn read(&mut self, info: MonitorInfo) -> Result<MonitorState, String> {
        ddc::read_monitor_state(info).map_err(|error| error.to_string())
    }
    fn apply(&mut self, job: PreparedJob) -> Result<HardwareOutcome, String> {
        job.execute()
    }
}

impl Work {
    pub(super) fn execute(
        self,
        backend: &mut impl HardwareBackend,
    ) -> Result<HardwareResult, String> {
        match self.operation {
            Operation::Discover => backend.discover().map(HardwareResult::Discovered),
            Operation::Read(info) => backend
                .read(info)
                .map(|state| HardwareResult::Read(Box::new(state))),
            Operation::Write(job) => backend.apply(job).map(HardwareResult::Applied),
            Operation::Rejected(error) => Err(error),
        }
    }
}

#[derive(Debug)]
pub(super) enum QueueEvent {
    Ignored,
    Discovered(Result<Vec<MonitorInfo>, String>),
    Read(MonitorKey, Result<Box<MonitorState>, String>),
    Applied(Result<HardwareOutcome, String>),
}

/// One FIFO for discovery, reads, writes and deferred hotkey segments. Only this
/// coordinator may dispatch hardware work or accept a hardware completion.
#[derive(Default)]
pub(super) struct HardwareCoordinator {
    executor: ActionExecutor<Queued>,
    active: Option<Work>,
    sequence: u64,
    generation: u64,
    available: Vec<MonitorInfo>,
    ready: bool,
    revisions: HashMap<MonitorKey, u64>,
}

impl HardwareCoordinator {
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    fn invalidate_topology(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("topology generation exhausted");
        self.ready = false;
        self.available.clear();
        self.revisions.clear();
    }

    /// A manual refresh cancels waiting intent, but never overlaps an active job.
    pub(super) fn refresh(&mut self) {
        self.invalidate_topology();
        self.executor.clear_pending();
        self.executor.extend([Queued::Discover(self.generation)]);
    }

    pub(super) fn accepts_monitor(&self, id: u32) -> bool {
        self.ready && self.available.iter().filter(|info| info.id == id).count() == 1
    }

    pub(super) fn retry(&mut self, id: u32) -> bool {
        if !self.accepts_monitor(id) {
            return false;
        }
        let info = self
            .available
            .iter()
            .find(|info| info.id == id)
            .unwrap()
            .clone();
        self.executor.extend([Queued::Read(self.generation, info)]);
        true
    }

    pub(super) fn enqueue_ui(&mut self, jobs: Vec<HardwareJob>) -> Result<(), String> {
        // Preparing now captures stable keys. Such writes are canceled, never
        // rebound from their UI numbers, if a barrier changes the generation.
        let prepared = actions::prepare_jobs(jobs, &self.available, self.generation)?;
        self.executor
            .extend(prepared.into_iter().map(Queued::Write));
        Ok(())
    }

    pub(super) fn enqueue_actions(
        &mut self,
        actions: Vec<HotkeyActionSpec>,
        behavior: TurnOffBehavior,
    ) -> Result<(), String> {
        actions::validate_action_bindings(&actions)?;
        let mut segment = Vec::new();
        for action in actions {
            let barrier = actions::is_profile(&action);
            segment.push(action);
            if barrier {
                self.executor
                    .extend([Queued::Segment(std::mem::take(&mut segment), behavior)]);
            }
        }
        if !segment.is_empty() {
            self.executor.extend([Queued::Segment(segment, behavior)]);
        }
        Ok(())
    }

    pub(super) fn outstanding(&self) -> impl Iterator<Item = &PreparedJob> {
        self.executor.outstanding().filter_map(|entry| match entry {
            Queued::Write(job) => Some(job),
            _ => None,
        })
    }

    pub(super) fn is_idle(&self) -> bool {
        self.executor.is_idle()
    }

    pub(super) fn start_next(&mut self, states: &[MonitorState]) -> Option<Work> {
        if self.active.is_some() {
            return None;
        }
        while let Some(entry) = self.executor.start_next() {
            let operation = match entry {
                Queued::Segment(actions, behavior) => {
                    self.executor.complete_active();
                    let prepared =
                        actions::resolve_actions(actions, behavior, &self.available, states)
                            .and_then(|jobs| {
                                actions::prepare_jobs(jobs, &self.available, self.generation)
                            });
                    match prepared {
                        Ok(jobs) => self.executor.prepend(jobs.into_iter().map(Queued::Write)),
                        Err(error) => {
                            // Resolution is atomic within a segment. Do not run
                            // the rest of a chain after its prerequisites failed.
                            self.executor.clear_pending();
                            self.executor.extend([Queued::Rejected(error)]);
                        }
                    }
                    continue;
                }
                Queued::Discover(generation) if generation == self.generation => {
                    Operation::Discover
                }
                Queued::Read(generation, info)
                    if generation == self.generation
                        && self.ready
                        && self.available.iter().any(|live| live.key == info.key) =>
                {
                    Operation::Read(info)
                }
                Queued::Write(mut job)
                    if job.generation == self.generation
                        || matches!(
                            job.job,
                            HardwareJob::ApplyProfile { .. } | HardwareJob::SaveProfile { .. }
                        ) =>
                {
                    if matches!(job.job, HardwareJob::ApplyProfile { .. }) {
                        // Start the barrier before the OS topology can change.
                        self.invalidate_topology();
                    }
                    // Profile operations carry no captured monitor binding.
                    job.generation = self.generation;
                    Operation::Write(job)
                }
                Queued::Rejected(error) => Operation::Rejected(error),
                _ => {
                    self.executor.complete_active();
                    continue;
                }
            };
            self.sequence = self
                .sequence
                .checked_add(1)
                .expect("hardware revision exhausted");
            let key = match &operation {
                Operation::Read(info) => Some(&info.key),
                Operation::Write(job) => job.key(),
                _ => None,
            };
            // Revisions advance at dispatch, not on unsent slider drafts. A read
            // finishing before a queued write is still valid confirmed knowledge.
            let revision = key.map(|key| {
                self.revisions.insert(key.clone(), self.sequence);
                (key.clone(), self.sequence)
            });
            let work = Work {
                id: self.sequence,
                generation: self.generation,
                revision,
                operation,
            };
            self.active = Some(work.clone());
            return Some(work);
        }
        None
    }

    fn accepts_result(&self, work: &Work) -> bool {
        work.generation == self.generation
            && work.revision.as_ref().is_none_or(|(key, revision)| {
                self.available.iter().any(|info| &info.key == key)
                    && self.revisions.get(key) == Some(revision)
            })
    }

    pub(super) fn finish(&mut self, id: u64, result: Result<HardwareResult, String>) -> QueueEvent {
        if self.active.as_ref().map(|work| work.id) != Some(id) {
            return QueueEvent::Ignored;
        }
        let work = self.active.take().unwrap();
        // Every terminal path, including a spawn_blocking join error, releases
        // the slot before handling results or inserting a profile refresh.
        self.executor.complete_active();
        if !self.accepts_result(&work) {
            return QueueEvent::Ignored;
        }
        match work.operation {
            Operation::Discover => {
                let result = match result {
                    Ok(HardwareResult::Discovered(infos)) => {
                        let valid = infos.iter().try_for_each(|info| {
                            MonitorTarget::Stable(info.key.clone())
                                .resolve(&infos)
                                .map(|_| ())
                        });
                        valid.map(|_| infos)
                    }
                    Err(error) => Err(error),
                    _ => Err("Unexpected discovery result".into()),
                };
                match &result {
                    Ok(infos) => {
                        self.available = infos.clone();
                        self.ready = true;
                        self.executor.prepend(
                            infos
                                .iter()
                                .cloned()
                                .map(|info| Queued::Read(self.generation, info)),
                        );
                    }
                    Err(_) => {
                        self.ready = false;
                        self.available.clear();
                        self.executor.clear_pending();
                    }
                }
                QueueEvent::Discovered(result)
            }
            Operation::Read(info) => {
                let result = match result {
                    Ok(HardwareResult::Read(state)) if state.info.key == info.key => Ok(state),
                    Err(error) => Err(error),
                    _ => Err("Read result did not match the requested monitor".into()),
                };
                QueueEvent::Read(info.key, result)
            }
            Operation::Write(job) => {
                let result = match result {
                    Ok(HardwareResult::Applied(outcome)) => {
                        let key = match &outcome {
                            HardwareOutcome::BrightnessApplied { monitor_id, .. }
                            | HardwareOutcome::ContrastApplied { monitor_id, .. }
                            | HardwareOutcome::InputSourceApplied { monitor_id, .. }
                            | HardwareOutcome::PowerModeApplied { monitor_id, .. }
                            | HardwareOutcome::CustomVcpApplied { monitor_id, .. } => {
                                Some(monitor_id)
                            }
                            _ => None,
                        };
                        if key == job.key() {
                            Ok(outcome)
                        } else {
                            Err("Write result monitor mismatch".into())
                        }
                    }
                    Err(error) => Err(error),
                    _ => Err("Unexpected write result".into()),
                };
                if matches!(job.job, HardwareJob::ApplyProfile { .. }) {
                    if result.is_err() {
                        // A failed/partially applied profile may have changed
                        // topology too. Rediscover, but cancel its dependents.
                        self.executor.clear_pending();
                    }
                    self.executor.prepend([Queued::Discover(self.generation)]);
                }
                QueueEvent::Applied(result)
            }
            Operation::Rejected(error) => QueueEvent::Applied(Err(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ActionTarget;
    use crate::ddc::tests::{FakeVcp, fake_session, key, monitor_state};
    use crate::ddc::{InputSource, VCP_BRIGHTNESS, VCP_CONTRAST, VCP_INPUT_SOURCE};
    use std::collections::VecDeque;

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Discover,
        Read(MonitorKey),
        Profile,
        Write(MonitorKey),
    }

    #[derive(Default)]
    struct FakeBackend {
        live: Vec<(MonitorInfo, FakeVcp)>,
        profile_topologies: VecDeque<Vec<(MonitorInfo, FakeVcp)>>,
        calls: Vec<Call>,
        fail_discovery: bool,
        fail_profile: bool,
        fail_read: bool,
        disconnect_after_write: Option<MonitorKey>,
    }

    fn topology(keys: &[u32]) -> Vec<(MonitorInfo, FakeVcp)> {
        keys.iter()
            .enumerate()
            .map(|(index, &id)| {
                let mut info = monitor_state().info;
                info.id = index as u32 + 1;
                info.key = key(id);
                (info, FakeVcp::default())
            })
            .collect()
    }

    impl HardwareBackend for FakeBackend {
        fn discover(&mut self) -> Result<Vec<MonitorInfo>, String> {
            self.calls.push(Call::Discover);
            if self.fail_discovery {
                return Err("discovery failed".into());
            }
            Ok(self.live.iter().map(|(info, _)| info.clone()).collect())
        }
        fn read(&mut self, info: MonitorInfo) -> Result<MonitorState, String> {
            self.calls.push(Call::Read(info.key.clone()));
            let (_, vcp) = self
                .live
                .iter()
                .find(|(live, _)| live.key == info.key)
                .ok_or("disconnected")?;
            if self.fail_read {
                return Err("read failed".into());
            }
            let mut state = monitor_state();
            state.info = info;
            for &(code, value) in vcp.writes.borrow().iter() {
                match code {
                    VCP_BRIGHTNESS => state.brightness = value,
                    VCP_CONTRAST => state.contrast = value,
                    VCP_INPUT_SOURCE => state.input_source = InputSource::from_vcp_value(value),
                    _ => {}
                }
            }
            Ok(state)
        }
        fn apply(&mut self, job: PreparedJob) -> Result<HardwareOutcome, String> {
            if let HardwareJob::ApplyProfile { name } = &job.job {
                self.calls.push(Call::Profile);
                if let Some(live) = self.profile_topologies.pop_front() {
                    self.live = live;
                }
                if self.fail_profile {
                    return Err("partially applied profile".into());
                }
                return Ok(HardwareOutcome::ProfileApplied { name: name.clone() });
            }
            let target = job.key().cloned().expect("test expected a monitor write");
            self.calls.push(Call::Write(target.clone()));
            let result = job.execute_monitor_job(
                |required| fake_session(&self.live, required),
                || panic!("tests must not sleep real displays"),
            );
            if result.is_ok() && self.disconnect_after_write.as_ref() == Some(&target) {
                self.live.retain(|(info, _)| info.key != target);
                for (index, (info, _)) in self.live.iter_mut().enumerate() {
                    info.id = index as u32 + 1;
                }
            }
            result
        }
    }

    fn finish_work(
        queue: &mut HardwareCoordinator,
        backend: &mut FakeBackend,
        work: Work,
    ) -> QueueEvent {
        let id = work.id;
        queue.finish(id, work.execute(backend))
    }

    fn run_next(queue: &mut HardwareCoordinator, backend: &mut FakeBackend) -> QueueEvent {
        let work = queue.start_next(&[]).expect("expected queued work");
        finish_work(queue, backend, work)
    }

    fn drain(queue: &mut HardwareCoordinator, backend: &mut FakeBackend) -> Vec<QueueEvent> {
        let mut events = Vec::new();
        while let Some(work) = queue.start_next(&[]) {
            assert!(events.len() < 100, "queue did not settle");
            events.push(finish_work(queue, backend, work));
        }
        assert!(queue.is_idle(), "executor wedged");
        events
    }

    fn ready(keys: &[u32]) -> (HardwareCoordinator, FakeBackend) {
        let mut queue = HardwareCoordinator::default();
        let mut backend = FakeBackend {
            live: topology(keys),
            ..Default::default()
        };
        queue.refresh();
        drain(&mut queue, &mut backend);
        backend.calls.clear();
        (queue, backend)
    }

    fn profile() -> HotkeyActionSpec {
        HotkeyActionSpec {
            target: ActionTarget::Profile,
            profile_name: "layout".into(),
            ..Default::default()
        }
    }

    fn brightness(target: u32) -> HotkeyActionSpec {
        HotkeyActionSpec {
            all_monitors: false,
            monitors: vec![MonitorTarget::Stable(key(target))],
            value: 65,
            ..Default::default()
        }
    }

    #[test]
    fn delayed_read_then_write_and_retry_are_serialized_and_old_revision_cannot_return() {
        let (mut queue, mut backend) = ready(&[1]);
        assert!(queue.retry(1));
        let read = queue.start_next(&[]).unwrap(); // Deliberately hold the completion.
        queue
            .enqueue_ui(vec![HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 80,
            }])
            .unwrap();
        assert!(queue.retry(1));
        assert!(queue.start_next(&[]).is_none());
        assert!(backend.calls.is_empty());
        let old_result = read.clone().execute(&mut backend);
        assert!(matches!(
            queue.finish(read.id, old_result.clone()),
            QueueEvent::Read(_, Ok(_))
        ));
        let write = queue.start_next(&[]).unwrap();
        assert!(
            !queue.accepts_result(&read),
            "write advances this monitor's revision"
        );
        assert!(matches!(
            queue.finish(read.id, old_result.clone()),
            QueueEvent::Ignored
        ));
        assert!(
            queue.start_next(&[]).is_none(),
            "stale read must not release active write"
        );
        assert!(queue.outstanding().any(|job| {
            job.preview_for(&backend.live[0].0, queue.generation())
                .is_some()
        }));
        assert!(matches!(
            finish_work(&mut queue, &mut backend, write),
            QueueEvent::Applied(Ok(HardwareOutcome::BrightnessApplied { value: 80, .. }))
        ));
        assert!(matches!(
            queue.finish(read.id, old_result),
            QueueEvent::Ignored
        ));
        let QueueEvent::Read(_, Ok(state)) = run_next(&mut queue, &mut backend) else {
            panic!("expected retry read")
        };
        assert_eq!(state.brightness, 80);
        assert_eq!(
            backend.calls,
            [Call::Read(key(1)), Call::Write(key(1)), Call::Read(key(1))]
        );
        assert!(queue.is_idle());
    }

    #[test]
    fn refresh_waits_for_active_work_and_rejects_old_generation_success_and_failure() {
        for fail in [false, true] {
            let (mut queue, mut backend) = ready(&[1]);
            queue.retry(1);
            let old = queue.start_next(&[]).unwrap();
            backend.fail_read = fail;
            let result = old.clone().execute(&mut backend);
            queue
                .enqueue_ui(vec![HardwareJob::SetBrightness {
                    monitor_id: 1,
                    value: 80,
                }])
                .unwrap();
            queue.refresh();
            assert!(!queue.accepts_result(&old));
            assert!(queue.start_next(&[]).is_none());
            assert!(matches!(queue.finish(old.id, result), QueueEvent::Ignored));
            backend.live = topology(&[2]); // Same display number, different key.
            backend.fail_read = false;
            let events = drain(&mut queue, &mut backend);
            assert!(matches!(events[0], QueueEvent::Discovered(Ok(_))));
            assert_eq!(
                backend.calls,
                [Call::Read(key(1)), Call::Discover, Call::Read(key(2))]
            );
            assert!(backend.live[0].1.writes.borrow().is_empty());
        }
    }

    #[test]
    fn profile_barrier_resolves_enabled_explicit_targets_and_all_on_new_topology() {
        let (mut queue, mut backend) = ready(&[1]);
        let old_info = backend.live[0].0.clone();
        backend.profile_topologies.push_back(topology(&[2, 1, 3]));
        let all = HotkeyActionSpec {
            target: ActionTarget::Contrast,
            value: 75,
            ..Default::default()
        };
        queue
            .enqueue_actions(vec![profile(), brightness(2), all], TurnOffBehavior::Ddc)
            .unwrap();
        // This UI intent was bound to A, never to the replacement at display #1.
        queue
            .enqueue_ui(vec![HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 99,
            }])
            .unwrap();
        let generation = queue.generation();
        let work = queue.start_next(&[]).unwrap();
        assert!(queue.generation() > generation);
        assert!(!queue.accepts_monitor(1));
        assert!(
            queue
                .outstanding()
                .all(|job| job.preview_for(&old_info, queue.generation()).is_none())
        );
        assert!(matches!(
            finish_work(&mut queue, &mut backend, work),
            QueueEvent::Applied(Ok(HardwareOutcome::ProfileApplied { .. }))
        ));
        let events = drain(&mut queue, &mut backend);
        assert!(matches!(events[0], QueueEvent::Discovered(Ok(_))));
        assert_eq!(
            backend.calls,
            [
                Call::Profile,
                Call::Discover,
                Call::Read(key(2)),
                Call::Read(key(1)),
                Call::Read(key(3)),
                Call::Write(key(2)),
                Call::Write(key(2)),
                Call::Write(key(1)),
                Call::Write(key(3))
            ]
        );
        assert_eq!(
            *backend.live[0].1.writes.borrow(),
            [(VCP_BRIGHTNESS, 65), (VCP_CONTRAST, 75)]
        );
        assert_eq!(*backend.live[1].1.writes.borrow(), [(VCP_CONTRAST, 75)]);
        assert_eq!(*backend.live[2].1.writes.borrow(), [(VCP_CONTRAST, 75)]);
    }

    #[test]
    fn each_profile_starts_a_new_segment_and_resolves_all_again() {
        let (mut queue, mut backend) = ready(&[1]);
        backend
            .profile_topologies
            .extend([topology(&[2]), topology(&[3, 4])]);
        let all = HotkeyActionSpec {
            value: 60,
            ..Default::default()
        };
        queue
            .enqueue_actions(
                vec![profile(), all.clone(), profile(), all],
                TurnOffBehavior::Ddc,
            )
            .unwrap();
        drain(&mut queue, &mut backend);
        assert_eq!(
            backend.calls,
            [
                Call::Profile,
                Call::Discover,
                Call::Read(key(2)),
                Call::Write(key(2)),
                Call::Profile,
                Call::Discover,
                Call::Read(key(3)),
                Call::Read(key(4)),
                Call::Write(key(3)),
                Call::Write(key(4))
            ]
        );
    }

    #[test]
    fn queued_ui_profiles_keep_their_order_across_barriers() {
        let (mut queue, mut backend) = ready(&[1]);
        backend
            .profile_topologies
            .extend([topology(&[2]), topology(&[3])]);
        queue
            .enqueue_ui(vec![
                HardwareJob::ApplyProfile {
                    name: "first".into(),
                },
                HardwareJob::ApplyProfile {
                    name: "second".into(),
                },
            ])
            .unwrap();
        drain(&mut queue, &mut backend);
        assert_eq!(
            backend.calls,
            [
                Call::Profile,
                Call::Discover,
                Call::Read(key(2)),
                Call::Profile,
                Call::Discover,
                Call::Read(key(3))
            ]
        );
    }

    #[test]
    fn obsolete_discovery_failure_does_not_cancel_a_newer_manual_refresh() {
        let (mut queue, mut backend) = ready(&[1]);
        queue.refresh();
        let obsolete = queue.start_next(&[]).unwrap();
        queue.refresh();
        queue
            .enqueue_actions(vec![brightness(2)], TurnOffBehavior::Ddc)
            .unwrap();
        assert!(matches!(
            queue.finish(obsolete.id, Err("old discovery failed".into())),
            QueueEvent::Ignored
        ));
        backend.live = topology(&[2]);
        drain(&mut queue, &mut backend);
        assert_eq!(*backend.live[0].1.writes.borrow(), [(VCP_BRIGHTNESS, 65)]);
    }

    #[test]
    fn failed_barrier_cancels_dependents_and_manual_refresh_recovers() {
        let (mut queue, mut backend) = ready(&[1]);
        queue
            .enqueue_actions(vec![profile(), brightness(1)], TurnOffBehavior::Ddc)
            .unwrap();
        backend.fail_discovery = true;
        let events = drain(&mut queue, &mut backend);
        assert!(matches!(
            events.last(),
            Some(QueueEvent::Discovered(Err(_)))
        ));
        assert_eq!(backend.calls, [Call::Profile, Call::Discover]);
        assert!(
            queue
                .enqueue_ui(vec![HardwareJob::SetBrightness {
                    monitor_id: 1,
                    value: 80
                }])
                .is_err()
        );
        backend.fail_discovery = false;
        queue.refresh();
        drain(&mut queue, &mut backend);
        queue
            .enqueue_actions(vec![brightness(1)], TurnOffBehavior::Ddc)
            .unwrap();
        drain(&mut queue, &mut backend);
        assert_eq!(*backend.live[0].1.writes.borrow(), [(VCP_BRIGHTNESS, 65)]);
    }

    #[test]
    fn partially_failed_profile_refreshes_but_does_not_run_its_dependents() {
        let (mut queue, mut backend) = ready(&[1]);
        backend.fail_profile = true;
        backend.profile_topologies.push_back(topology(&[2]));
        queue
            .enqueue_actions(vec![profile(), brightness(2)], TurnOffBehavior::Ddc)
            .unwrap();
        let events = drain(&mut queue, &mut backend);
        assert!(matches!(events[0], QueueEvent::Applied(Err(_))));
        assert_eq!(
            backend.calls,
            [Call::Profile, Call::Discover, Call::Read(key(2))]
        );
        queue
            .enqueue_actions(vec![brightness(2)], TurnOffBehavior::Ddc)
            .unwrap();
        drain(&mut queue, &mut backend);
        assert_eq!(*backend.live[0].1.writes.borrow(), [(VCP_BRIGHTNESS, 65)]);
    }

    #[test]
    fn invalid_legacy_binding_after_profile_is_rejected_before_any_side_effect() {
        let (mut queue, backend) = ready(&[1]);
        let mut legacy = brightness(1);
        legacy.monitors = vec![MonitorTarget::LegacyIndex(1)];
        assert!(
            queue
                .enqueue_actions(vec![profile(), legacy], TurnOffBehavior::Ddc)
                .is_err()
        );
        assert!(queue.start_next(&[]).is_none());
        assert!(queue.is_idle());
        assert!(backend.calls.is_empty());
    }

    #[test]
    fn segment_preflight_allows_a_to_disconnect_after_switch_before_b_writes() {
        let (mut queue, mut backend) = ready(&[1, 2]);
        let a_writes = backend.live[0].1.writes.clone();
        let b_writes = backend.live[1].1.writes.clone();
        backend.disconnect_after_write = Some(key(1));
        let inputs = HotkeyActionSpec {
            target: ActionTarget::InputSource,
            input_source: InputSource::Dp1,
            ..Default::default()
        };
        queue
            .enqueue_actions(vec![inputs], TurnOffBehavior::Ddc)
            .unwrap();
        let events = drain(&mut queue, &mut backend);
        assert!(
            events
                .iter()
                .all(|event| matches!(event, QueueEvent::Applied(Ok(_))))
        );
        assert_eq!(
            *a_writes.borrow(),
            [(VCP_INPUT_SOURCE, InputSource::Dp1.vcp_value())]
        );
        assert_eq!(
            *b_writes.borrow(),
            [(VCP_INPUT_SOURCE, InputSource::Dp1.vcp_value())]
        );
    }

    #[test]
    fn missing_initial_selection_blocks_the_whole_segment_but_not_later_requests() {
        let (mut queue, mut backend) = ready(&[1, 2]);
        let writes = backend.live[0].1.writes.clone();
        queue
            .enqueue_actions(vec![HotkeyActionSpec::default()], TurnOffBehavior::Ddc)
            .unwrap();
        backend.live.remove(1);
        let events = drain(&mut queue, &mut backend);
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .all(|event| matches!(event, QueueEvent::Applied(Err(_))))
        );
        assert!(writes.borrow().is_empty());
        queue
            .enqueue_actions(vec![brightness(1)], TurnOffBehavior::Ddc)
            .unwrap();
        drain(&mut queue, &mut backend);
        assert_eq!(*writes.borrow(), [(VCP_BRIGHTNESS, 65)]);
    }

    #[test]
    fn read_failure_and_wrong_identity_release_the_slot_without_blocking_writes() {
        let (mut queue, mut backend) = ready(&[1]);
        queue.retry(1);
        let read = queue.start_next(&[]).unwrap();
        let mut wrong = monitor_state();
        wrong.info.key = key(2);
        assert!(matches!(
            queue.finish(read.id, Ok(HardwareResult::Read(Box::new(wrong)))),
            QueueEvent::Read(_, Err(_))
        ));
        queue.retry(1);
        queue
            .enqueue_ui(vec![HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 80,
            }])
            .unwrap();
        backend.fail_read = true;
        let events = drain(&mut queue, &mut backend);
        assert!(matches!(events[0], QueueEvent::Read(_, Err(_))));
        assert!(matches!(events[1], QueueEvent::Applied(Ok(_))));
        assert_eq!(*backend.live[0].1.writes.borrow(), [(VCP_BRIGHTNESS, 80)]);
    }

    struct PanickingBackend;
    impl HardwareBackend for PanickingBackend {
        fn discover(&mut self) -> Result<Vec<MonitorInfo>, String> {
            panic!("injected discovery panic")
        }
        fn read(&mut self, _: MonitorInfo) -> Result<MonitorState, String> {
            panic!("injected read panic")
        }
        fn apply(&mut self, _: PreparedJob) -> Result<HardwareOutcome, String> {
            panic!("injected operation panic")
        }
    }

    #[tokio::test]
    async fn join_panics_release_read_write_profile_and_discovery_slots() {
        for kind in 0..4 {
            let (mut queue, mut backend) = ready(&[1]);
            match kind {
                0 => {
                    queue.retry(1);
                }
                1 => queue
                    .enqueue_ui(vec![HardwareJob::SetBrightness {
                        monitor_id: 1,
                        value: 80,
                    }])
                    .unwrap(),
                2 => queue
                    .enqueue_actions(vec![profile(), brightness(1)], TurnOffBehavior::Ddc)
                    .unwrap(),
                _ => queue.refresh(),
            }
            let work = queue.start_next(&[]).unwrap();
            let id = work.id;
            let join =
                tokio::task::spawn_blocking(move || work.execute(&mut PanickingBackend)).await;
            let error = join.expect_err("injected panic must become a join error");
            queue.finish(id, Err(format!("Task join error: {error}")));
            drain(&mut queue, &mut backend);
            queue.refresh();
            drain(&mut queue, &mut backend);
            queue
                .enqueue_actions(vec![brightness(1)], TurnOffBehavior::Ddc)
                .unwrap();
            drain(&mut queue, &mut backend);
            assert_eq!(*backend.live[0].1.writes.borrow(), [(VCP_BRIGHTNESS, 65)]);
        }
    }
}
