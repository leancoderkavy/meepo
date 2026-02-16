//! Goal management for the autonomous agent
//!
//! Evaluates due goals by building a situation report and asking the agent
//! whether a goal should be acted on, deferred, or marked complete.

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use meepo_knowledge::{Goal, KnowledgeDb};

/// Result of evaluating a single goal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalEvaluation {
    pub goal_id: String,
    pub decision: GoalDecision,
    pub reasoning: String,
    pub confidence: f64,
    /// If the decision is Act, this is the action prompt to send to the agent
    pub action_prompt: Option<String>,
}

/// What to do with a goal after evaluation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalDecision {
    /// Take action toward this goal now
    Act,
    /// Defer — not the right time or not enough info
    Defer,
    /// Goal has been achieved
    Complete,
    /// Goal is no longer relevant
    Abandon,
    /// Need more information before deciding
    Investigate,
}

impl std::fmt::Display for GoalDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Act => write!(f, "act"),
            Self::Defer => write!(f, "defer"),
            Self::Complete => write!(f, "complete"),
            Self::Abandon => write!(f, "abandon"),
            Self::Investigate => write!(f, "investigate"),
        }
    }
}

/// Evaluates goals and decides what actions to take
pub struct GoalEvaluator {
    db: Arc<KnowledgeDb>,
    min_confidence: f64,
}

impl GoalEvaluator {
    pub fn new(db: Arc<KnowledgeDb>, min_confidence: f64) -> Self {
        Self { db, min_confidence }
    }

    /// Get goals that are due for evaluation
    pub async fn get_due_goals(&self) -> Result<Vec<Goal>> {
        self.db.get_due_goals().await
    }

    /// Build a goal evaluation prompt for the agent to reason about
    pub fn build_evaluation_prompt(&self, goals: &[Goal]) -> Option<String> {
        if goals.is_empty() {
            return None;
        }

        let mut prompt = String::from(
            "You have the following active goals that are due for evaluation. \
             For each goal, decide what to do.\n\n",
        );

        for (i, goal) in goals.iter().enumerate() {
            prompt.push_str(&format!(
                "Goal {}: [{}] {}\n  Priority: {}\n  Success criteria: {}\n",
                i + 1,
                goal.id,
                goal.description,
                goal.priority,
                goal.success_criteria.as_deref().unwrap_or("(none)"),
            ));
            if let Some(ref strategy) = goal.strategy {
                prompt.push_str(&format!("  Current strategy: {}\n", strategy));
            }
            prompt.push('\n');
        }

        prompt.push_str(
            "For each goal, respond with a JSON array of objects:\n\
             ```json\n\
             [{\"goal_id\": \"...\", \"decision\": \"act|defer|complete|abandon|investigate\", \
             \"confidence\": 0.0-1.0, \"reasoning\": \"...\", \"action_prompt\": \"...\"}]\n\
             ```\n\
             Only set action_prompt if decision is \"act\" — describe the specific action to take.\n\
             Be conservative: only \"act\" if confidence >= 0.7 and the action is clearly beneficial.",
        );

        Some(prompt)
    }

    /// Parse the agent's evaluation response into GoalEvaluation structs
    pub fn parse_evaluations(&self, response: &str) -> Vec<GoalEvaluation> {
        // Try to extract JSON from the response
        let json_str = extract_json_array(response);

        match serde_json::from_str::<Vec<GoalEvaluation>>(&json_str) {
            Ok(evals) => {
                debug!("Parsed {} goal evaluations", evals.len());
                evals
            }
            Err(e) => {
                warn!("Failed to parse goal evaluations: {}", e);
                vec![]
            }
        }
    }

