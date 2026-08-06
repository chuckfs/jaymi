//! Paused Execution Plans — Planner-owned resume state for review gates.
//!
//! When an [`ExecutionPlan`] enters [`ExecutionStatus::AwaitingReview`], the
//! Planner **pauses**: the conversation stays active, tools do not run, and
//! the frozen plan + [`ToolInput`] are retained so Approve can resume without
//! replanning.
//!
//! Modify regenerates a child plan (re-paused); Cancel / timeout / a new user
//! request invalidate the pause.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use jaymi_capabilities::Capability;
use jaymi_permissions::PermissionCheckResult;
use jaymi_policies::PolicyEvaluation;
use jaymi_tools::ToolInput;

use crate::execution_plan::{ExecutionPlan, ExecutionPlanId, ExecutionStatus};

/// Default how long a pause may wait for review before timing out.
pub const DEFAULT_PAUSE_TTL: Duration = Duration::from_secs(30 * 60);

/// Planner-owned pause entry — enough to resume the same plan deterministically.
#[derive(Debug, Clone)]
pub struct PausedExecution {
    /// Plan frozen in [`ExecutionStatus::AwaitingReview`].
    pub plan: ExecutionPlan,
    /// Exact tool input captured at pause time (no replan on resume).
    pub tool_input: ToolInput,
    /// Selected tool id.
    pub tool_id: String,
    /// Bound provider id, when known.
    pub provider_id: Option<String>,
    /// Capability that owned the plan.
    pub capability: Capability,
    /// Action-policy snapshot from the original gate.
    pub policy_evaluation: Option<PolicyEvaluation>,
    /// Permission snapshot from the original gate.
    pub permission_result: Option<PermissionCheckResult>,
    /// When the Planner paused (for timeout).
    pub paused_at: Instant,
}

impl PausedExecution {
    /// Plan id this pause is keyed by.
    pub fn plan_id(&self) -> &ExecutionPlanId {
        self.plan.id()
    }

    /// True when this pause has exceeded `ttl`.
    pub fn is_timed_out(&self, now: Instant, ttl: Duration) -> bool {
        now.duration_since(self.paused_at) >= ttl
    }
}

/// Why a pause lookup / transition failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PauseError {
    /// No pause registered for this plan id.
    NotFound {
        /// Requested plan.
        plan_id: String,
    },
    /// Pause existed but review TTL elapsed.
    TimedOut {
        /// Timed-out plan.
        plan_id: String,
    },
    /// Pause is not in a resumable status (duplicate approval, already cancelled, …).
    InvalidStatus {
        /// Plan id.
        plan_id: String,
        /// Observed status.
        status: ExecutionStatus,
    },
    /// Store lock poisoned.
    Poisoned,
}

impl std::fmt::Display for PauseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { plan_id } => {
                write!(
                    f,
                    "no paused execution for plan {plan_id} (duplicate, cancelled, or unknown)"
                )
            }
            Self::TimedOut { plan_id } => {
                write!(f, "paused execution for plan {plan_id} timed out")
            }
            Self::InvalidStatus { plan_id, status } => {
                write!(
                    f,
                    "paused execution for plan {plan_id} has invalid status {status}"
                )
            }
            Self::Poisoned => write!(f, "paused execution store lock poisoned"),
        }
    }
}

impl std::error::Error for PauseError {}

/// In-memory store of plans waiting on conversational review.
#[derive(Debug)]
pub struct PausedPlanStore {
    entries: HashMap<String, PausedExecution>,
    ttl: Duration,
}

impl Default for PausedPlanStore {
    fn default() -> Self {
        Self::new(DEFAULT_PAUSE_TTL)
    }
}

impl PausedPlanStore {
    /// Create an empty store with the given review TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }

    /// Review TTL used for timeout checks.
    pub fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Override TTL (tests / configuration).
    pub fn set_ttl(&mut self, ttl: Duration) {
        self.ttl = ttl;
    }

    /// Number of active pauses.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when nothing is paused.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether a plan id is currently paused (ignores timeout until expire/take).
    pub fn contains(&self, plan_id: &ExecutionPlanId) -> bool {
        self.entries.contains_key(plan_id.as_str())
    }

