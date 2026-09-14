use crate::ccd;
use crate::ddc::{self, InputSource, PowerMode, VCP_BRIGHTNESS, VCP_CONTRAST};
use crate::profiles;
use std::collections::VecDeque;

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
    SoftTurnOff,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HardwareOutcome {
    BrightnessApplied {
        monitor_id: u32,
        value: u16,
    },
    ContrastApplied {
        monitor_id: u32,
        value: u16,
    },
    InputSourceApplied {
        monitor_id: u32,
        source: InputSource,
    },
    PowerModeApplied {
        monitor_id: u32,
        mode: PowerMode,
    },
    CustomVcpApplied {
        monitor_id: u32,
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

impl HardwareJob {
    pub fn execute(self) -> Result<HardwareOutcome, String> {
        match self {
            Self::SetBrightness { monitor_id, value } => {
                ddc::set_brightness(monitor_id, value)
                    .map_err(|error| format!("Brightness error: {error}"))?;
                Ok(HardwareOutcome::BrightnessApplied { monitor_id, value })
            }
            Self::OffsetBrightness { monitor_id, offset } => {
                let value = apply_vcp_offset(monitor_id, VCP_BRIGHTNESS, offset)
                    .map_err(|error| format!("Brightness error: {error}"))?;
                Ok(HardwareOutcome::BrightnessApplied { monitor_id, value })
            }
            Self::SetContrast { monitor_id, value } => {
                ddc::set_contrast(monitor_id, value)
                    .map_err(|error| format!("Contrast error: {error}"))?;
                Ok(HardwareOutcome::ContrastApplied { monitor_id, value })
            }
            Self::OffsetContrast { monitor_id, offset } => {
                let value = apply_vcp_offset(monitor_id, VCP_CONTRAST, offset)
                    .map_err(|error| format!("Contrast error: {error}"))?;
                Ok(HardwareOutcome::ContrastApplied { monitor_id, value })
            }
            Self::SetInputSource { monitor_id, source } => {
                ddc::set_input_source(monitor_id, source)
                    .map_err(|error| format!("Input source error: {error}"))?;
                Ok(HardwareOutcome::InputSourceApplied { monitor_id, source })
            }
            Self::SetPowerMode { monitor_id, mode } => {
                ddc::set_power_mode(monitor_id, mode)
                    .map_err(|error| format!("Power mode error: {error}"))?;
                Ok(HardwareOutcome::PowerModeApplied { monitor_id, mode })
            }
            Self::SetCustomVcp {
                monitor_id,
                code,
                value,
            } => {
                ddc::set_vcp(monitor_id, code, value)
                    .map_err(|error| format!("Custom VCP error: {error}"))?;
                Ok(HardwareOutcome::CustomVcpApplied {
                    monitor_id,
                    code,
                    value,
                })
            }
            Self::OffsetCustomVcp {
                monitor_id,
                code,
                offset,
            } => {
                let value = apply_vcp_offset(monitor_id, code, offset)
                    .map_err(|error| format!("Custom VCP error: {error}"))?;
                Ok(HardwareOutcome::CustomVcpApplied {
                    monitor_id,
                    code,
                    value,
                })
            }
            Self::ApplyProfile { name } => {
                profiles::apply_profile(&name)
                    .map_err(|error| format!("Apply profile error: {error}"))?;
                Ok(HardwareOutcome::ProfileApplied { name })
            }
            Self::SaveProfile { name, replace } => {
                profiles::save_current(&name, replace)
                    .map_err(|error| format!("Save profile error: {error}"))?;
                Ok(HardwareOutcome::ProfileSaved { name })
            }
            Self::SoftTurnOff => {
                ccd::turn_off_monitors();
                Ok(HardwareOutcome::MonitorsPoweredOff)
            }
        }
    }
}

fn apply_vcp_offset(monitor_id: u32, code: u8, offset: i32) -> ddc::Result<u16> {
    let (current, maximum) = ddc::get_vcp(monitor_id, code)?;
    let value = (i32::from(current) + offset).clamp(0, i32::from(maximum)) as u16;
    ddc::set_vcp(monitor_id, code, value)?;
    Ok(value)
}

#[derive(Debug)]
pub struct ActionExecutor<Job> {
    pending: VecDeque<Job>,
    active: bool,
}

impl<Job> Default for ActionExecutor<Job> {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            active: false,
        }
    }
}

impl<Job> ActionExecutor<Job> {
    pub fn extend(&mut self, jobs: impl IntoIterator<Item = Job>) {
        self.pending.extend(jobs);
    }

    pub fn start_next(&mut self) -> Option<Job> {
        if self.active {
            return None;
        }
        let job = self.pending.pop_front()?;
        self.active = true;
        Some(job)
    }

    pub fn complete_active(&mut self) {
        debug_assert!(
            self.active,
            "completed a hardware job while none was active"
        );
        self.active = false;
    }

    pub fn is_idle(&self) -> bool {
        !self.active && self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::ActionExecutor;

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
