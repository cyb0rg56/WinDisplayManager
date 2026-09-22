use super::*;
use crate::ccd::{
    PathSourceInfo, PathTargetInfo, Rational, apply_config_with, config_from_raw, mode_to_mirror,
    mode_to_raw, path_to_raw,
};
use windows_sys::Win32::Devices::Display::{
    DISPLAYCONFIG_MODE_INFO, SDC_APPLY, SDC_SAVE_TO_DATABASE, SDC_USE_SUPPLIED_DISPLAY_CONFIG,
    SDC_VALIDATE,
};

fn ep(low: u32, high: i32, id: u32) -> Endpoint {
    Endpoint {
        adapter: Luid { low, high },
        id,
    }
}

fn monitor(device_path: &str) -> MonitorInfo {
    MonitorInfo {
        valid: true,
        friendly_name: "Identical model".into(),
        device_path: device_path.into(),
        ..MonitorInfo::default()
    }
}

fn path(src: Endpoint, dst: Endpoint, active: bool) -> PathInfo {
    PathInfo {
        source: PathSourceInfo {
            adapter_id: src.adapter,
            id: src.id,
            mode_info_idx: NO_MODE,
            status_flags: 0,
        },
        target: PathTargetInfo {
            adapter_id: dst.adapter,
            id: dst.id,
            mode_info_idx: NO_MODE,
            output_technology: 5,
            rotation: 2,
            scaling: 3,
            refresh_rate: Rational {
                num: 60000,
                den: 1001,
            },
            scan_line_ordering: 1,
            target_available: 1,
            status_flags: 0,
        },
        flags: if active { DISPLAYCONFIG_PATH_ACTIVE } else { 0 },
    }
}

fn mode(endpoint: Endpoint, kind: i32) -> ModeInfo {
    let mut raw = DISPLAYCONFIG_MODE_INFO {
        infoType: kind,
        id: endpoint.id,
        adapterId: endpoint.adapter.into(),
        ..DISPLAYCONFIG_MODE_INFO::default()
    };
    if kind == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE {
        // Real source fields make resolution/position preservation meaningful.
        unsafe {
            let source = &mut raw.Anonymous.sourceMode;
            source.width = 1920;
            source.height = 1080;
            source.pixelFormat = 4;
            source.position.x = -1920;
            source.position.y = 120;
        }
    }
    mode_to_mirror(&raw)
}

fn saved(routes: &[(Endpoint, Endpoint, &str)]) -> DisplayConfig {
    let mut config = DisplayConfig {
        paths: Vec::new(),
        modes: Vec::new(),
        monitors: Vec::new(),
        path_monitors: Vec::new(),
    };
    let mut sources = HashMap::new();
    for &(src, dst, name) in routes {
        let mut path = path(src, dst, true);
        path.source.mode_info_idx = *sources.entry(src).or_insert_with(|| {
            let index = config.modes.len() as u32;
            config
                .modes
                .push(mode(src, DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE));
            config.monitors.push(MonitorInfo::default());
            index
        });
        path.target.mode_info_idx = config.modes.len() as u32;
        config
            .modes
            .push(mode(dst, DISPLAYCONFIG_MODE_INFO_TYPE_TARGET));
        config.monitors.push(monitor(name));
        config.paths.push(path);
        config.path_monitors.push(monitor(name));
    }
    config
}

fn live(routes: &[(Endpoint, Endpoint, &str, bool)]) -> DisplayConfig {
    // No modes at all: routing/identity must come exclusively from paths.
    DisplayConfig {
        paths: routes
            .iter()
            .map(|&(src, dst, _, active)| path(src, dst, active))
            .collect(),
        modes: Vec::new(),
        monitors: Vec::new(),
        path_monitors: routes
            .iter()
            .map(|&(_, _, name, _)| monitor(name))
            .collect(),
    }
}

fn json(config: &DisplayConfig) -> String {
    serde_json::to_string(config).unwrap()
}