    /// Insert a pause. Replaces and returns any previous entry with the same id.
    ///
    /// Also expires timed-out entries first. Does **not** cancel other plan ids —
    /// callers decide whether a new request invalidates prior pauses.
    pub fn pause(&mut self, entry: PausedExecution) -> Option<PausedExecution> {
        self.expire_due(Instant::now());
        let id = entry.plan_id().as_str().to_string();
        self.entries.insert(id, entry)
    }

    /// Remove and return a pause for resume/cancel without timeout enforcement.
    pub fn remove(&mut self, plan_id: &ExecutionPlanId) -> Option<PausedExecution> {
        self.entries.remove(plan_id.as_str())
    }

    /// Take a pause for Approve. Enforces AwaitingReview + TTL.
    ///
    /// On timeout, the entry is removed and [`PauseError::TimedOut`] is returned.
    /// On success the entry is removed (single-use — prevents duplicate approval).
    pub fn take_for_resume(&mut self, plan_id: &ExecutionPlanId) -> Result<PausedExecution, PauseError> {
        self.take_for_resume_at(plan_id, Instant::now())
    }

    /// [`take_for_resume`] with an explicit clock (tests).
    pub fn take_for_resume_at(
        &mut self,
        plan_id: &ExecutionPlanId,
        now: Instant,
    ) -> Result<PausedExecution, PauseError> {
        let key = plan_id.as_str();
        let Some(entry) = self.entries.remove(key) else {
            return Err(PauseError::NotFound {
                plan_id: key.to_string(),
            });
        };
        if entry.is_timed_out(now, self.ttl) {
            return Err(PauseError::TimedOut {
                plan_id: key.to_string(),
            });
        }
        if entry.plan.status() != ExecutionStatus::AwaitingReview {
            return Err(PauseError::InvalidStatus {
                plan_id: key.to_string(),
                status: entry.plan.status(),
            });
        }
        Ok(entry)
    }

    /// Take a pause for Cancel / Modify (TTL still applied — timed-out pauses
    /// surface as TimedOut so callers can emit a timeout summary).
    pub fn take_for_invalidate(
        &mut self,
        plan_id: &ExecutionPlanId,
    ) -> Result<PausedExecution, PauseError> {
        self.take_for_invalidate_at(plan_id, Instant::now())
    }

    /// [`take_for_invalidate`] with an explicit clock.
    pub fn take_for_invalidate_at(
        &mut self,
        plan_id: &ExecutionPlanId,
        now: Instant,
    ) -> Result<PausedExecution, PauseError> {
        let key = plan_id.as_str();
        let Some(entry) = self.entries.remove(key) else {
            return Err(PauseError::NotFound {
                plan_id: key.to_string(),
            });
        };
        if entry.is_timed_out(now, self.ttl) {
            return Err(PauseError::TimedOut {
                plan_id: key.to_string(),
            });
        }
        Ok(entry)
    }

    /// Drop every pause (e.g. new user request that is not a review resolve).
    ///
    /// Returns removed entries so the Planner can mark plans Cancelled.
    pub fn invalidate_all(&mut self) -> Vec<PausedExecution> {
        self.entries.drain().map(|(_, entry)| entry).collect()
    }

    /// Remove and return every timed-out pause.
    pub fn expire_due(&mut self, now: Instant) -> Vec<PausedExecution> {
        let ttl = self.ttl;
        let timed_out: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.is_timed_out(now, ttl))
            .map(|(id, _)| id.clone())
            .collect();
        timed_out
            .into_iter()
            .filter_map(|id| self.entries.remove(&id))
            .collect()
    }

    /// Read-only snapshots of every active pause (diagnostics).
    pub fn snapshots(&self, now: Instant) -> Vec<PausedPlanSnapshot> {
        let mut snaps: Vec<_> = self
            .entries
            .values()
            .map(|entry| PausedPlanSnapshot::from_paused(entry, now))
            .collect();
        snaps.sort_by(|a, b| a.plan_id.cmp(&b.plan_id));
        snaps
    }
}

