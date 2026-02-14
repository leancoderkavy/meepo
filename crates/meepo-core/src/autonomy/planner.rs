//! Situation report builder and confidence gating
//!
//! Builds context-rich situation reports for the agent's autonomous tick,
//! and provides confidence gating to prevent low-confidence autonomous actions.

use super::action_log::ActionRisk;

/// Maximum allowed risk level for autonomous actions at a given confidence
pub struct ConfidenceGate {
    /// Minimum confidence to execute read-only actions autonomously
    pub read_only_threshold: f64,
    /// Minimum confidence to execute write actions autonomously
    pub write_threshold: f64,
    /// Minimum confidence to execute external actions autonomously
    pub external_threshold: f64,
    /// Minimum confidence to execute destructive actions autonomously
    pub destructive_threshold: f64,
}

impl Default for ConfidenceGate {
    fn default() -> Self {
        Self {
            read_only_threshold: 0.3,
            write_threshold: 0.5,
            external_threshold: 0.7,
            destructive_threshold: 0.9,
        }
    }
}

impl ConfidenceGate {
    /// Check if an action at the given risk level should be allowed at the given confidence
    pub fn is_allowed(&self, risk: ActionRisk, confidence: f64) -> bool {
        let threshold = match risk {
            ActionRisk::ReadOnly => self.read_only_threshold,
            ActionRisk::Write => self.write_threshold,
            ActionRisk::External => self.external_threshold,
            ActionRisk::Destructive => self.destructive_threshold,
        };
        confidence >= threshold
    }

    /// Get the required confidence for a given risk level
    pub fn required_confidence(&self, risk: ActionRisk) -> f64 {
        match risk {
            ActionRisk::ReadOnly => self.read_only_threshold,
            ActionRisk::Write => self.write_threshold,
            ActionRisk::External => self.external_threshold,
            ActionRisk::Destructive => self.destructive_threshold,
        }
    }
}

/// Build a situation report string for the autonomous tick
pub fn build_situation_report(
    active_goal_count: usize,
    due_goal_count: usize,
    running_task_count: usize,
    active_watcher_count: usize,
) -> String {
    let mut report = String::from("## Current Situation\n\n");

    report.push_str(&format!("- **Active goals:** {}\n", active_goal_count));
    report.push_str(&format!("- **Goals due for review:** {}\n", due_goal_count));
    report.push_str(&format!("- **Running background tasks:** {}\n", running_task_count));
    report.push_str(&format!("- **Active watchers:** {}\n", active_watcher_count));

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_gate_defaults() {
        let gate = ConfidenceGate::default();
        assert!(gate.is_allowed(ActionRisk::ReadOnly, 0.3));
        assert!(!gate.is_allowed(ActionRisk::ReadOnly, 0.2));
        assert!(gate.is_allowed(ActionRisk::Write, 0.5));
        assert!(!gate.is_allowed(ActionRisk::Write, 0.4));
        assert!(gate.is_allowed(ActionRisk::External, 0.7));
        assert!(!gate.is_allowed(ActionRisk::External, 0.6));
        assert!(gate.is_allowed(ActionRisk::Destructive, 0.9));
        assert!(!gate.is_allowed(ActionRisk::Destructive, 0.8));
    }

    #[test]
    fn test_required_confidence() {
        let gate = ConfidenceGate::default();
        assert_eq!(gate.required_confidence(ActionRisk::ReadOnly), 0.3);
        assert_eq!(gate.required_confidence(ActionRisk::Destructive), 0.9);
    }

    #[test]
    fn test_build_situation_report() {
        let report = build_situation_report(5, 2, 1, 3);
        assert!(report.contains("Active goals:** 5"));
        assert!(report.contains("Goals due for review:** 2"));
        assert!(report.contains("Running background tasks:** 1"));
        assert!(report.contains("Active watchers:** 3"));
    }
}