fn rejects_before_set(saved: &DisplayConfig, live: &DisplayConfig, expected: &str) {
    let before = (json(saved), json(live));
    let err = apply_config_with(saved, live, |_, _, _| {
        panic!("SetDisplayConfig must not be called")
    })
    .unwrap_err();
    assert!(
        err.to_string().contains(expected),
        "{err}; expected {expected}"
    );
    assert_eq!(
        before,
        (json(saved), json(live)),
        "inputs changed on rejection"
    );
}

#[test]
fn two_gpus_with_identical_local_ids_and_low_halves_remap_independently() {
    let saved = saved(&[
        (ep(10, 1, 0), ep(10, 1, 7), "A"),
        (ep(10, 2, 0), ep(10, 2, 7), "B"),
    ]);
    let live = live(&[
        (ep(90, -3, 4), ep(90, -3, 9), "B", true),
        (ep(80, -4, 5), ep(80, -4, 8), "A", true),
    ]);
    let before = (json(&saved), json(&live));
    let result = remap_config(&saved, &live).unwrap();
    assert_eq!(source(&result.paths[0]), ep(80, -4, 5));
    assert_eq!(target(&result.paths[0]), ep(80, -4, 8));
    assert_eq!(source(&result.paths[1]), ep(90, -3, 4));
    assert_eq!(target(&result.paths[1]), ep(90, -3, 9));
    for (new, old) in result.modes.iter().zip(&saved.modes) {
        assert_eq!(new.payload, old.payload);
    }
    validate_layout(&result.paths, &result.modes).unwrap();
    assert_eq!(before, (json(&saved), json(&live)));
}

#[test]
fn changed_endpoint_ids_match_case_insensitive_device_paths_and_preserve_layout() {
    let saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), r"\\?\DISPLAY#MODEL#Instance-A")]);
    let live = live(&[(
        ep(2, 2, 11),
        ep(2, 2, 22),
        r"\\?\display#model#instance-a",
        true,
    )]);
    let result = remap_config(&saved, &live).unwrap();
    let p = &result.paths[0];
    assert_eq!(p.source.id, 11);
    assert_eq!(p.target.id, 22);
    assert_eq!(result.modes[p.source.mode_info_idx as usize].id, 11);
    assert_eq!(result.modes[p.target.mode_info_idx as usize].id, 22);
    assert_eq!(
        (
            p.target.rotation,
            p.target.scaling,
            p.target.refresh_rate.num,
            p.target.refresh_rate.den
        ),
        (2, 3, 60000, 1001)
    );
    assert_eq!(
        p.target.scan_line_ordering,
        saved.paths[0].target.scan_line_ordering
    );
    let raw = mode_to_raw(&result.modes[p.source.mode_info_idx as usize]).unwrap();
    let source = unsafe { raw.Anonymous.sourceMode };
    assert_eq!(
        (
            source.width,
            source.height,
            source.position.x,
            source.position.y
        ),
        (1920, 1080, -1920, 120)
    );
}

#[test]
fn moving_one_target_does_not_rewrite_other_targets_on_its_old_adapter() {
    let saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 10), "A"),
        (ep(1, 1, 1), ep(1, 1, 11), "B"),
    ]);
    let mut live = live(&[
        (ep(2, 8, 7), ep(2, 8, 20), "A", true),
        (ep(1, 1, 1), ep(1, 1, 11), "B", true),
    ]);
    live.paths[0].target.output_technology = 10;
    let result = remap_config(&saved, &live).unwrap();
    assert_eq!(target(&result.paths[0]), ep(2, 8, 20));
    assert_eq!(source(&result.paths[1]), source(&saved.paths[1]));
    assert_eq!(target(&result.paths[1]), target(&saved.paths[1]));
    assert_eq!(result.paths[0].target.output_technology, 10);
    assert_eq!(result.modes[3].adapter_id, saved.modes[3].adapter_id);
}