    /// Apply evaluation results: update goal statuses and return action prompts
    pub async fn apply_evaluations(
        &self,
        evaluations: &[GoalEvaluation],
    ) -> Result<Vec<GoalEvaluation>> {
        let mut actions = Vec::new();

        for eval in evaluations {
            match eval.decision {
                GoalDecision::Complete => {
                    info!("Goal {} completed: {}", eval.goal_id, eval.reasoning);
                    self.db
                        .update_goal_status(&eval.goal_id, "completed")
                        .await?;
                }
                GoalDecision::Abandon => {
                    info!("Goal {} abandoned: {}", eval.goal_id, eval.reasoning);
                    self.db.update_goal_status(&eval.goal_id, "failed").await?;
                }
                GoalDecision::Act => {
                    if eval.confidence >= self.min_confidence {
                        info!(
                            "Goal {} — acting (confidence: {:.2}): {}",
                            eval.goal_id, eval.confidence, eval.reasoning
                        );
                        self.db
                            .update_goal_checked(&eval.goal_id, Some(&eval.reasoning))
                            .await?;
                        actions.push(eval.clone());
                    } else {
                        debug!(
                            "Goal {} — confidence too low ({:.2} < {:.2}), deferring",
                            eval.goal_id, eval.confidence, self.min_confidence
                        );
                        self.db
                            .update_goal_checked(&eval.goal_id, Some(&eval.reasoning))
                            .await?;
                    }
                }
                GoalDecision::Defer | GoalDecision::Investigate => {
                    debug!(
                        "Goal {} — {}: {}",
                        eval.goal_id, eval.decision, eval.reasoning
                    );
                    self.db
                        .update_goal_checked(&eval.goal_id, Some(&eval.reasoning))
                        .await?;
                }
            }
        }

        Ok(actions)
    }
}

