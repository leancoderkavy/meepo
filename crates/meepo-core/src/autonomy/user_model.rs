//! User preference learning for the autonomous agent
//!
//! Tracks interaction patterns (active hours, preferred channels, common topics)
//! and stores learned preferences in the knowledge graph for the agent to use
//! when making autonomous decisions.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use chrono::{Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

use meepo_knowledge::KnowledgeDb;

/// Aggregated user interaction patterns
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserProfile {
    /// Hour-of-day histogram (0-23) — how many messages per hour
    pub active_hours: [u32; 24],
    /// Day-of-week histogram (0=Mon, 6=Sun) — how many messages per day
    pub active_days: [u32; 7],
    /// Channel usage counts
    pub channel_usage: HashMap<String, u32>,
    /// Total interactions tracked
    pub total_interactions: u32,
}

impl UserProfile {
    /// Get the most active hour (0-23), defaults to 9 if no data
    pub fn peak_hour(&self) -> usize {
        if self.total_interactions == 0 {
            return 9;
        }
        self.active_hours
            .iter()
            .enumerate()
            .max_by_key(|(_, count)| *count)
            .map(|(hour, _)| hour)
            .unwrap_or(9)
    }

    /// Get the most active day (0=Mon, 6=Sun), defaults to 0 (Monday) if no data
    pub fn peak_day(&self) -> usize {
        if self.total_interactions == 0 {
            return 0;
        }
        self.active_days
            .iter()
            .enumerate()
            .max_by_key(|(_, count)| *count)
            .map(|(day, _)| day)
            .unwrap_or(0)
    }

    /// Get the preferred channel (most used)
    pub fn preferred_channel(&self) -> Option<&str> {
        self.channel_usage
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(channel, _)| channel.as_str())
    }

    /// Check if the user is likely active right now based on historical patterns
    pub fn is_likely_active(&self) -> bool {
        if self.total_interactions < 10 {
            return true; // Not enough data, assume active
        }
        let now = Utc::now();
        let hour = now.hour() as usize;
        let avg = self.total_interactions as f64 / 24.0;
        self.active_hours[hour] as f64 > avg * 0.5
    }
}

/// Tracks and learns user preferences over time
pub struct UserModel {
    db: Arc<KnowledgeDb>,
}

impl UserModel {
    pub fn new(db: Arc<KnowledgeDb>) -> Self {
        Self { db }
    }

    /// Record an interaction from a user
    pub async fn record_interaction(&self, channel: &str) -> Result<()> {
        let now = Utc::now();
        let hour = now.hour();
        let day = now.weekday().num_days_from_monday();

        // Store as a preference in the knowledge graph
        let value = serde_json::json!({
            "hour": hour,
            "day": day,
            "channel": channel,
            "timestamp": now.to_rfc3339(),
        });

        self.db
            .upsert_preference("user_model", "last_interaction", value, 1.0, Some("auto"))
            .await?;

        debug!(
            "Recorded interaction: hour={}, day={}, channel={}",
            hour, day, channel
        );
        Ok(())
    }

    /// Build a user profile from stored conversation history
    pub async fn build_profile(&self) -> Result<UserProfile> {
        let conversations = self.db.get_recent_conversations(None, 500).await?;

        let mut profile = UserProfile::default();

        for conv in &conversations {
            if conv.sender == "meepo" {
                continue; // Only count user messages
            }

            let hour = conv.created_at.hour() as usize;
            let day = conv.created_at.weekday().num_days_from_monday() as usize;
            profile.active_hours[hour] += 1;
            profile.active_days[day] += 1;

            *profile
                .channel_usage
                .entry(conv.channel.clone())
                .or_insert(0) += 1;

            profile.total_interactions += 1;
        }

        debug!(
            "Built user profile: {} interactions, peak hour={}, preferred channel={:?}",
            profile.total_interactions,
            profile.peak_hour(),
            profile.preferred_channel()
        );

        Ok(profile)
    }

