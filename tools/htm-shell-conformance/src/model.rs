use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

pub const REPORT_SCHEMA_VERSION: u32 = 1;
pub const CONTRACT_INTERFACE_VERSION: u32 = 1;
pub const CAPABILITY_ROOT_OVERLAY: u32 = 1;
pub const CAPABILITY_STANDARD_POINTER_FOCUS: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum KnownCapability {
    RootOverlay,
    StandardPointerFocus,
}

impl KnownCapability {
    pub const fn wire_value(self) -> u32 {
        match self {
            Self::RootOverlay => CAPABILITY_ROOT_OVERLAY,
            Self::StandardPointerFocus => CAPABILITY_STANDARD_POINTER_FOCUS,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilitySet {
    values: BTreeSet<u32>,
}

impl CapabilitySet {
    pub fn insert(&mut self, value: u32) {
        self.values.insert(value);
    }

    pub fn contains(&self, capability: KnownCapability) -> bool {
        self.values.contains(&capability.wire_value())
    }

    pub fn unknown_values(&self) -> Vec<u32> {
        self.values
            .iter()
            .copied()
            .filter(|value| decode_capability(*value).is_none())
            .collect()
    }
}

pub const fn decode_capability(value: u32) -> Option<KnownCapability> {
    match value {
        CAPABILITY_ROOT_OVERLAY => Some(KnownCapability::RootOverlay),
        CAPABILITY_STANDARD_POINTER_FOCUS => Some(KnownCapability::StandardPointerFocus),
        _ => None,
    }
}

pub fn compatible_bind_version(advertised: u32) -> Option<u32> {
    (advertised > 0).then_some(advertised.min(CONTRACT_INTERFACE_VERSION))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResultCategory {
    Pass,
    Fail,
    Skip,
    Unsupported,
    Inconclusive,
    Timeout,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TestResult {
    pub name: String,
    pub group: String,
    pub required: bool,
    pub result: ResultCategory,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl TestResult {
    pub fn new(
        name: impl Into<String>,
        group: impl Into<String>,
        required: bool,
        result: ResultCategory,
        detail: Option<impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            group: group.into(),
            required,
            result,
            detail: detail.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ContractDescriptor {
    pub name: String,
    pub interface_version: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConformanceReport {
    pub schema_version: u32,
    pub contract: ContractDescriptor,
    pub result: ResultCategory,
    pub tests: Vec<TestResult>,
}

impl ConformanceReport {
    pub fn new(mut tests: Vec<TestResult>) -> Self {
        tests.sort_by(|left, right| left.name.cmp(&right.name));
        Self {
            schema_version: REPORT_SCHEMA_VERSION,
            contract: ContractDescriptor {
                name: "htm-shell-v1".into(),
                interface_version: CONTRACT_INTERFACE_VERSION,
            },
            result: aggregate_result(&tests),
            tests,
        }
    }

    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        let mut json = serde_json::to_string_pretty(self)?;
        json.push('\n');
        Ok(json)
    }
}

pub fn aggregate_result(tests: &[TestResult]) -> ResultCategory {
    let required = tests.iter().filter(|test| test.required);
    if required.clone().any(|test| {
        matches!(
            test.result,
            ResultCategory::Fail | ResultCategory::Unsupported
        )
    }) {
        return ResultCategory::Fail;
    }
    if required
        .clone()
        .any(|test| test.result == ResultCategory::Timeout)
    {
        return ResultCategory::Timeout;
    }
    if required.clone().any(|test| {
        matches!(
            test.result,
            ResultCategory::Skip | ResultCategory::Inconclusive
        )
    }) {
        return ResultCategory::Inconclusive;
    }
    ResultCategory::Pass
}

pub fn redact_detail(detail: &str, secret: &str) -> String {
    if secret.is_empty() {
        detail.to_owned()
    } else {
        detail.replace(secret, "<redacted>")
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleError {
    InvalidTransition,
    DuplicateRoot,
    InvalidConfigure,
    InvalidAcknowledgement,
    BufferBeforeAcknowledgement,
    StaleObject,
}

#[cfg(test)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LifecycleModel {
    discovered: bool,
    authorized: bool,
    capabilities_ready: bool,
    surface_alive: bool,
    root_alive: bool,
    role_assigned: bool,
    pending_configure: Option<u32>,
    acknowledged_configure: Option<u32>,
    mapped: bool,
    disconnected: bool,
}

#[cfg(test)]
impl LifecycleModel {
    pub fn discover(&mut self) -> Result<(), LifecycleError> {
        if self.discovered || self.disconnected {
            return Err(LifecycleError::InvalidTransition);
        }
        self.discovered = true;
        Ok(())
    }

    pub fn authorize(&mut self) -> Result<(), LifecycleError> {
        if !self.discovered || self.authorized || self.disconnected {
            return Err(LifecycleError::InvalidTransition);
        }
        self.authorized = true;
        Ok(())
    }

    pub fn capabilities_ready(&mut self) -> Result<(), LifecycleError> {
        if !self.authorized || self.capabilities_ready || self.disconnected {
            return Err(LifecycleError::InvalidTransition);
        }
        self.capabilities_ready = true;
        Ok(())
    }

    pub fn create_root(&mut self) -> Result<(), LifecycleError> {
        if self.root_alive || self.role_assigned {
            return Err(LifecycleError::DuplicateRoot);
        }
        if !self.capabilities_ready || self.disconnected {
            return Err(LifecycleError::InvalidTransition);
        }
        self.surface_alive = true;
        self.root_alive = true;
        self.role_assigned = true;
        Ok(())
    }

    pub fn configure(
        &mut self,
        serial: u32,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<(), LifecycleError> {
        if !self.root_alive {
            return Err(LifecycleError::StaleObject);
        }
        if serial == 0 || logical_width == 0 || logical_height == 0 {
            return Err(LifecycleError::InvalidConfigure);
        }
        self.pending_configure = Some(serial);
        Ok(())
    }

    pub fn acknowledge(&mut self, serial: u32) -> Result<(), LifecycleError> {
        if !self.root_alive {
            return Err(LifecycleError::StaleObject);
        }
        if self.pending_configure != Some(serial) || self.acknowledged_configure == Some(serial) {
            return Err(LifecycleError::InvalidAcknowledgement);
        }
        self.acknowledged_configure = Some(serial);
        self.pending_configure = None;
        Ok(())
    }

    pub fn commit_buffer(&mut self) -> Result<(), LifecycleError> {
        if !self.root_alive || !self.surface_alive {
            return Err(LifecycleError::StaleObject);
        }
        if self.acknowledged_configure.is_none() {
            return Err(LifecycleError::BufferBeforeAcknowledgement);
        }
        self.mapped = true;
        Ok(())
    }

    pub fn unmap(&mut self) -> Result<(), LifecycleError> {
        if !self.surface_alive {
            return Err(LifecycleError::StaleObject);
        }
        self.mapped = false;
        Ok(())
    }

    pub fn destroy_root(&mut self) -> Result<(), LifecycleError> {
        if !self.root_alive {
            return Err(LifecycleError::StaleObject);
        }
        self.root_alive = false;
        self.mapped = false;
        self.pending_configure = None;
        Ok(())
    }

    pub fn destroy_surface(&mut self) -> Result<(), LifecycleError> {
        if !self.surface_alive {
            return Err(LifecycleError::StaleObject);
        }
        self.surface_alive = false;
        self.root_alive = false;
        self.mapped = false;
        self.pending_configure = None;
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.disconnected = true;
        self.authorized = false;
        self.capabilities_ready = false;
        self.surface_alive = false;
        self.root_alive = false;
        self.mapped = false;
        self.pending_configure = None;
    }

    pub const fn mapped(&self) -> bool {
        self.mapped
    }

    pub const fn root_alive(&self) -> bool {
        self.root_alive
    }

    pub const fn surface_alive(&self) -> bool {
        self.surface_alive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(name: &str, required: bool) -> TestResult {
        TestResult::new(name, "unit", required, ResultCategory::Pass, None::<String>)
    }

    #[test]
    fn capability_decoding_contains_unknown_values() {
        let mut capabilities = CapabilitySet::default();
        capabilities.insert(CAPABILITY_ROOT_OVERLAY);
        capabilities.insert(9_999);
        assert!(capabilities.contains(KnownCapability::RootOverlay));
        assert_eq!(capabilities.unknown_values(), vec![9_999]);
        assert_eq!(decode_capability(9_999), None);
    }

    #[test]
    fn interface_version_binding_is_additive() {
        assert_eq!(compatible_bind_version(0), None);
        assert_eq!(compatible_bind_version(1), Some(1));
        assert_eq!(compatible_bind_version(7), Some(1));
    }

    #[test]
    fn valid_root_lifecycle_completes() {
        let mut lifecycle = LifecycleModel::default();
        lifecycle.discover().unwrap();
        lifecycle.authorize().unwrap();
        lifecycle.capabilities_ready().unwrap();
        lifecycle.create_root().unwrap();
        lifecycle.configure(41, 640, 160).unwrap();
        lifecycle.acknowledge(41).unwrap();
        lifecycle.commit_buffer().unwrap();
        assert!(lifecycle.mapped());
        lifecycle.unmap().unwrap();
        lifecycle.destroy_root().unwrap();
        lifecycle.destroy_surface().unwrap();
        lifecycle.disconnect();
        assert!(!lifecycle.root_alive());
        assert!(!lifecycle.surface_alive());
    }

    #[test]
    fn buffer_before_configure_acknowledgement_is_rejected() {
        let mut lifecycle = LifecycleModel::default();
        lifecycle.discover().unwrap();
        lifecycle.authorize().unwrap();
        lifecycle.capabilities_ready().unwrap();
        lifecycle.create_root().unwrap();
        lifecycle.configure(7, 640, 160).unwrap();
        assert_eq!(
            lifecycle.commit_buffer(),
            Err(LifecycleError::BufferBeforeAcknowledgement)
        );
    }

    #[test]
    fn duplicate_acknowledgement_and_root_are_rejected() {
        let mut lifecycle = LifecycleModel::default();
        lifecycle.discover().unwrap();
        lifecycle.authorize().unwrap();
        lifecycle.capabilities_ready().unwrap();
        lifecycle.create_root().unwrap();
        assert_eq!(lifecycle.create_root(), Err(LifecycleError::DuplicateRoot));
        lifecycle.configure(9, 640, 160).unwrap();
        lifecycle.acknowledge(9).unwrap();
        assert_eq!(
            lifecycle.acknowledge(9),
            Err(LifecycleError::InvalidAcknowledgement)
        );
    }

    #[test]
    fn either_destroy_order_is_contained() {
        let mut root_first = configured_lifecycle();
        root_first.destroy_root().unwrap();
        root_first.destroy_surface().unwrap();

        let mut surface_first = configured_lifecycle();
        surface_first.destroy_surface().unwrap();
        assert!(!surface_first.root_alive());
        assert_eq!(
            surface_first.destroy_root(),
            Err(LifecycleError::StaleObject)
        );
    }

    #[test]
    fn abrupt_disconnect_drops_authority_and_objects() {
        let mut lifecycle = configured_lifecycle();
        lifecycle.commit_buffer().unwrap();
        lifecycle.disconnect();
        assert!(!lifecycle.mapped());
        assert!(!lifecycle.root_alive());
        assert!(!lifecycle.surface_alive());
    }

    #[test]
    fn deterministic_json_sorts_tests() {
        let report = ConformanceReport::new(vec![pass("z.last", true), pass("a.first", true)]);
        let first = report.to_pretty_json().unwrap();
        let second = report.to_pretty_json().unwrap();
        assert_eq!(first, second);
        assert!(first.find("a.first").unwrap() < first.find("z.last").unwrap());
    }

    #[test]
    fn optional_unsupported_does_not_fail_baseline() {
        let report = ConformanceReport::new(vec![
            pass("baseline", true),
            TestResult::new(
                "optional",
                "unit",
                false,
                ResultCategory::Unsupported,
                None::<String>,
            ),
        ]);
        assert_eq!(report.result, ResultCategory::Pass);
    }

    #[test]
    fn missing_mandatory_capability_fails_baseline() {
        let report = ConformanceReport::new(vec![TestResult::new(
            "capability.standard_pointer_focus",
            "capability",
            true,
            ResultCategory::Fail,
            Some("mandatory capability was not advertised"),
        )]);
        assert_eq!(report.result, ResultCategory::Fail);
    }

    #[test]
    fn timeout_is_preserved_when_no_harder_failure_exists() {
        let report = ConformanceReport::new(vec![TestResult::new(
            "frame.callback",
            "root",
            true,
            ResultCategory::Timeout,
            None::<String>,
        )]);
        assert_eq!(report.result, ResultCategory::Timeout);
    }

    #[test]
    fn report_never_contains_supplied_secret() {
        let secret = "private-session-value";
        let detail = redact_detail("claim private-session-value was rejected", secret);
        let report = ConformanceReport::new(vec![TestResult::new(
            "authorization",
            "authorization",
            true,
            ResultCategory::Fail,
            Some(detail),
        )]);
        assert!(!report.to_pretty_json().unwrap().contains(secret));
    }

    fn configured_lifecycle() -> LifecycleModel {
        let mut lifecycle = LifecycleModel::default();
        lifecycle.discover().unwrap();
        lifecycle.authorize().unwrap();
        lifecycle.capabilities_ready().unwrap();
        lifecycle.create_root().unwrap();
        lifecycle.configure(11, 640, 160).unwrap();
        lifecycle.acknowledge(11).unwrap();
        lifecycle
    }
}
