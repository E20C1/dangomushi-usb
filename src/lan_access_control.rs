//! Explicit process-local LAN operator-intent control.
//!
//! This module deliberately does not bind sockets, persist state, or grant network
//! authority. A future LAN listener may require this gate, but later M3 security
//! boundaries must also be satisfied before any non-loopback listener exists.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanEnableState {
    Disabled,
    EnabledForThisRun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanEnableControl {
    state: LanEnableState,
}

impl Default for LanEnableControl {
    fn default() -> Self {
        Self {
            state: LanEnableState::Disabled,
        }
    }
}

impl LanEnableControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn state(&self) -> LanEnableState {
        self.state
    }

    pub const fn is_enabled(&self) -> bool {
        matches!(self.state, LanEnableState::EnabledForThisRun)
    }

    pub fn enable_for_this_run(&mut self) -> bool {
        let changed = !self.is_enabled();
        self.state = LanEnableState::EnabledForThisRun;
        changed
    }

    pub fn disable(&mut self) -> bool {
        let changed = self.is_enabled();
        self.state = LanEnableState::Disabled;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_control_is_disabled() {
        let control = LanEnableControl::new();
        assert_eq!(control.state(), LanEnableState::Disabled);
        assert!(!control.is_enabled());
    }

    #[test]
    fn enabling_requires_an_explicit_transition() {
        let mut control = LanEnableControl::default();

        assert!(control.enable_for_this_run());
        assert!(control.is_enabled());
        assert!(!control.enable_for_this_run());
    }

    #[test]
    fn disable_is_explicit_and_idempotent() {
        let mut control = LanEnableControl::default();
        control.enable_for_this_run();

        assert!(control.disable());
        assert!(!control.is_enabled());
        assert!(!control.disable());
    }

    #[test]
    fn a_new_process_state_does_not_inherit_enablement() {
        let mut first = LanEnableControl::default();
        first.enable_for_this_run();
        assert!(first.is_enabled());

        let restarted = LanEnableControl::default();
        assert!(!restarted.is_enabled());
    }
}