/// Read-only diagnostics view of a paused Execution Plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PausedPlanSnapshot {
    /// Plan id.
    pub plan_id: String,
    /// Plan lifecycle status.
    pub plan_status: String,
    /// Originating request / goal.
    pub originating_request: String,
    /// Planner intent label.
    pub planner_intent: String,
    /// Capability id.
    pub capability_id: String,
    /// Tool waiting to run.
    pub tool_id: String,
    /// Provider id, when known.
    pub provider_id: Option<String>,
    /// Estimated risk label.
    pub risk: String,
    /// Review requirement label.
    pub review_requirement: String,
    /// Reversibility label.
    pub reversibility: String,
    /// Permission requirement labels (`filesystem:write`, …).
    pub permissions: Vec<String>,
    /// Resources the plan would touch.
    pub affected_resources: Vec<String>,
    /// Step descriptions.
    pub steps: Vec<String>,
    /// Revision number.
    pub revision: u32,
    /// Parent plan when this is a Modify child.
    pub parent_plan_id: Option<String>,
    /// Diff vs parent.
    pub revision_changes: Vec<String>,
    /// Policy decision label, when evaluated.
    pub policy_decision: Option<String>,
    /// Policy explanation.
    pub policy_explanation: Option<String>,
    /// Permission decision label, when evaluated.
    pub permission_decision: Option<String>,
    /// Permission explanation.
    pub permission_explanation: Option<String>,
    /// Seconds since pause.
    pub paused_for_secs: u64,
    /// Developer-facing explanation of why execution is paused.
    pub pause_explanation: String,
    /// Developer-facing explanation of how resume works.
    pub resume_explanation: String,
}

