use crate::lan_access_control::LanEnableControl;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectedNetworkCategory {
    Unknown,
    Public,
    Private,
    DomainAuthenticated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkSafetySource {
    WindowsNetworkListManager,
    UnsupportedPlatform,
    ProbeFailed,
}

impl NetworkSafetySource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::WindowsNetworkListManager => "WINDOWS NETWORK LIST MANAGER",
            Self::UnsupportedPlatform => "UNSUPPORTED PLATFORM / FAIL CLOSED",
            Self::ProbeFailed => "PROBE FAILED / FAIL CLOSED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkEnvironment {
    Unknown,
    Public,
    Trusted,
}

impl NetworkEnvironment {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "UNKNOWN",
            Self::Public => "PUBLIC / UNTRUSTED",
            Self::Trusted => "PRIVATE OR DOMAIN / TRUSTED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkSafetyAssessment {
    environment: NetworkEnvironment,
    connected_networks: usize,
    source: NetworkSafetySource,
}

impl NetworkSafetyAssessment {
    pub const fn environment(self) -> NetworkEnvironment {
        self.environment
    }

    pub const fn connected_networks(self) -> usize {
        self.connected_networks
    }

    pub const fn source(self) -> NetworkSafetySource {
        self.source
    }

    pub const fn is_trusted(self) -> bool {
        matches!(self.environment, NetworkEnvironment::Trusted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanExposureDecision {
    LanIntentOff,
    BlockedUnknownNetwork,
    BlockedPublicNetwork,
    EligibleTrustedNetwork,
}

impl LanExposureDecision {
    pub const fn allows_exposure(self) -> bool {
        matches!(self, Self::EligibleTrustedNetwork)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::LanIntentOff => "BLOCKED / LAN CONTROL OFF",
            Self::BlockedUnknownNetwork => "BLOCKED / NETWORK UNKNOWN",
            Self::BlockedPublicNetwork => "BLOCKED / PUBLIC NETWORK",
            Self::EligibleTrustedNetwork => "ELIGIBLE / PRIVATE OR DOMAIN",
        }
    }
}

pub fn classify_connected_categories(
    categories: &[ConnectedNetworkCategory],
    source: NetworkSafetySource,
) -> NetworkSafetyAssessment {
    let environment = if categories.is_empty() {
        NetworkEnvironment::Unknown
    } else if categories
        .iter()
        .any(|category| matches!(category, ConnectedNetworkCategory::Public))
    {
        NetworkEnvironment::Public
    } else if categories
        .iter()
        .any(|category| matches!(category, ConnectedNetworkCategory::Unknown))
    {
        NetworkEnvironment::Unknown
    } else {
        NetworkEnvironment::Trusted
    };

    NetworkSafetyAssessment {
        environment,
        connected_networks: categories.len(),
        source,
    }
}

pub fn evaluate(
    lan_control: &LanEnableControl,
    assessment: NetworkSafetyAssessment,
) -> LanExposureDecision {
    if !lan_control.is_enabled() {
        return LanExposureDecision::LanIntentOff;
    }

    match assessment.environment {
        NetworkEnvironment::Unknown => LanExposureDecision::BlockedUnknownNetwork,
        NetworkEnvironment::Public => LanExposureDecision::BlockedPublicNetwork,
        NetworkEnvironment::Trusted => LanExposureDecision::EligibleTrustedNetwork,
    }
}

pub fn assess_current_network() -> NetworkSafetyAssessment {
    platform::assess_current_network()
}

pub fn current_exposure_decision(
    lan_control: &LanEnableControl,
) -> (NetworkSafetyAssessment, LanExposureDecision) {
    let assessment = assess_current_network();
    let decision = evaluate(lan_control, assessment);
    (assessment, decision)
}

#[cfg(windows)]
mod platform {
    use super::{
        classify_connected_categories, ConnectedNetworkCategory, NetworkSafetyAssessment,
        NetworkSafetySource,
    };
    use std::thread;
    use windows::Win32::Networking::NetworkListManager::{
        INetwork, INetworkListManager, NetworkListManager, NLM_ENUM_NETWORK_CONNECTED,
        NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED, NLM_NETWORK_CATEGORY_PRIVATE,
        NLM_NETWORK_CATEGORY_PUBLIC,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    struct ComApartment;

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }

    pub fn assess_current_network() -> NetworkSafetyAssessment {
        match thread::spawn(assess_on_fresh_com_thread).join() {
            Ok(Ok(categories)) => classify_connected_categories(
                &categories,
                NetworkSafetySource::WindowsNetworkListManager,
            ),
            Ok(Err(())) | Err(_) => classify_connected_categories(
                &[],
                NetworkSafetySource::ProbeFailed,
            ),
        }
    }

    fn assess_on_fresh_com_thread() -> Result<Vec<ConnectedNetworkCategory>, ()> {
        unsafe {
            let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
            if initialized.is_err() {
                return Err(());
            }
            let _apartment = ComApartment;

            let manager: INetworkListManager =
                CoCreateInstance(&NetworkListManager, None, CLSCTX_ALL).map_err(|_| ())?;
            let networks = manager
                .GetNetworks(NLM_ENUM_NETWORK_CONNECTED)
                .map_err(|_| ())?;

            let mut categories = Vec::new();

            loop {
                let mut slot: [Option<INetwork>; 1] = [None];
                let mut fetched = 0_u32;
                networks
                    .Next(&mut slot, Some(&mut fetched))
                    .map_err(|_| ())?;

                if fetched == 0 {
                    break;
                }

                let network = slot[0].take().ok_or(())?;
                let category = network.GetCategory().map_err(|_| ())?;

                let category = if category == NLM_NETWORK_CATEGORY_PUBLIC {
                    ConnectedNetworkCategory::Public
                } else if category == NLM_NETWORK_CATEGORY_PRIVATE {
                    ConnectedNetworkCategory::Private
                } else if category == NLM_NETWORK_CATEGORY_DOMAIN_AUTHENTICATED {
                    ConnectedNetworkCategory::DomainAuthenticated
                } else {
                    ConnectedNetworkCategory::Unknown
                };

                categories.push(category);
            }

            Ok(categories)
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{
        classify_connected_categories, NetworkSafetyAssessment, NetworkSafetySource,
    };

    pub fn assess_current_network() -> NetworkSafetyAssessment {
        classify_connected_categories(&[], NetworkSafetySource::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_the_default_for_no_connected_evidence() {
        let assessment =
            classify_connected_categories(&[], NetworkSafetySource::ProbeFailed);
        assert_eq!(assessment.environment(), NetworkEnvironment::Unknown);
        assert_eq!(assessment.connected_networks(), 0);
    }

    #[test]
    fn any_public_network_forces_public() {
        let assessment = classify_connected_categories(
            &[
                ConnectedNetworkCategory::Private,
                ConnectedNetworkCategory::Public,
                ConnectedNetworkCategory::DomainAuthenticated,
            ],
            NetworkSafetySource::WindowsNetworkListManager,
        );
        assert_eq!(assessment.environment(), NetworkEnvironment::Public);
    }

    #[test]
    fn unknown_mixed_with_trusted_stays_unknown() {
        let assessment = classify_connected_categories(
            &[
                ConnectedNetworkCategory::Private,
                ConnectedNetworkCategory::Unknown,
            ],
            NetworkSafetySource::WindowsNetworkListManager,
        );
        assert_eq!(assessment.environment(), NetworkEnvironment::Unknown);
    }

    #[test]
    fn all_private_or_domain_is_trusted() {
        let assessment = classify_connected_categories(
            &[
                ConnectedNetworkCategory::Private,
                ConnectedNetworkCategory::DomainAuthenticated,
            ],
            NetworkSafetySource::WindowsNetworkListManager,
        );
        assert_eq!(assessment.environment(), NetworkEnvironment::Trusted);
        assert!(assessment.is_trusted());
    }

    #[test]
    fn exposure_requires_both_lan_intent_and_trusted_environment() {
        let trusted = classify_connected_categories(
            &[ConnectedNetworkCategory::Private],
            NetworkSafetySource::WindowsNetworkListManager,
        );
        let public = classify_connected_categories(
            &[ConnectedNetworkCategory::Public],
            NetworkSafetySource::WindowsNetworkListManager,
        );
        let unknown =
            classify_connected_categories(&[], NetworkSafetySource::ProbeFailed);

        let mut lan = LanEnableControl::default();
        assert_eq!(evaluate(&lan, trusted), LanExposureDecision::LanIntentOff);

        lan.enable_for_this_run();
        assert_eq!(
            evaluate(&lan, trusted),
            LanExposureDecision::EligibleTrustedNetwork
        );
        assert_eq!(
            evaluate(&lan, public),
            LanExposureDecision::BlockedPublicNetwork
        );
        assert_eq!(
            evaluate(&lan, unknown),
            LanExposureDecision::BlockedUnknownNetwork
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn unsupported_platform_probe_fails_closed() {
        let assessment = assess_current_network();
        assert_eq!(
            assessment.source(),
            NetworkSafetySource::UnsupportedPlatform
        );
        assert_eq!(assessment.environment(), NetworkEnvironment::Unknown);
    }
}
