//! Live source metadata, separate from immutable component assignments.
//! These records retain no provider object, callback, state value, or registry.

use crate::{StateBindingKey, StateBindingScope};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StateProviderIdentity {
    Clock,
    UPower,
    PowerProfiles,
    PipeWire,
    Shell,
}

impl StateProviderIdentity {
    pub(crate) fn for_binding(key: StateBindingKey) -> Self {
        let name = key.as_str();
        if name.starts_with("clock.") {
            Self::Clock
        } else if name.starts_with("battery.") || name.starts_with("upower.") {
            Self::UPower
        } else if name.starts_with("power_profile.") {
            Self::PowerProfiles
        } else if name.starts_with("pipewire.") {
            Self::PipeWire
        } else {
            Self::Shell
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct StateProviderIncarnation {
    pub connection_epoch: u64,
    pub source_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct StateValueVersion {
    pub incarnation: StateProviderIncarnation,
    pub semantic_sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "category", rename_all = "snake_case")]
pub enum LiveStateScope {
    Process,
    Output { global_name: u32, generation: u64 },
    Surface { owner: u64, generation: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiveStateSourceIdentity {
    pub source: &'static str,
    pub scope: LiveStateScope,
    pub provider: StateProviderIdentity,
    pub incarnation: StateProviderIncarnation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LiveStateConsumerMetadata {
    pub binding_identity: String,
    pub assignment_identity: String,
    pub source_route_identity: String,
    pub resolved_scope: Option<LiveStateScope>,
    pub source_identity: Option<LiveStateSourceIdentity>,
    pub value_version: Option<StateValueVersion>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StateReferenceContext {
    pub output_global: u32,
    pub output_generation: u64,
    pub surface_owner: u64,
    pub surface_generation: u64,
}

pub(crate) fn resolve_scope(
    scope: StateBindingScope,
    context: Option<StateReferenceContext>,
) -> Option<LiveStateScope> {
    match scope {
        StateBindingScope::Process => Some(LiveStateScope::Process),
        StateBindingScope::Output => context.map(|context| LiveStateScope::Output {
            global_name: context.output_global,
            generation: context.output_generation,
        }),
        StateBindingScope::Surface => context.map(|context| LiveStateScope::Surface {
            owner: context.surface_owner,
            generation: context.surface_generation,
        }),
    }
}
