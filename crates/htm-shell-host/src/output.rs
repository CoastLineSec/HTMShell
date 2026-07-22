use crate::ShellHostError;
use std::collections::BTreeMap;

pub const WL_OUTPUT_MAX_VERSION: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputKey {
    pub global_name: u32,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputEligibility {
    AwaitingInitialState,
    EligibleScale1,
    EligibleFractional(i32),
    UnsupportedScale(i32),
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputRecord {
    pub key: OutputKey,
    pub advertised_version: u32,
    pub bound_version: u32,
    pub scale: i32,
    pub name: Option<String>,
    pub description: Option<String>,
    pub ready: bool,
    pub present: bool,
}

impl OutputRecord {
    pub fn eligibility(&self, fractional_available: bool) -> OutputEligibility {
        if !self.present {
            OutputEligibility::Removed
        } else if !self.ready {
            OutputEligibility::AwaitingInitialState
        } else if self.scale == 1 {
            OutputEligibility::EligibleScale1
        } else if fractional_available {
            OutputEligibility::EligibleFractional(self.scale)
        } else {
            OutputEligibility::UnsupportedScale(self.scale)
        }
    }

    pub fn diagnostic_label(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("output-global-{}", self.key.global_name))
    }
}

#[derive(Debug, Default)]
pub struct OutputCatalog {
    records: BTreeMap<OutputKey, OutputRecord>,
    current_by_global: BTreeMap<u32, OutputKey>,
    next_generation: u64,
}

impl OutputCatalog {
    pub fn add(
        &mut self,
        global_name: u32,
        advertised_version: u32,
    ) -> Result<OutputKey, ShellHostError> {
        if self.current_by_global.contains_key(&global_name) {
            return Err(ShellHostError::Wayland(format!(
                "wl_output global {global_name} was advertised twice without removal"
            )));
        }
        self.next_generation = self.next_generation.saturating_add(1);
        let key = OutputKey {
            global_name,
            generation: self.next_generation,
        };
        self.records.insert(
            key,
            OutputRecord {
                key,
                advertised_version,
                bound_version: advertised_version.min(WL_OUTPUT_MAX_VERSION),
                scale: 1,
                name: None,
                description: None,
                ready: false,
                present: true,
            },
        );
        self.current_by_global.insert(global_name, key);
        Ok(key)
    }

    pub fn key_for_global(&self, global_name: u32) -> Option<OutputKey> {
        self.current_by_global.get(&global_name).copied()
    }

    pub fn get(&self, key: OutputKey) -> Option<&OutputRecord> {
        self.records.get(&key).filter(|record| record.present)
    }

    pub fn get_mut(&mut self, key: OutputKey) -> Option<&mut OutputRecord> {
        self.records.get_mut(&key).filter(|record| record.present)
    }

    pub fn set_scale(&mut self, key: OutputKey, scale: i32) -> bool {
        if scale <= 0 {
            return false;
        }
        let Some(record) = self.get_mut(key) else {
            return false;
        };
        let changed = record.scale != scale;
        record.scale = scale;
        changed
    }

    pub fn set_name(&mut self, key: OutputKey, name: String) -> bool {
        let Some(record) = self.get_mut(key) else {
            return false;
        };
        if record.bound_version < 4 {
            return false;
        }
        record.name = Some(name);
        true
    }

    pub fn set_description(&mut self, key: OutputKey, description: String) -> bool {
        let Some(record) = self.get_mut(key) else {
            return false;
        };
        if record.bound_version < 4 {
            return false;
        }
        record.description = Some(description);
        true
    }

    pub fn mark_ready(&mut self, key: OutputKey) -> bool {
        let Some(record) = self.get_mut(key) else {
            return false;
        };
        let changed = !record.ready;
        record.ready = true;
        changed
    }

    pub fn finalize_initial(&mut self) {
        for record in self.records.values_mut().filter(|record| record.present) {
            record.ready = true;
        }
    }

    pub fn remove(&mut self, global_name: u32) -> Option<OutputRecord> {
        let key = self.current_by_global.remove(&global_name)?;
        let mut record = self.records.remove(&key)?;
        record.present = false;
        record.ready = false;
        Some(record)
    }

    pub fn eligible(&self, fractional_available: bool) -> Vec<&OutputRecord> {
        let mut records: Vec<_> = self
            .records
            .values()
            .filter(|record| {
                matches!(
                    record.eligibility(fractional_available),
                    OutputEligibility::EligibleScale1 | OutputEligibility::EligibleFractional(_)
                )
            })
            .collect();
        records.sort_by(|left, right| {
            left.name
                .as_deref()
                .cmp(&right.name.as_deref())
                .then(left.key.global_name.cmp(&right.key.global_name))
                .then(left.key.generation.cmp(&right.key.generation))
        });
        records
    }

    pub fn present(&self) -> Vec<&OutputRecord> {
        self.records
            .values()
            .filter(|record| record.present)
            .collect()
    }

    pub fn generation_count(&self) -> u64 {
        self.next_generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_and_two_output_discovery_is_order_independent() {
        let mut catalog = OutputCatalog::default();
        let second = catalog.add(8, 4).unwrap();
        let first = catalog.add(2, 3).unwrap();
        catalog.set_name(second, "zeta".into());
        catalog.finalize_initial();
        let keys: Vec<_> = catalog
            .eligible(false)
            .iter()
            .map(|record| record.key.global_name)
            .collect();
        assert_eq!(keys, vec![2, 8]);
        assert_eq!(catalog.get(first).unwrap().bound_version, 3);
        assert_eq!(catalog.get(second).unwrap().bound_version, 4);
    }

    #[test]
    fn version_four_metadata_is_diagnostic_only() {
        let mut catalog = OutputCatalog::default();
        let old = catalog.add(1, 2).unwrap();
        let new = catalog.add(2, 9).unwrap();
        assert!(!catalog.set_name(old, "legacy".into()));
        assert!(catalog.set_name(new, "same-name".into()));
        assert!(catalog.set_description(new, "description".into()));
        assert_eq!(
            catalog.get(old).unwrap().diagnostic_label(),
            "output-global-1"
        );
        assert_eq!(catalog.get(new).unwrap().diagnostic_label(), "same-name");
        assert_eq!(catalog.get(new).unwrap().bound_version, 4);
    }

    #[test]
    fn remove_and_readd_uses_a_fresh_generation_even_with_same_name() {
        let mut catalog = OutputCatalog::default();
        let first = catalog.add(7, 4).unwrap();
        catalog.set_name(first, "DP-1".into());
        catalog.mark_ready(first);
        let removed = catalog.remove(7).unwrap();
        assert_eq!(removed.eligibility(false), OutputEligibility::Removed);
        assert!(catalog.get(first).is_none());
        let second = catalog.add(7, 4).unwrap();
        catalog.set_name(second, "DP-1".into());
        catalog.mark_ready(second);
        assert_ne!(first, second);
        assert_eq!(first.global_name, second.global_name);
        assert!(second.generation > first.generation);
    }

    #[test]
    fn scale_eligibility_does_not_break_supported_outputs() {
        let mut catalog = OutputCatalog::default();
        let supported = catalog.add(1, 4).unwrap();
        let unsupported = catalog.add(2, 4).unwrap();
        catalog.set_scale(unsupported, 2);
        catalog.finalize_initial();
        assert_eq!(
            catalog.get(supported).unwrap().eligibility(false),
            OutputEligibility::EligibleScale1
        );
        assert_eq!(
            catalog.get(unsupported).unwrap().eligibility(false),
            OutputEligibility::UnsupportedScale(2)
        );
        assert_eq!(catalog.eligible(false).len(), 1);
        assert_eq!(
            catalog.get(unsupported).unwrap().eligibility(true),
            OutputEligibility::EligibleFractional(2)
        );
        assert_eq!(catalog.eligible(true).len(), 2);
    }

    #[test]
    fn stale_events_and_repeated_removal_are_contained() {
        let mut catalog = OutputCatalog::default();
        let key = catalog.add(4, 4).unwrap();
        assert!(catalog.remove(4).is_some());
        assert!(catalog.remove(4).is_none());
        assert!(!catalog.set_scale(key, 2));
        assert!(!catalog.mark_ready(key));
        assert!(catalog.present().is_empty());
        assert!(catalog.records.is_empty());
    }

    #[test]
    fn no_output_state_is_idle_data_not_an_error() {
        let mut catalog = OutputCatalog::default();
        assert!(catalog.eligible(false).is_empty());
        let key = catalog.add(3, 1).unwrap();
        assert_eq!(
            catalog.get(key).unwrap().eligibility(false),
            OutputEligibility::AwaitingInitialState
        );
        catalog.mark_ready(key);
        assert_eq!(catalog.eligible(false).len(), 1);
    }
}
