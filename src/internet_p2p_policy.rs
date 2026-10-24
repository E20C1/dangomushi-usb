//! Repository-level policy produced by the M4 Internet P2P threat review.
//!
//! This module deliberately contains no socket, discovery, rendezvous, NAT traversal,
//! relay, or configuration code. Internet P2P remains disabled after review.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternetP2PDecision {
    DisabledAfterThreatReview,
}

impl InternetP2PDecision {
    pub const fn label(self) -> &'static str {
        match self {
            Self::DisabledAfterThreatReview => "DISABLED / THREAT REVIEW COMPLETE",
        }
    }

    pub const fn allows_internet_p2p(self) -> bool {
        false
    }
}

pub const THREAT_REVIEW_VERSION: u16 = 1;

pub const fn current_decision() -> InternetP2PDecision {
    InternetP2PDecision::DisabledAfterThreatReview
}

pub const fn internet_p2p_enabled() -> bool {
    current_decision().allows_internet_p2p()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewed_policy_remains_fail_closed() {
        assert_eq!(
            current_decision(),
            InternetP2PDecision::DisabledAfterThreatReview
        );
        assert!(!internet_p2p_enabled());
        assert_eq!(
            current_decision().label(),
            "DISABLED / THREAT REVIEW COMPLETE"
        );
    }
}