impl PausedPlanSnapshot {
    /// Build a diagnostics snapshot from a live pause entry.
    pub fn from_paused(entry: &PausedExecution, now: Instant) -> Self {
        let plan = &entry.plan;
        let policy_decision = entry
            .policy_evaluation
            .as_ref()
            .map(|evaluation| evaluation.decision.as_str().to_string());
        let policy_explanation = entry
            .policy_evaluation
            .as_ref()
            .map(|evaluation| evaluation.explanation());
        let permission_decision = entry
            .permission_result
            .as_ref()
            .map(|result| result.decision.as_str().to_string());
        let permission_explanation = entry
            .permission_result
            .as_ref()
            .map(|result| result.explanation.clone());

        let mut reasons = Vec::new();
        reasons.push(format!(
            "Review requirement = {}",
            plan.review_requirement().as_str()
        ));
        reasons.push(format!(
            "Estimated risk = {} (tool `{}`)",
            plan.estimated_risk().as_str(),
            entry.tool_id
        ));
        if let Some(policy) = &policy_explanation {
            reasons.push(format!(
                "Policy: {} — {policy}",
                policy_decision.as_deref().unwrap_or("unknown")
            ));
        }
        if let Some(permission) = &permission_explanation {
            reasons.push(format!(
                "Permission: {} — {permission}",
                permission_decision.as_deref().unwrap_or("unknown")
            ));
        }
        if plan.revision() > 1 {
            reasons.push(format!(
                "This is revision {} (modified plan pending re-approval)",
                plan.revision()
            ));
        }

        let pause_explanation = format!(
            "Execution is PAUSED. Tools will not run until the Review Card is resolved.\n{}",
            reasons
                .iter()
                .map(|reason| format!("• {reason}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let resource = plan
            .affected_resources()
            .first()
            .cloned()
            .unwrap_or_else(|| "unspecified".into());
        let resume_explanation = format!(
            "Approve resumes plan {} without replan and executes `{}` on `{resource}`.\n\
             Modify regenerates affected steps into a child plan and re-pauses.\n\
             Cancel drops the pause; nothing executes.",
            plan.id(),
            entry.tool_id
        );

        Self {
            plan_id: plan.id().as_str().to_string(),
            plan_status: plan.status().as_str().to_string(),
            originating_request: plan.originating_request().to_string(),
            planner_intent: plan.planner_intent().as_str().to_string(),
            capability_id: plan.capability().id().to_string(),
            tool_id: entry.tool_id.clone(),
            provider_id: entry.provider_id.clone(),
            risk: plan.estimated_risk().as_str().to_string(),
            review_requirement: plan.review_requirement().as_str().to_string(),
            reversibility: plan.estimated_reversibility().as_str().to_string(),
            permissions: plan
                .permissions_required()
                .iter()
                .map(|permission| permission.label())
                .collect(),
            affected_resources: plan.affected_resources().to_vec(),
            steps: plan
                .steps()
                .iter()
                .map(|step| {
                    format!(
                        "{}. {} ({})",
                        step.order,
                        step.description,
                        step.tool_id.as_deref().unwrap_or("—")
                    )
                })
                .collect(),
            revision: plan.revision(),
            parent_plan_id: plan.parent_plan_id().map(|id| id.as_str().to_string()),
            revision_changes: plan.revision_changes().to_vec(),
            policy_decision,
            policy_explanation,
            permission_decision,
            permission_explanation,
            paused_for_secs: now.duration_since(entry.paused_at).as_secs(),
            pause_explanation,
            resume_explanation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{
        EstimatedReversibility, EstimatedRisk, ExecutionPlanParams, ExecutionStep,
        PlanPermissionRequirement, ReviewRequirement,
    };
    use jaymi_core::IntentId;

    fn awaiting_plan(label: &str) -> ExecutionPlan {
        let mut plan = ExecutionPlan::create(ExecutionPlanParams {
            originating_request: label.into(),
            planner_intent: IntentId::WriteFile,
            capability: Capability::FileManagement,
            proposed_tools: vec!["write_file".into()],
            steps: vec![ExecutionStep {
                order: 1,
                description: "Write".into(),
                tool_id: Some("write_file".into()),
                resource: Some("/tmp/a".into()),
            }],
            estimated_risk: EstimatedRisk::Medium,
            affected_resources: vec!["/tmp/a".into()],
            permissions_required: vec![PlanPermissionRequirement {
                category: "filesystem".into(),
                action: "write".into(),
            }],
            review_requirement: ReviewRequirement::Required,
            estimated_reversibility: EstimatedReversibility::PartiallyReversible,
            expected_outputs: vec!["written file".into()],
        lineage: Default::default(),
        });
        plan.mark_ready().unwrap();
        plan.mark_awaiting_review().unwrap();
        plan
    }

    fn entry(plan: ExecutionPlan) -> PausedExecution {
        PausedExecution {
            tool_id: plan.primary_tool_id().unwrap().to_string(),
            provider_id: Some("filesystem".into()),
            capability: plan.capability(),
            tool_input: ToolInput::write_file("/tmp/a", "hi"),
            plan,
            policy_evaluation: None,
            permission_result: None,
            paused_at: Instant::now(),
        }
    }

    #[test]
    fn pause() {
        let mut store = PausedPlanStore::default();
        let plan = awaiting_plan("pause");
        let id = plan.id().clone();
        store.pause(entry(plan));
        assert!(store.contains(&id));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn resume_take_is_single_use() {
        let mut store = PausedPlanStore::default();
        let plan = awaiting_plan("resume");
        let id = plan.id().clone();
        store.pause(entry(plan));
        let taken = store.take_for_resume(&id).expect("resume");
        assert_eq!(taken.plan.status(), ExecutionStatus::AwaitingReview);
        assert!(store.take_for_resume(&id).is_err());
    }

    #[test]
    fn cancel_removes_pause() {
        let mut store = PausedPlanStore::default();
        let plan = awaiting_plan("cancel");
        let id = plan.id().clone();
        store.pause(entry(plan));
        let taken = store.take_for_invalidate(&id).expect("cancel");
        assert_eq!(taken.plan_id(), &id);
        assert!(store.is_empty());
    }

    #[test]
    fn modify_invalidates_like_cancel() {
        let mut store = PausedPlanStore::default();
        let plan = awaiting_plan("modify");
        let id = plan.id().clone();
        store.pause(entry(plan));
        assert!(store.take_for_invalidate(&id).is_ok());
        assert!(!store.contains(&id));
    }

    #[test]
    fn timeout() {
        let mut store = PausedPlanStore::new(Duration::from_millis(1));
        let plan = awaiting_plan("timeout");
        let id = plan.id().clone();
        let mut paused = entry(plan);
        paused.paused_at = Instant::now() - Duration::from_secs(5);
        store.pause(paused);
        let err = store
            .take_for_resume_at(&id, Instant::now())
            .expect_err("timed out");
        assert!(matches!(err, PauseError::TimedOut { .. }));
        assert!(store.is_empty());
    }

    #[test]
    fn duplicate_approval() {
        let mut store = PausedPlanStore::default();
        let plan = awaiting_plan("dup");
        let id = plan.id().clone();
        store.pause(entry(plan));
        store.take_for_resume(&id).unwrap();
        let err = store.take_for_resume(&id).expect_err("duplicate");
        assert!(matches!(err, PauseError::NotFound { .. }));
    }
}
