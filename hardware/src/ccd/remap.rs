//! Pure, fail-closed reconstruction of a saved layout on a live CCD snapshot.

use super::{CcdError, DisplayConfig, Luid, ModeInfo, MonitorInfo, PathInfo, Result};
use std::collections::{HashMap, HashSet};
use windows_sys::Win32::Devices::Display::{
    DISPLAYCONFIG_MODE_INFO_0, DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE,
    DISPLAYCONFIG_MODE_INFO_TYPE_TARGET,
};
use windows_sys::Win32::Graphics::Gdi::DISPLAYCONFIG_PATH_ACTIVE;

const NO_MODE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Endpoint {
    adapter: Luid,
    id: u32,
}

fn source(path: &PathInfo) -> Endpoint {
    Endpoint {
        adapter: path.source.adapter_id,
        id: path.source.id,
    }
}

fn target(path: &PathInfo) -> Endpoint {
    Endpoint {
        adapter: path.target.adapter_id,
        id: path.target.id,
    }
}

fn invalid(reason: impl Into<String>) -> CcdError {
    CcdError::InvalidConfig(reason.into())
}

pub(super) fn validate_mode(mode: &ModeInfo) -> Result<()> {
    if mode.payload.len() != std::mem::size_of::<DISPLAYCONFIG_MODE_INFO_0>() {
        return Err(invalid(format!(
            "mode {} has an incorrect payload length",
            mode.id
        )));
    }
    if !matches!(
        mode.info_type,
        DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE | DISPLAYCONFIG_MODE_INFO_TYPE_TARGET
    ) {
        return Err(invalid(format!(
            "unsupported mode type {} (virtual/desktop-image modes are not supported)",
            mode.info_type
        )));
    }
    Ok(())
}

/// Validate the legacy, non-virtual index representation before interpreting it
/// or constructing any Win32 arrays. Windows validates the actual mode timings.
pub(super) fn validate_layout(paths: &[PathInfo], modes: &[ModeInfo]) -> Result<()> {
    if paths.is_empty() {
        return Err(CcdError::Empty);
    }
    if paths.len() > u32::MAX as usize || modes.len() > u32::MAX as usize {
        return Err(invalid("too many paths or modes"));
    }
    let mut mode_keys = HashSet::new();
    for mode in modes {
        validate_mode(mode)?;
        if !mode_keys.insert((mode.info_type, mode.adapter_id, mode.id)) {
            return Err(invalid("duplicate adapter-qualified mode endpoint"));
        }
    }
    let mut sources = HashMap::new();
    let mut targets = HashSet::new();
    for (i, path) in paths.iter().enumerate() {
        // Virtual-aware paths use packed 16-bit indices, not modeInfoIdx.
        if path.flags != DISPLAYCONFIG_PATH_ACTIVE {
            return Err(invalid(format!(
                "path {} is inactive or uses unsupported path flags",
                i + 1
            )));
        }
        if !targets.insert(target(path)) {
            return Err(invalid(format!("path {} repeats a target endpoint", i + 1)));
        }
        for (endpoint, index, kind) in [
            (
                source(path),
                path.source.mode_info_idx,
                DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE,
            ),
            (
                target(path),
                path.target.mode_info_idx,
                DISPLAYCONFIG_MODE_INFO_TYPE_TARGET,
            ),
        ] {
            if index == NO_MODE && kind == DISPLAYCONFIG_MODE_INFO_TYPE_TARGET {
                continue;
            }
            let mode = modes.get(index as usize).ok_or_else(|| {
                invalid(format!(
                    "path {} has a missing or out-of-range {} mode index",
                    i + 1,
                    if kind == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE {
                        "source"
                    } else {
                        "target"
                    }
                ))
            })?;
            if mode.info_type != kind
                || mode.adapter_id != endpoint.adapter
                || mode.id != endpoint.id
            {
                return Err(invalid(format!(
                    "path {} mode index does not reference its adapter-qualified endpoint and type",
                    i + 1
                )));
            }
        }
        if let Some(index) = sources.insert(source(path), path.source.mode_info_idx)
            && index != path.source.mode_info_idx
        {
            return Err(invalid(
                "cloned paths must reference the same source mode index",
            ));
        }
    }
    Ok(())
}

fn identity(monitor: &MonitorInfo) -> Option<String> {
    // Do not strip instance/connector suffixes or use model names as identity.
    (monitor.valid && !monitor.device_path.trim().is_empty() && !monitor.device_path.contains('\0'))
        .then(|| monitor.device_path.to_ascii_lowercase())
}

