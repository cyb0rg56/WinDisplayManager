//! View models derived from state; widget code lives in the UI crate.

use crate::actions::HardwareJob;
use crate::ddc::InputSource;
use crate::debounce::{SliderDebounce, SliderFeature};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PendingValue<T> {
    #[default]
    None,
    Set(T),
    Adjusting,
}

#[derive(Default)]
pub struct MonitorPending {
    pub brightness: PendingValue<u16>,
    pub contrast: PendingValue<u16>,
    pub input_source: PendingValue<InputSource>,
}

impl MonitorPending {
    pub fn new<'a>(
        monitor_id: u32,
        jobs: impl Iterator<Item = &'a HardwareJob>,
        drafts: &SliderDebounce,
    ) -> Self {
        let mut pending = Self::default();
        for job in jobs {
            match *job {
                HardwareJob::SetBrightness {
                    monitor_id: id,
                    value,
                } if id == monitor_id => {
                    pending.brightness = PendingValue::Set(value);
                }
                HardwareJob::OffsetBrightness { monitor_id: id, .. } if id == monitor_id => {
                    pending.brightness = PendingValue::Adjusting;
                }
                HardwareJob::SetContrast {
                    monitor_id: id,
                    value,
                } if id == monitor_id => {
                    pending.contrast = PendingValue::Set(value);
                }
                HardwareJob::OffsetContrast { monitor_id: id, .. } if id == monitor_id => {
                    pending.contrast = PendingValue::Adjusting;
                }
                HardwareJob::SetInputSource {
                    monitor_id: id,
                    source,
                } if id == monitor_id => {
                    pending.input_source = PendingValue::Set(source);
                }
                _ => {}
            }
        }
        // A draft is newer than submitted writes, even while an older one completes.
        if let Some(value) = drafts.value(monitor_id, SliderFeature::Brightness) {
            pending.brightness = PendingValue::Set(value);
        }
        if let Some(value) = drafts.value(monitor_id, SliderFeature::Contrast) {
            pending.contrast = PendingValue::Set(value);
        }
        pending
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ControlPresentation<'a, T> {
    pub value: Option<T>,
    pub label: String,
    pub read_error: Option<&'a str>,
}

fn control_presentation<T: Copy>(
    value: T,
    read_error: Option<&str>,
    pending: PendingValue<T>,
    format: impl Fn(T) -> String,
) -> ControlPresentation<'_, T> {
    let confirmed = read_error.is_none().then_some(value);
    let (value, label) = match pending {
        PendingValue::Set(target) => (Some(target), format!("{} (pending)", format(target))),
        PendingValue::Adjusting => (
            confirmed,
            format!(
                "{} (change pending)",
                confirmed.map(&format).unwrap_or("Unknown".into())
            ),
        ),
        PendingValue::None => (
            confirmed,
            confirmed.map(&format).unwrap_or("Unknown".into()),
        ),
    };
    ControlPresentation {
        value,
        label,
        read_error,
    }
}

pub fn scalar_presentation(
    value: u16,
    maximum: u16,
    read_error: Option<&str>,
    pending: PendingValue<u16>,
) -> ControlPresentation<'_, u16> {
    let mut presentation = control_presentation(value, read_error, pending, |value| {
        if read_error.is_some() {
            format!("Unknown; requested {value}")
        } else {
            format!("{value} / {maximum}")
        }
    });
    // A write does not tell us the range. Never offer a slider over a guessed range.
    if read_error.is_some() {
        presentation.value = None;
    }
    presentation
}

