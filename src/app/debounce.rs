use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum SliderFeature {
    Brightness,
    Contrast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebounceToken {
    generation: u64,
    revision: u64,
}

#[derive(Debug)]
struct PendingChange {
    token: DebounceToken,
    value: u16,
}

/// Drafts are separate from both confirmed readings and submitted hardware jobs.
#[derive(Debug, Default)]
pub(super) struct SliderDebounce {
    revision: u64,
    pending: HashMap<(u32, SliderFeature), PendingChange>,
}

impl SliderDebounce {
    pub(super) fn record(
        &mut self,
        generation: u64,
        monitor_id: u32,
        feature: SliderFeature,
        value: u16,
    ) -> DebounceToken {
        // Never wrap or reset this counter: an old timer may outlive invalidation.
        self.revision = self
            .revision
            .checked_add(1)
            .expect("slider revision exhausted");
        let token = DebounceToken {
            generation,
            revision: self.revision,
        };
        self.pending
            .insert((monitor_id, feature), PendingChange { token, value });
        token
    }

    pub(super) fn value(&self, monitor_id: u32, feature: SliderFeature) -> Option<u16> {
        self.pending
            .get(&(monitor_id, feature))
            .map(|change| change.value)
    }

    /// Consume only the latest edit for this key, at most once per timer token.
    pub(super) fn take_current(
        &mut self,
        generation: u64,
        monitor_id: u32,
        feature: SliderFeature,
        token: DebounceToken,
    ) -> Option<u16> {
        let key = (monitor_id, feature);
        if token.generation != generation || self.pending.get(&key)?.token != token {
            return None;
        }
        self.pending.remove(&key).map(|change| change.value)
    }

    pub(super) fn invalidate(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SliderFeature::{Brightness, Contrast};

    #[test]
    fn monitors_and_features_debounce_independently() {
        let mut drafts = SliderDebounce::default();
        let keys = [
            (1, Brightness, 20),
            (2, Brightness, 30),
            (1, Contrast, 40),
            (2, Contrast, 50),
        ];
        let tokens = keys.map(|(id, feature, value)| drafts.record(7, id, feature, value));
        for &(id, feature, value) in &keys {
            assert_eq!(drafts.value(id, feature), Some(value));
        }
        // A token cannot be applied to another monitor or feature.
        assert_eq!(drafts.take_current(7, 2, Brightness, tokens[0]), None);
        assert_eq!(drafts.take_current(7, 1, Contrast, tokens[0]), None);
        // Delivery order across keys need not match edit order.
        for index in [1, 2, 0, 3] {
            let (id, feature, value) = keys[index];
            assert_eq!(
                drafts.take_current(7, id, feature, tokens[index]),
                Some(value)
            );
            assert_eq!(drafts.value(id, feature), None);
            assert_eq!(drafts.take_current(7, id, feature, tokens[index]), None);
        }
    }

    #[test]
    fn repeated_values_do_not_allow_an_older_timer_to_apply_early() {
        for feature in [Brightness, Contrast] {
            let mut drafts = SliderDebounce::default();
            let first = drafts.record(7, 1, feature, 20);
            let middle = drafts.record(7, 1, feature, 30);
            let latest = drafts.record(7, 1, feature, 20);
            assert!(first.revision < middle.revision && middle.revision < latest.revision);
            assert_eq!(drafts.take_current(7, 1, feature, first), None);
            assert_eq!(drafts.take_current(7, 1, feature, middle), None);
            assert_eq!(drafts.value(1, feature), Some(20));
            assert_eq!(drafts.take_current(7, 1, feature, latest), Some(20));
            assert_eq!(drafts.take_current(7, 1, feature, latest), None);
        }
    }

    #[test]
    fn consumed_tokens_cannot_consume_a_later_equal_edit() {
        let mut drafts = SliderDebounce::default();
        let first = drafts.record(7, 1, Brightness, 20);
        assert_eq!(drafts.take_current(7, 1, Brightness, first), Some(20));
        let second = drafts.record(7, 1, Brightness, 20);
        assert!(second.revision > first.revision);
        assert_eq!(drafts.take_current(7, 1, Brightness, first), None);
        assert_eq!(drafts.take_current(7, 1, Brightness, second), Some(20));
        assert_eq!(drafts.take_current(7, 1, Brightness, second), None);
    }

    #[test]
    fn invalidation_clears_every_draft_without_reusing_revisions() {
        let mut drafts = SliderDebounce::default();
        let keys = [
            (1, Brightness),
            (2, Brightness),
            (1, Contrast),
            (2, Contrast),
        ];
        let old = keys.map(|(id, feature)| drafts.record(7, id, feature, 20));
        drafts.invalidate();
        for (index, &(id, feature)) in keys.iter().enumerate() {
            assert_eq!(drafts.value(id, feature), None);
            assert_eq!(drafts.take_current(7, id, feature, old[index]), None);
        }
        // Repeated clears, even without a new topology generation, cannot revive timers.
        drafts.invalidate();
        let same_generation = drafts.record(7, 1, Brightness, 20);
        assert!(same_generation.revision > old[3].revision);
        assert_eq!(drafts.take_current(7, 1, Brightness, old[0]), None);
        drafts.invalidate();
        let new_generation = drafts.record(8, 1, Brightness, 20);
        assert_eq!(new_generation.generation, 8);
        assert!(new_generation.revision > same_generation.revision);
        assert_eq!(drafts.take_current(8, 1, Brightness, old[0]), None);
        assert_eq!(drafts.take_current(8, 1, Brightness, same_generation), None);
        assert_eq!(
            drafts.take_current(8, 1, Brightness, new_generation),
            Some(20)
        );
    }

    #[test]
    fn topology_generation_must_match_even_if_the_draft_token_matches() {
        let mut drafts = SliderDebounce::default();
        let token = drafts.record(7, 1, Brightness, 20);
        assert_eq!(drafts.take_current(8, 1, Brightness, token), None);
        assert_eq!(drafts.value(1, Brightness), Some(20));
        drafts.invalidate();
        assert_eq!(drafts.value(1, Brightness), None);
    }
}