fn saved_monitor(saved: &DisplayConfig, index: usize) -> Result<MonitorInfo> {
    let per_path = saved
        .path_monitors
        .get(index)
        .filter(|m| identity(m).is_some());
    let legacy = saved
        .monitors
        .get(saved.paths[index].target.mode_info_idx as usize)
        .filter(|m| identity(m).is_some());
    if let (Some(a), Some(b)) = (per_path, legacy)
        && identity(a) != identity(b)
    {
        return Err(invalid(format!(
            "path {} has conflicting saved monitor identities",
            index + 1
        )));
    }
    per_path.or(legacy).cloned().ok_or_else(|| CcdError::Remap(format!(
        "saved path {} has no valid monitor device path; this older or incomplete profile must be recaptured after arranging the displays in Windows", index + 1
    )))
}

struct LiveTarget {
    endpoint: Endpoint,
    identity: Option<String>,
    output_technology: i32,
    sources: Vec<Endpoint>,
    active_sources: Vec<Endpoint>,
}

fn live_targets(live: &DisplayConfig) -> Result<Vec<LiveTarget>> {
    if live.path_monitors.len() != live.paths.len() {
        return Err(CcdError::Remap(
            "live target identities are incomplete; reconnect the monitors and retry".into(),
        ));
    }
    let mut targets: Vec<LiveTarget> = Vec::new();
    for (path, monitor) in live.paths.iter().zip(&live.path_monitors) {
        if path.target.target_available == 0 {
            continue;
        }
        let endpoint = target(path);
        let key = identity(monitor);
        let index = match targets.iter().position(|t| t.endpoint == endpoint) {
            Some(index) => index,
            None => {
                targets.push(LiveTarget {
                    endpoint,
                    identity: key.clone(),
                    output_technology: path.target.output_technology,
                    sources: Vec::new(),
                    active_sources: Vec::new(),
                });
                targets.len() - 1
            }
        };
        let record = &mut targets[index];
        if record.identity != key || record.output_technology != path.target.output_technology {
            return Err(CcdError::Remap("live paths disagree about a target's identity or connector; refresh displays and retry".into()));
        }
        if !record.sources.contains(&source(path)) {
            record.sources.push(source(path));
        }
        if path.flags & DISPLAYCONFIG_PATH_ACTIVE != 0
            && !record.active_sources.contains(&source(path))
        {
            record.active_sources.push(source(path));
        }
    }
    Ok(targets)
}

struct SourceGroup {
    original: Endpoint,
    paths: Vec<usize>,
    candidates: Vec<Endpoint>,
}

fn routing_error(path: usize, reason: &str) -> CcdError {
    CcdError::Remap(format!(
        "source routing for saved path {} {reason}; enable and arrange the intended displays in Windows, then retry or recapture the profile",
        path + 1
    ))
}