pub fn input_presentation(
    source: InputSource,
    read_error: Option<&str>,
    pending: PendingValue<InputSource>,
) -> ControlPresentation<'_, InputSource> {
    control_presentation(source, read_error, pending, |source| source.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ActionExecutor;
    use crate::ddc::STANDARD_INPUT_SOURCES;
    use crate::ddc::test_util::monitor_state;

    fn input_source_index(source: &InputSource) -> Option<usize> {
        STANDARD_INPUT_SOURCES.iter().position(|s| s == source)
    }

    #[test]
    fn partial_read_failure_hides_only_failed_slider_and_can_recover() {
        let mut monitor = monitor_state();
        monitor.brightness = 0;
        monitor.brightness_read_error = Some("timeout".into());
        let brightness = scalar_presentation(
            monitor.brightness,
            monitor.brightness_max,
            monitor.brightness_read_error.as_deref(),
            PendingValue::None,
        );
        assert_eq!(brightness.value, None);
        assert_eq!(brightness.label, "Unknown");
        assert_eq!(brightness.read_error, Some("timeout"));

        let contrast = scalar_presentation(
            monitor.contrast,
            monitor.contrast_max,
            monitor.contrast_read_error.as_deref(),
            PendingValue::None,
        );
        assert_eq!(contrast.value, Some(120));
        assert_eq!(contrast.label, "120 / 255");
        let input = input_presentation(
            monitor.input_source,
            monitor.input_source_read_error.as_deref(),
            PendingValue::None,
        );
        assert_eq!(input.value.as_ref().and_then(input_source_index), Some(0));

        // A later valid read can recover, including an actual zero reading.
        monitor.brightness_read_error = None;
        let recovered = scalar_presentation(
            monitor.brightness,
            monitor.brightness_max,
            monitor.brightness_read_error.as_deref(),
            PendingValue::None,
        );
        assert_eq!(recovered.value, Some(0));
        assert_eq!(recovered.label, "0 / 100");
        assert_eq!(recovered.read_error, None);
    }

    #[test]
    fn failed_input_read_is_not_a_confirmed_source_and_selection_stays_available() {
        let unknown =
            input_presentation(InputSource::Custom(0), Some("timeout"), PendingValue::None);
        assert_eq!(unknown.value, None);
        assert_eq!(unknown.label, "Unknown");
        assert_eq!(unknown.read_error, Some("timeout"));

        let pending = input_presentation(
            InputSource::Custom(0),
            Some("timeout"),
            PendingValue::Set(InputSource::Dp1),
        );
        assert_eq!(pending.value.as_ref().and_then(input_source_index), Some(2));
        assert_eq!(pending.label, "DisplayPort 1 (pending)");
        assert_eq!(pending.read_error, Some("timeout"));

        let custom = input_presentation(InputSource::Custom(0x42), None, PendingValue::None);
        assert_eq!(custom.label, "Custom (0x42)");
        assert_eq!(custom.value, Some(InputSource::Custom(0x42)));
    }

    #[test]
    fn input_source_decoded_sl_selects_the_standard_dropdown_option() {
        for (raw, source, index, label) in [
            (0x0F11, InputSource::Hdmi1, 0, "HDMI 1"),
            (0x1111, InputSource::Hdmi1, 0, "HDMI 1"),
            (0xAB12, InputSource::Hdmi2, 1, "HDMI 2"),
            (0x110F, InputSource::Dp1, 2, "DisplayPort 1"),
            (0x0F0F, InputSource::Dp1, 2, "DisplayPort 1"),
            (0x1010, InputSource::Dp2, 3, "DisplayPort 2"),
        ] {
            let input = input_presentation(
                crate::ddc::decode_input_source(raw),
                None,
                PendingValue::None,
            );
            assert_eq!(input.value, Some(source));
            assert_eq!(
                input.value.as_ref().and_then(input_source_index),
                Some(index)
            );
            assert_eq!(STANDARD_INPUT_SOURCES[index], source);
            assert_eq!(input.label, label);
            assert_eq!(input.read_error, None);
        }
    }

    #[test]
    fn input_source_unknown_sl_does_not_select_a_known_dropdown_option() {
        for (raw, sl, label) in [
            (0x1100, 0x00, "Custom (0x00)"),
            (0x0FEE, 0xEE, "Custom (0xEE)"),
            (0x11FF, 0xFF, "Custom (0xFF)"),
        ] {
            let input = input_presentation(
                crate::ddc::decode_input_source(raw),
                None,
                PendingValue::None,
            );
            assert_eq!(input.value, Some(InputSource::Custom(sl)));
            assert_eq!(input.value.as_ref().and_then(input_source_index), None);
            assert_eq!(input.label, label);
            assert_eq!(input.read_error, None);
        }
    }

    #[test]
    fn pending_scalar_write_does_not_expose_a_guessed_range_or_clear_error() {
        let pending = scalar_presentation(0, 100, Some("timeout"), PendingValue::Set(80));
        assert_eq!(pending.value, None);
        assert_eq!(pending.label, "Unknown; requested 80 (pending)");
        assert_eq!(pending.read_error, Some("timeout"));
    }

    #[test]
    fn draft_overlays_active_and_queued_writes_without_crossing_monitors() {
        let mut executor = ActionExecutor::default();
        executor.extend([
            HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 70,
            },
            HardwareJob::SetBrightness {
                monitor_id: 1,
                value: 80,
            },
            HardwareJob::SetBrightness {
                monitor_id: 2,
                value: 10,
            },
            HardwareJob::OffsetContrast {
                monitor_id: 1,
                offset: 5,
            },
        ]);
        executor.start_next().unwrap();
        let mut drafts = SliderDebounce::default();
        drafts.record(7, 1, SliderFeature::Brightness, 90);
        drafts.record(7, 2, SliderFeature::Contrast, 20);
        let draft = MonitorPending::new(1, executor.outstanding(), &drafts);
        assert_eq!(draft.brightness, PendingValue::Set(90));
        assert_eq!(draft.contrast, PendingValue::Adjusting);
        assert_eq!(draft.input_source, PendingValue::None);
        let preview = scalar_presentation(40, 100, None, draft.brightness);
        assert_eq!(preview.value, Some(90));
        assert_eq!(preview.label, "90 / 100 (pending)");
        let offset = scalar_presentation(120, 255, None, draft.contrast);
        assert_eq!(offset.value, Some(120));
        assert_eq!(offset.label, "120 / 255 (change pending)");

        let other_draft = MonitorPending::new(2, executor.outstanding(), &drafts);
        assert_eq!(other_draft.brightness, PendingValue::Set(10));
        assert_eq!(other_draft.contrast, PendingValue::Set(20));

        executor.complete_active();
        let still_pending = MonitorPending::new(1, executor.outstanding(), &drafts);
        assert_eq!(still_pending.brightness, PendingValue::Set(90));
        drafts.invalidate();
        let queued = MonitorPending::new(1, executor.outstanding(), &drafts);
        assert_eq!(queued.brightness, PendingValue::Set(80));
        let other = MonitorPending::new(2, executor.outstanding(), &drafts);
        assert_eq!(other.brightness, PendingValue::Set(10));
        assert_eq!(other.contrast, PendingValue::None);
    }
}