#[test]
fn identical_model_names_never_override_distinct_instance_paths() {
    let saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 10), "MODEL#A"),
        (ep(1, 1, 1), ep(1, 1, 11), "MODEL#B"),
    ]);
    let live = live(&[
        (ep(2, 2, 0), ep(2, 2, 10), "MODEL#B", true),
        (ep(2, 2, 1), ep(2, 2, 11), "MODEL#A", true),
    ]);
    let result = remap_config(&saved, &live).unwrap();
    assert_eq!(result.paths[0].target.id, 11);
    assert_eq!(result.paths[1].target.id, 10);
}

#[test]
fn query_collects_inactive_identities_without_modes_once_per_qualified_target() {
    let a = path(ep(20, 3, 4), ep(20, 3, 7), false);
    let b = path(ep(20, 4, 4), ep(20, 4, 7), false);
    let raw = [path_to_raw(&a), path_to_raw(&a), path_to_raw(&b)];
    let mut requests = Vec::new();
    let live = config_from_raw(&raw, &[], |adapter, id| {
        requests.push((Luid::from(adapter), id));
        monitor(if adapter.HighPart == 3 { "A" } else { "B" })
    });
    assert_eq!(
        requests,
        vec![(a.target.adapter_id, 7), (b.target.adapter_id, 7)]
    );
    assert!(live.modes.is_empty());
    let saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 1), "A"),
        (ep(1, 2, 0), ep(1, 2, 1), "B"),
    ]);
    let result = remap_config(&saved, &live).unwrap();
    assert_eq!(source(&result.paths[0]), source(&a));
    assert_eq!(target(&result.paths[1]), target(&b));
    assert!(
        result
            .paths
            .iter()
            .all(|p| p.flags == DISPLAYCONFIG_PATH_ACTIVE)
    );
}

#[test]
fn new_per_path_metadata_handles_saved_missing_target_mode() {
    let mut saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    saved.paths[0].target.mode_info_idx = NO_MODE;
    saved.modes.pop();
    saved.monitors.pop();
    let raw_paths = saved.paths.iter().map(path_to_raw).collect::<Vec<_>>();
    let raw_modes = saved
        .modes
        .iter()
        .map(|m| mode_to_raw(m).unwrap())
        .collect::<Vec<_>>();
    let mut saved = config_from_raw(&raw_paths, &raw_modes, |_, _| monitor("A"));
    assert_eq!(saved.path_monitors[0].device_path, "A");
    assert!(!saved.monitors[0].valid);
    let live = live(&[(ep(2, 2, 9), ep(2, 2, 10), "A", false)]);
    let result = remap_config(&saved, &live).unwrap();
    assert_eq!(result.paths[0].target.mode_info_idx, NO_MODE);
    assert_eq!(result.modes.len(), 1);
    let roundtrip: DisplayConfig = serde_json::from_str(&json(&result)).unwrap();
    assert_eq!(roundtrip.path_monitors[0].device_path, "A");
    saved.path_monitors.clear();
    rejects_before_set(&saved, &live, "recaptured");
}

#[test]
fn legacy_json_uses_referenced_target_mode_identity_without_schema_break() {
    let mut saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    saved.path_monitors.clear();
    let text = json(&saved);
    assert!(!text.contains("path_monitors"));
    let legacy: DisplayConfig = serde_json::from_str(&text).unwrap();
    assert!(legacy.path_monitors.is_empty());
    let live = live(&[(ep(2, 2, 9), ep(2, 2, 10), "A", true)]);
    assert!(remap_config(&legacy, &live).is_ok());
    saved.monitors[1].device_path.clear();
    rejects_before_set(&saved, &live, "recaptured");
}

#[test]
fn missing_or_unreadable_live_identity_does_not_fallback_to_ids_or_model() {
    let saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    for identity in ["", "B", "A#other-instance", " A "] {
        let live = live(&[(ep(1, 1, 0), ep(1, 1, 1), identity, true)]);
        rejects_before_set(&saved, &live, "missing");
    }
    let mut live = live(&[(ep(1, 1, 0), ep(1, 1, 1), "A", true)]);
    live.path_monitors[0].valid = false;
    rejects_before_set(&saved, &live, "missing");
    live.path_monitors[0].valid = true;
    live.paths[0].target.target_available = 0;
    rejects_before_set(&saved, &live, "missing");
}