    /// Generate a brief summary of user patterns for the agent's context
    pub async fn summarize_for_agent(&self) -> Result<String> {
        let profile = self.build_profile().await?;

        if profile.total_interactions < 5 {
            return Ok("Not enough interaction data to build a user profile yet.".to_string());
        }

        let day_names = [
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
            "Sunday",
        ];

        let mut summary = String::from("## User Patterns\n\n");
        summary.push_str(&format!(
            "- Most active around {}:00\n",
            profile.peak_hour()
        ));
        summary.push_str(&format!(
            "- Most active on {}\n",
            day_names[profile.peak_day()]
        ));
        if let Some(channel) = profile.preferred_channel() {
            summary.push_str(&format!("- Preferred channel: {}\n", channel));
        }
        summary.push_str(&format!(
            "- Total interactions tracked: {}\n",
            profile.total_interactions
        ));

        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_profile_defaults() {
        let profile = UserProfile::default();
        assert_eq!(profile.total_interactions, 0);
        assert_eq!(profile.peak_hour(), 9); // default when all zeros
    }

    #[test]
    fn test_user_profile_peak_hour() {
        let mut profile = UserProfile::default();
        profile.total_interactions = 15;
        profile.active_hours[14] = 10;
        profile.active_hours[9] = 5;
        assert_eq!(profile.peak_hour(), 14);
    }

    #[test]
    fn test_user_profile_preferred_channel() {
        let mut profile = UserProfile::default();
        profile.channel_usage.insert("discord".to_string(), 50);
        profile.channel_usage.insert("imessage".to_string(), 30);
        assert_eq!(profile.preferred_channel(), Some("discord"));
    }

    #[test]
    fn test_is_likely_active_insufficient_data() {
        let profile = UserProfile::default();
        assert!(profile.is_likely_active()); // Not enough data, assume active
    }

    #[test]
    fn test_user_profile_peak_day() {
        let mut profile = UserProfile::default();
        assert_eq!(profile.peak_day(), 0); // default when no data

        profile.total_interactions = 20;
        profile.active_days[4] = 15; // Friday
        profile.active_days[0] = 5;  // Monday
        assert_eq!(profile.peak_day(), 4);
    }

    #[test]
    fn test_user_profile_preferred_channel_none() {
        let profile = UserProfile::default();
        assert!(profile.preferred_channel().is_none());
    }

    #[test]
    fn test_user_profile_serde_roundtrip() {
        let mut profile = UserProfile::default();
        profile.total_interactions = 10;
        profile.active_hours[9] = 5;
        profile.channel_usage.insert("discord".to_string(), 7);

        let json = serde_json::to_string(&profile).unwrap();
        let parsed: UserProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_interactions, 10);
        assert_eq!(parsed.active_hours[9], 5);
        assert_eq!(parsed.channel_usage.get("discord"), Some(&7));
    }

    #[tokio::test]
    async fn test_user_model_record_interaction() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&dir.path().join("test.db")).unwrap());
        let model = UserModel::new(db.clone());

        model.record_interaction("discord").await.unwrap();

        let prefs = db.get_preferences(Some("user_model")).await.unwrap();
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0].key, "last_interaction");
    }

    #[tokio::test]
    async fn test_user_model_build_profile_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&dir.path().join("test.db")).unwrap());
        let model = UserModel::new(db);

        let profile = model.build_profile().await.unwrap();
        assert_eq!(profile.total_interactions, 0);
        assert_eq!(profile.peak_hour(), 9);
    }

    #[tokio::test]
    async fn test_user_model_build_profile_with_conversations() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&dir.path().join("test.db")).unwrap());

        // Insert some conversations from a user (not "meepo")
        for _ in 0..5 {
            db.insert_conversation("discord", "alice", "hello", None)
                .await
                .unwrap();
        }
        // Meepo's own messages should be excluded
        db.insert_conversation("discord", "meepo", "hi back", None)
            .await
            .unwrap();

        let model = UserModel::new(db);
        let profile = model.build_profile().await.unwrap();
        assert_eq!(profile.total_interactions, 5);
        assert_eq!(profile.channel_usage.get("discord"), Some(&5));
    }

    #[tokio::test]
    async fn test_user_model_summarize_insufficient_data() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&dir.path().join("test.db")).unwrap());
        let model = UserModel::new(db);

        let summary = model.summarize_for_agent().await.unwrap();
        assert!(summary.contains("Not enough interaction data"));
    }

    #[tokio::test]
    async fn test_user_model_summarize_with_data() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(KnowledgeDb::new(&dir.path().join("test.db")).unwrap());

        for _ in 0..10 {
            db.insert_conversation("slack", "bob", "msg", None)
                .await
                .unwrap();
        }

        let model = UserModel::new(db);
        let summary = model.summarize_for_agent().await.unwrap();
        assert!(summary.contains("User Patterns"));
        assert!(summary.contains("Most active"));
        assert!(summary.contains("slack"));
        assert!(summary.contains("10"));
    }
}