/// Extract a JSON array from a response that may contain markdown fences
fn extract_json_array(text: &str) -> String {
    // Try to find JSON between ```json ... ``` fences
    if let Some(start) = text.find("```json") {
        let after_fence = &text[start + 7..];
        if let Some(end) = after_fence.find("```") {
            return after_fence[..end].trim().to_string();
        }
    }
    // Try to find JSON between ``` ... ``` fences
    if let Some(start) = text.find("```") {
        let after_fence = &text[start + 3..];
        if let Some(end) = after_fence.find("```") {
            let inner = after_fence[..end].trim();
            if inner.starts_with('[') {
                return inner.to_string();
            }
        }
    }
    // Try to find a raw JSON array
    if let Some(start) = text.find('[')
        && let Some(end) = text.rfind(']')
    {
        return text[start..=end].to_string();
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_array_fenced() {
        let input = "Here are my evaluations:\n```json\n[{\"goal_id\": \"g1\"}]\n```\nDone.";
        assert_eq!(extract_json_array(input), "[{\"goal_id\": \"g1\"}]");
    }

    #[test]
    fn test_extract_json_array_raw() {
        let input = "Result: [{\"goal_id\": \"g1\"}]";
        assert_eq!(extract_json_array(input), "[{\"goal_id\": \"g1\"}]");
    }

    #[test]
    fn test_parse_evaluations() {
        let db = Arc::new(
            KnowledgeDb::new(&tempfile::TempDir::new().unwrap().path().join("test.db")).unwrap(),
        );
        let evaluator = GoalEvaluator::new(db, 0.7);

        let json = r#"[{"goal_id": "g1", "decision": "act", "confidence": 0.9, "reasoning": "Ready to go", "action_prompt": "Do the thing"}]"#;
        let evals = evaluator.parse_evaluations(json);
        assert_eq!(evals.len(), 1);
        assert_eq!(evals[0].decision, GoalDecision::Act);
        assert_eq!(evals[0].confidence, 0.9);
    }

    #[test]
    fn test_build_evaluation_prompt_empty() {
        let db = Arc::new(
            KnowledgeDb::new(&tempfile::TempDir::new().unwrap().path().join("test.db")).unwrap(),
        );
        let evaluator = GoalEvaluator::new(db, 0.7);
        assert!(evaluator.build_evaluation_prompt(&[]).is_none());
    }

    #[test]
    fn test_goal_decision_display() {
        assert_eq!(GoalDecision::Act.to_string(), "act");
        assert_eq!(GoalDecision::Defer.to_string(), "defer");
        assert_eq!(GoalDecision::Complete.to_string(), "complete");
        assert_eq!(GoalDecision::Abandon.to_string(), "abandon");
        assert_eq!(GoalDecision::Investigate.to_string(), "investigate");
    }

    #[test]
    fn test_goal_decision_serde_roundtrip() {
        let decisions = [
            GoalDecision::Act,
            GoalDecision::Defer,
            GoalDecision::Complete,
            GoalDecision::Abandon,
            GoalDecision::Investigate,
        ];
        for d in &decisions {
            let json = serde_json::to_string(d).unwrap();
            let parsed: GoalDecision = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, d);
        }
    }

    #[test]
    fn test_parse_evaluations_invalid_json() {
        let db = Arc::new(
            KnowledgeDb::new(&tempfile::TempDir::new().unwrap().path().join("test.db")).unwrap(),
        );
        let evaluator = GoalEvaluator::new(db, 0.7);
        let evals = evaluator.parse_evaluations("this is not json at all");
        assert!(evals.is_empty());
    }

    #[test]
    fn test_parse_evaluations_fenced_json() {
        let db = Arc::new(
            KnowledgeDb::new(&tempfile::TempDir::new().unwrap().path().join("test.db")).unwrap(),
        );
        let evaluator = GoalEvaluator::new(db, 0.7);
        let response = "Here's my evaluation:\n```json\n[{\"goal_id\": \"g1\", \"decision\": \"defer\", \"confidence\": 0.4, \"reasoning\": \"Not ready\", \"action_prompt\": null}]\n```";
        let evals = evaluator.parse_evaluations(response);
        assert_eq!(evals.len(), 1);
        assert_eq!(evals[0].decision, GoalDecision::Defer);
    }

    #[test]
    fn test_build_evaluation_prompt_with_goals() {
        let db = Arc::new(
            KnowledgeDb::new(&tempfile::TempDir::new().unwrap().path().join("test.db")).unwrap(),
        );
        let evaluator = GoalEvaluator::new(db, 0.7);
        let goals = vec![Goal {
            id: "g1".to_string(),
            description: "Review PRs".to_string(),
            status: "active".to_string(),
            priority: 3,
            success_criteria: Some("All PRs reviewed".to_string()),
            strategy: Some("Check GitHub".to_string()),
            check_interval_secs: 3600,
            last_checked_at: None,
            source_channel: Some("discord".to_string()),
            source: "user".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }];
        let prompt = evaluator.build_evaluation_prompt(&goals);
        assert!(prompt.is_some());
        let prompt = prompt.unwrap();
        assert!(prompt.contains("Review PRs"));
        assert!(prompt.contains("All PRs reviewed"));
        assert!(prompt.contains("Check GitHub"));
        assert!(prompt.contains("goal_id"));
    }

    #[test]
    fn test_extract_json_array_plain_fences() {
        let input = "Result:\n```\n[{\"goal_id\": \"g1\"}]\n```";
        assert_eq!(extract_json_array(input), "[{\"goal_id\": \"g1\"}]");
    }

    #[test]
    fn test_extract_json_array_no_fences_no_brackets() {
        let input = "no json here";
        assert_eq!(extract_json_array(input), "no json here");
    }

    #[tokio::test]
    async fn test_apply_evaluations() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&dir.path().join("test.db")).unwrap());
        let evaluator = GoalEvaluator::new(db.clone(), 0.5);

        // Create goals
        let g1 = db
            .insert_goal("Goal A", 3, 3600, Some("Done A"), None, "user")
            .await
            .unwrap();
        let g2 = db
            .insert_goal("Goal B", 2, 3600, None, None, "user")
            .await
            .unwrap();
        let g3 = db
            .insert_goal("Goal C", 1, 3600, None, None, "user")
            .await
            .unwrap();

        let evals = vec![
            GoalEvaluation {
                goal_id: g1.clone(),
                decision: GoalDecision::Complete,
                reasoning: "All done".to_string(),
                confidence: 0.9,
                action_prompt: None,
            },
            GoalEvaluation {
                goal_id: g2.clone(),
                decision: GoalDecision::Act,
                reasoning: "Time to act".to_string(),
                confidence: 0.8,
                action_prompt: Some("Do the thing".to_string()),
            },
            GoalEvaluation {
                goal_id: g3.clone(),
                decision: GoalDecision::Abandon,
                reasoning: "No longer relevant".to_string(),
                confidence: 0.7,
                action_prompt: None,
            },
        ];

        let actions = evaluator.apply_evaluations(&evals).await.unwrap();
        // Only g2 (Act with confidence >= 0.5) should be in actions
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].goal_id, g2);

        // g1 completed, g3 abandoned (failed), g2 still active (Act just marks checked)
        let active = db.get_active_goals().await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, g2);
    }

    #[tokio::test]
    async fn test_apply_evaluations_low_confidence_act() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&dir.path().join("test.db")).unwrap());
        let evaluator = GoalEvaluator::new(db.clone(), 0.8);

        let g1 = db
            .insert_goal("Goal X", 3, 3600, None, None, "user")
            .await
            .unwrap();

        let evals = vec![GoalEvaluation {
            goal_id: g1,
            decision: GoalDecision::Act,
            reasoning: "Maybe".to_string(),
            confidence: 0.5, // below min_confidence of 0.8
            action_prompt: Some("Do it".to_string()),
        }];

        let actions = evaluator.apply_evaluations(&evals).await.unwrap();
        assert!(actions.is_empty()); // confidence too low
    }
}