#[test]
fn duplicate_live_identity_is_ambiguous_even_with_same_ids_on_two_gpus() {
    let saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    let live = live(&[
        (ep(2, 2, 0), ep(2, 2, 1), "A", true),
        (ep(2, 3, 0), ep(2, 3, 1), "a", true),
    ]);
    rejects_before_set(&saved, &live, "multiple live targets");
}

#[test]
fn conflicting_metadata_and_duplicate_saved_identity_are_rejected() {
    let mut saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 1), "A"),
        (ep(1, 1, 2), ep(1, 1, 3), "B"),
    ]);
    let live = live(&[
        (ep(2, 2, 0), ep(2, 2, 1), "A", true),
        (ep(2, 2, 2), ep(2, 2, 3), "B", true),
    ]);
    saved.path_monitors[0] = monitor("B");
    rejects_before_set(&saved, &live, "conflicting");
    saved.monitors[1] = monitor("B");
    rejects_before_set(&saved, &live, "multiple saved targets");
    saved.path_monitors.pop();
    rejects_before_set(&saved, &live, "metadata length");
}

#[test]
fn ambiguous_inactive_source_routes_never_choose_first() {
    let saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    let mut live = live(&[
        (ep(2, 2, 5), ep(2, 2, 7), "A", false),
        (ep(2, 2, 6), ep(2, 2, 7), "A", false),
    ]);
    rejects_before_set(&saved, &live, "ambiguous");
    live.paths.reverse();
    rejects_before_set(&saved, &live, "ambiguous");
}

#[test]
fn active_route_not_enumeration_order_disambiguates_alternatives() {
    let saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    let mut live = live(&[
        (ep(2, 2, 5), ep(2, 2, 7), "A", false),
        (ep(2, 2, 6), ep(2, 2, 7), "A", true),
    ]);
    assert_eq!(
        source(&remap_config(&saved, &live).unwrap().paths[0]),
        ep(2, 2, 6)
    );
    live.paths.reverse();
    assert_eq!(
        source(&remap_config(&saved, &live).unwrap().paths[0]),
        ep(2, 2, 6)
    );
}

#[test]
fn exact_original_source_is_qualified_by_both_luid_halves() {
    let saved = saved(&[(ep(1, 7, 0), ep(1, 7, 1), "A")]);
    let mut live = live(&[
        (ep(1, 8, 0), ep(1, 7, 9), "A", true),
        (ep(1, 7, 0), ep(1, 7, 9), "A", false),
    ]);
    assert_eq!(
        source(&remap_config(&saved, &live).unwrap().paths[0]),
        ep(1, 7, 0)
    );
    live.paths[1].source.adapter_id.high = 9;
    // The matching low half alone must not override the sole active route.
    assert_eq!(
        source(&remap_config(&saved, &live).unwrap().paths[0]),
        ep(1, 8, 0)
    );
}

#[test]
fn clone_group_uses_common_source_and_one_shared_mode() {
    let saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 1), "A"),
        (ep(1, 1, 0), ep(1, 1, 2), "B"),
    ]);
    let live = live(&[
        (ep(2, 2, 3), ep(2, 2, 10), "A", false),
        (ep(2, 2, 4), ep(2, 2, 10), "A", false),
        (ep(2, 2, 4), ep(2, 2, 11), "B", false),
        (ep(2, 2, 5), ep(2, 2, 11), "B", false),
    ]);
    let result = remap_config(&saved, &live).unwrap();
    assert!(result.paths.iter().all(|p| source(p) == ep(2, 2, 4)));
    assert_eq!(
        result.paths[0].source.mode_info_idx,
        result.paths[1].source.mode_info_idx
    );
    assert_eq!(result.modes.len(), 3);
    assert_eq!(result.modes[0].payload, saved.modes[0].payload);
}

#[test]
fn incompatible_clone_routes_are_rejected() {
    let saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 1), "A"),
        (ep(1, 1, 0), ep(1, 1, 2), "B"),
    ]);
    let live = live(&[
        (ep(2, 2, 0), ep(2, 2, 1), "A", true),
        (ep(3, 3, 0), ep(3, 3, 2), "B", true),
    ]);
    rejects_before_set(&saved, &live, "clone/extended");
}