/// Neither input is mutated, including on failure. Live modes are deliberately
/// unused: inactive but connected targets often have none in QDC_ALL_PATHS.
pub(super) fn remap_config(saved: &DisplayConfig, live: &DisplayConfig) -> Result<DisplayConfig> {
    validate_layout(&saved.paths, &saved.modes)?;
    if !saved.path_monitors.is_empty() && saved.path_monitors.len() != saved.paths.len() {
        return Err(invalid(
            "per-path monitor metadata length does not match the paths",
        ));
    }
    let targets = live_targets(live)?;
    let mut identities = HashSet::new();
    let mut monitors = Vec::new();
    let mut matched = Vec::new();
    for i in 0..saved.paths.len() {
        let monitor = saved_monitor(saved, i)?;
        let key = identity(&monitor).expect("saved_monitor checks identity");
        if !identities.insert(key.clone()) {
            return Err(invalid(format!(
                "multiple saved targets claim device path {}",
                monitor.device_path
            )));
        }
        let mut matches = targets.iter().filter(|t| t.identity.as_ref() == Some(&key));
        let found = matches.next().ok_or_else(|| CcdError::Remap(format!(
            "monitor {} is missing or its device path could not be read; reconnect it and retry, or recapture if the port/dock/driver changed its identity", monitor.device_path
        )))?;
        if matches.next().is_some() {
            return Err(CcdError::Remap(format!(
                "monitor device path {} identifies multiple live targets; reconnect/refresh displays and recapture if the ambiguity persists",
                monitor.device_path
            )));
        }
        monitors.push(monitor);
        matched.push(found);
    }

    // One source assignment per original adapter-qualified clone group. Only
    // routes supported by every target in that group are candidates.
    let mut groups: Vec<SourceGroup> = Vec::new();
    for (i, path) in saved.paths.iter().enumerate() {
        if let Some(group) = groups.iter_mut().find(|g| g.original == source(path)) {
            group.paths.push(i);
            group.candidates.retain(|s| matched[i].sources.contains(s));
        } else {
            groups.push(SourceGroup {
                original: source(path),
                paths: vec![i],
                candidates: matched[i].sources.clone(),
            });
        }
    }
    for group in &mut groups {
        if group.candidates.contains(&group.original) {
            group.candidates = vec![group.original];
            continue;
        }
        let mut active = Vec::new();
        for &i in &group.paths {
            for endpoint in &matched[i].active_sources {
                if group.candidates.contains(endpoint) && !active.contains(endpoint) {
                    active.push(*endpoint);
                }
            }
        }
        if active.len() == 1 {
            group.candidates = active;
        }
    }
    // Singleton propagation respects distinct saved desktops without a generic
    // topology solver or order-dependent "first source wins" selection.
    let mut assigned = vec![None; groups.len()];
    let mut used = HashSet::new();
    loop {
        let mut progress = false;
        for (group, assignment) in groups.iter_mut().zip(&mut assigned) {
            if assignment.is_some() {
                continue;
            }
            group.candidates.retain(|s| !used.contains(s));
            match group.candidates.as_slice() {
                [] => {
                    return Err(routing_error(
                        group.paths[0],
                        "cannot preserve the saved clone/extended layout",
                    ));
                }
                [endpoint] => {
                    *assignment = Some(*endpoint);
                    used.insert(*endpoint);
                    progress = true;
                }
                _ => {}
            }
        }
        if !progress {
            break;
        }
    }
    let mut sources = vec![source(&saved.paths[0]); saved.paths.len()];
    for (group, assignment) in groups.iter().zip(assigned) {
        let endpoint = assignment.ok_or_else(|| routing_error(group.paths[0], "is ambiguous"))?;
        for &i in &group.paths {
            sources[i] = endpoint;
        }
    }

    let mut result = DisplayConfig {
        paths: saved.paths.clone(),
        modes: Vec::new(),
        monitors: Vec::new(),
        path_monitors: monitors,
    };
    let mut indices = HashMap::new();
    for (i, original) in saved.paths.iter().enumerate() {
        let src = sources[i];
        let dst = matched[i].endpoint;
        // Resolve against ORIGINAL indices, never IDs mutated on a previous pass.
        let source_index = append_mode(
            saved,
            original.source.mode_info_idx,
            src,
            &MonitorInfo::default(),
            &mut result,
            &mut indices,
        )?;
        let monitor = result.path_monitors[i].clone();
        let target_index = append_mode(
            saved,
            original.target.mode_info_idx,
            dst,
            &monitor,
            &mut result,
            &mut indices,
        )?;
        let path = &mut result.paths[i];
        path.source.adapter_id = src.adapter;
        path.source.id = src.id;
        path.source.mode_info_idx = source_index;
        path.target.adapter_id = dst.adapter;
        path.target.id = dst.id;
        path.target.mode_info_idx = target_index;
        path.target.output_technology = matched[i].output_technology;
        path.target.target_available = 1;
    }
    validate_layout(&result.paths, &result.modes)?;
    Ok(result)
}

fn append_mode(
    saved: &DisplayConfig,
    index: u32,
    endpoint: Endpoint,
    monitor: &MonitorInfo,
    result: &mut DisplayConfig,
    indices: &mut HashMap<u32, (Endpoint, u32)>,
) -> Result<u32> {
    if index == NO_MODE {
        return Ok(NO_MODE);
    }
    if let Some(&(previous, new_index)) = indices.get(&index) {
        if previous != endpoint {
            return Err(invalid("shared mode would map to different live endpoints"));
        }
        return Ok(new_index);
    }
    let mut mode = saved.modes[index as usize].clone();
    mode.adapter_id = endpoint.adapter;
    mode.id = endpoint.id;
    let new_index = result.modes.len() as u32;
    result.modes.push(mode);
    result.monitors.push(monitor.clone());
    indices.insert(index, (endpoint, new_index));
    Ok(new_index)
}

#[cfg(test)]
mod tests;