#[test]
fn multiple_common_clone_sources_are_ambiguous_not_selected_by_path_order() {
    let saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 1), "A"),
        (ep(1, 1, 0), ep(1, 1, 2), "B"),
    ]);
    let live = live(&[
        (ep(2, 2, 3), ep(2, 2, 10), "A", true),
        (ep(2, 2, 4), ep(2, 2, 10), "A", false),
        (ep(2, 2, 3), ep(2, 2, 11), "B", false),
        (ep(2, 2, 4), ep(2, 2, 11), "B", true),
    ]);
    rejects_before_set(&saved, &live, "ambiguous");
}

#[test]
fn separate_saved_sources_cannot_silently_become_clones() {
    let saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 1), "A"),
        (ep(1, 1, 2), ep(1, 1, 3), "B"),
    ]);
    let live = live(&[
        (ep(2, 2, 0), ep(2, 2, 10), "A", true),
        (ep(2, 2, 0), ep(2, 2, 11), "B", true),
    ]);
    rejects_before_set(&saved, &live, "clone/extended");
}

#[test]
fn unique_source_assignment_can_be_deduced_from_other_saved_groups() {
    let saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 1), "A"),
        (ep(1, 1, 2), ep(1, 1, 3), "B"),
    ]);
    let live = live(&[
        (ep(2, 2, 0), ep(2, 2, 10), "A", false),
        (ep(2, 2, 1), ep(2, 2, 10), "A", false),
        (ep(2, 2, 0), ep(2, 2, 11), "B", false),
    ]);
    let result = remap_config(&saved, &live).unwrap();
    assert_eq!(source(&result.paths[0]), ep(2, 2, 1));
    assert_eq!(source(&result.paths[1]), ep(2, 2, 0));
}

#[test]
fn rebuilding_uses_indices_not_mode_order_and_drops_unused_modes() {
    let mut saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    saved.modes.swap(0, 1);
    saved.monitors.swap(0, 1);
    saved.paths[0].source.mode_info_idx = 1;
    saved.paths[0].target.mode_info_idx = 0;
    saved
        .modes
        .push(mode(ep(99, 99, 0), DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE));
    saved.monitors.push(MonitorInfo::default());
    saved.path_monitors.clear();
    let live = live(&[(ep(2, 2, 10), ep(2, 2, 11), "A", true)]);
    let result = remap_config(&saved, &live).unwrap();
    assert_eq!(result.modes.len(), 2);
    assert_eq!(result.modes[0].payload, saved.modes[1].payload);
    assert_eq!(result.paths[0].source.mode_info_idx, 0);
    assert_eq!(result.paths[0].target.mode_info_idx, 1);
    assert_eq!(result.monitors[1].device_path, "A");
}

#[test]
fn malformed_payload_lengths_and_types_are_rejected_before_ffi() {
    let original = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    let live = live(&[(ep(2, 2, 0), ep(2, 2, 1), "A", true)]);
    let size = std::mem::size_of::<DISPLAYCONFIG_MODE_INFO_0>();
    for len in [0, size - 1, size + 1] {
        let mut bad = original.clone();
        bad.modes[0].payload.resize(len, 0);
        rejects_before_set(&bad, &live, "payload length");
        assert!(mode_to_raw(&bad.modes[0]).is_err());
    }
    for kind in [0, 3, 4, -1] {
        let mut bad = original.clone();
        bad.modes[1].info_type = kind;
        rejects_before_set(&bad, &live, "unsupported mode type");
        assert!(mode_to_raw(&bad.modes[1]).is_err());
    }
}

#[test]
fn malformed_indices_endpoint_types_and_virtual_flags_are_rejected() {
    let original = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    let live = live(&[(ep(2, 2, 0), ep(2, 2, 1), "A", true)]);
    for index in [2, NO_MODE] {
        let mut bad = original.clone();
        bad.paths[0].source.mode_info_idx = index;
        rejects_before_set(&bad, &live, "source mode index");
    }
    let mut bad = original.clone();
    bad.paths[0].target.mode_info_idx = 2;
    rejects_before_set(&bad, &live, "target mode index");
    bad = original.clone();
    bad.paths[0].source.mode_info_idx = 1;
    rejects_before_set(&bad, &live, "endpoint and type");
    bad = original.clone();
    bad.modes[0].adapter_id.high += 1;
    rejects_before_set(&bad, &live, "endpoint and type");
    bad = original.clone();
    bad.modes[1].id += 1;
    rejects_before_set(&bad, &live, "endpoint and type");
    for flags in [0, 9, 17] {
        bad = original.clone();
        bad.paths[0].flags = flags;
        rejects_before_set(&bad, &live, "unsupported path flags");
    }
    bad = original.clone();
    bad.paths.push(bad.paths[0]);
    rejects_before_set(&bad, &live, "repeats a target");
    bad.paths.clear();
    rejects_before_set(&bad, &live, "no display paths");
}

#[test]
fn inconsistent_saved_clone_modes_are_rejected() {
    let mut saved = saved(&[
        (ep(1, 1, 0), ep(1, 1, 1), "A"),
        (ep(1, 1, 0), ep(1, 1, 2), "B"),
    ]);
    let live = live(&[
        (ep(2, 2, 0), ep(2, 2, 1), "A", true),
        (ep(2, 2, 0), ep(2, 2, 2), "B", true),
    ]);
    let mut duplicate = saved.modes[0].clone();
    duplicate.payload[0] ^= 1;
    saved.paths[1].source.mode_info_idx = saved.modes.len() as u32;
    saved.modes.push(duplicate);
    rejects_before_set(&saved, &live, "duplicate adapter-qualified mode");
}

#[test]
fn inconsistent_live_records_and_metadata_lengths_are_rejected() {
    let saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    let mut live = live(&[
        (ep(2, 2, 0), ep(2, 2, 1), "A", true),
        (ep(2, 2, 2), ep(2, 2, 1), "B", false),
    ]);
    rejects_before_set(&saved, &live, "disagree");
    live.path_monitors.pop();
    rejects_before_set(&saved, &live, "incomplete");
}

#[test]
fn validate_precedes_apply_with_exact_flags_and_identical_payloads() {
    let saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    let live = live(&[(ep(2, 2, 9), ep(2, 2, 10), "A", true)]);
    let mut flags_seen = Vec::new();
    let mut snapshots = Vec::new();
    apply_config_with(&saved, &live, |paths, modes, flags| {
        flags_seen.push(flags);
        let mirror = config_from_raw(paths, modes, |_, _| monitor("A"));
        snapshots.push(json(&mirror));
        assert_eq!(paths[0].sourceInfo.id, 9);
        assert_eq!(paths[0].targetInfo.adapterId.HighPart, 2);
        0
    })
    .unwrap();
    assert_eq!(
        flags_seen,
        [
            SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_VALIDATE,
            SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_APPLY | SDC_SAVE_TO_DATABASE
        ]
    );
    assert_eq!(snapshots[0], snapshots[1]);
}

#[test]
fn windows_validation_failure_stops_before_apply_and_apply_failure_has_no_fallback() {
    let saved = saved(&[(ep(1, 1, 0), ep(1, 1, 1), "A")]);
    let live = live(&[(ep(2, 2, 9), ep(2, 2, 10), "A", true)]);
    let mut calls = 0;
    let error = apply_config_with(&saved, &live, |_, _, _| {
        calls += 1;
        87
    })
    .unwrap_err();
    assert!(matches!(error, CcdError::Validate(87)));
    assert_eq!(calls, 1);
    calls = 0;
    let error = apply_config_with(&saved, &live, |_, _, _| {
        calls += 1;
        if calls == 1 { 0 } else { 31 }
    })
    .unwrap_err();
    assert!(matches!(error, CcdError::Apply(31)));
    assert_eq!(calls, 2);
}
