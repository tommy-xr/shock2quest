use anyhow::{Context, Result};
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{debug, info, warn};

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub name: String,
    pub file_path: PathBuf,
    pub content: String,
    pub weight: u32,
    pub metadata: PromptMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub risk_level: RiskLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,    // Safe operations like documentation, analysis
    Medium, // Code changes, refactoring
    High,   // System changes, major refactoring
}

impl Default for PromptMetadata {
    fn default() -> Self {
        Self {
            title: None,
            description: None,
            tags: Vec::new(),
            risk_level: RiskLevel::Medium,
        }
    }
}

/// Discover and load all prompts from the prompts directory
pub async fn discover_prompts(config: &Config) -> Result<Vec<Prompt>> {
    let prompts_dir = config.prompts_dir();
    debug!("Discovering prompts in directory: {}", prompts_dir.display());

    if !prompts_dir.exists() {
        warn!("Prompts directory does not exist: {}", prompts_dir.display());
        return Ok(Vec::new());
    }

    let mut entries = fs::read_dir(&prompts_dir)
        .await
        .with_context(|| format!("Failed to read prompts directory: {}", prompts_dir.display()))?;

    let mut prompts = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        // Only process .md files
        if let Some(extension) = path.extension() {
            if extension == "md" {
                match load_prompt(&path, config).await {
                    Ok(prompt) => {
                        debug!("Loaded prompt: {} (weight: {})", prompt.name, prompt.weight);
                        prompts.push(prompt);
                    }
                    Err(e) => {
                        warn!("Failed to load prompt from {}: {}", path.display(), e);
                    }
                }
            }
        }
    }

    info!("Discovered {} prompts", prompts.len());
    Ok(prompts)
}

/// Load a single prompt from a file
pub async fn load_prompt(path: &Path, config: &Config) -> Result<Prompt> {
    let content = fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read prompt file: {}", path.display()))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Parse frontmatter and extract metadata
    let (metadata, prompt_content) = parse_prompt_content(&content)?;

    // Get weight from config or use default
    let weight = config
        .shodan
        .prompt_weights
        .get(&file_name)
        .copied()
        .unwrap_or(1);

    let prompt = Prompt {
        name: file_name,
        file_path: path.to_path_buf(),
        content: prompt_content,
        weight,
        metadata,
    };

    validate_prompt(&prompt)?;
    Ok(prompt)
}

/// Parse prompt content and extract frontmatter metadata
fn parse_prompt_content(content: &str) -> Result<(PromptMetadata, String)> {
    let content = content.trim();

    if content.starts_with("---") {
        // Extract YAML frontmatter
        let lines: Vec<&str> = content.lines().collect();
        if let Some(end_index) = lines.iter().skip(1).position(|&line| line.trim() == "---") {
            let frontmatter = lines[1..end_index + 1].join("\n");
            let prompt_content = lines[end_index + 2..].join("\n").trim().to_string();

            let metadata: PromptMetadata = serde_yaml::from_str(&frontmatter)
                .unwrap_or_else(|_| PromptMetadata::default());

            return Ok((metadata, prompt_content));
        }
    }

    // No frontmatter, use defaults and full content
    Ok((PromptMetadata::default(), content.to_string()))
}

/// Validate a prompt for safety and correctness
fn validate_prompt(prompt: &Prompt) -> Result<()> {
    // Check minimum content length
    if prompt.content.trim().len() < 10 {
        return Err(anyhow::anyhow!(
            "Prompt '{}' is too short (minimum 10 characters)",
            prompt.name
        ));
    }

    // Security checks for dangerous patterns
    let dangerous_patterns = [
        "rm -rf",
        "sudo rm",
        "format /",
        "del /s",
        "DROP TABLE",
        "DROP DATABASE",
        "system(",
        "exec(",
        "eval(",
        "__import__",
    ];

    let content_lower = prompt.content.to_lowercase();
    for pattern in &dangerous_patterns {
        if content_lower.contains(&pattern.to_lowercase()) {
            warn!(
                "Security: Prompt '{}' contains potentially dangerous pattern: {}",
                prompt.name, pattern
            );
        }
    }

    // Check for prompt injection patterns
    let injection_patterns = [
        "ignore previous instructions",
        "forget your role",
        "you are now",
        "new instructions:",
        "system: ",
        "admin mode",
        "developer mode",
    ];

    for pattern in &injection_patterns {
        if content_lower.contains(&pattern.to_lowercase()) {
            warn!(
                "Security: Prompt '{}' contains potential injection pattern: {}",
                prompt.name, pattern
            );
        }
    }

    Ok(())
}

/// Select a random prompt based on weights
pub fn select_random_prompt(prompts: &[Prompt]) -> Result<&Prompt> {
    if prompts.is_empty() {
        return Err(anyhow::anyhow!("No prompts available"));
    }

    // Calculate total weight
    let total_weight: u32 = prompts.iter().map(|p| p.weight).sum();

    if total_weight == 0 {
        return Err(anyhow::anyhow!("All prompts have zero weight"));
    }

    // Generate random number
    let mut rng = thread_rng();
    let random_value = rng.gen_range(0..total_weight);

    // Select prompt based on weight
    let mut current_weight = 0;
    for prompt in prompts {
        current_weight += prompt.weight;
        if random_value < current_weight {
            debug!(
                "Selected prompt: {} (weight: {}, random: {}/{})",
                prompt.name, prompt.weight, random_value, total_weight
            );
            return Ok(prompt);
        }
    }

    // Fallback to last prompt (shouldn't happen)
    prompts.last().ok_or_else(|| anyhow::anyhow!("No prompts available"))
}

/// Get prompt statistics
pub fn get_prompt_stats(prompts: &[Prompt]) -> PromptStats {
    let total_weight: u32 = prompts.iter().map(|p| p.weight).sum();
    let mut risk_counts = HashMap::new();
    let mut tag_counts = HashMap::new();

    for prompt in prompts {
        // Count risk levels
        let risk_key = format!("{:?}", prompt.metadata.risk_level);
        *risk_counts.entry(risk_key).or_insert(0) += 1;

        // Count tags
        for tag in &prompt.metadata.tags {
            *tag_counts.entry(tag.clone()).or_insert(0) += 1;
        }
    }

    PromptStats {
        total_prompts: prompts.len(),
        total_weight,
        risk_distribution: risk_counts,
        tag_distribution: tag_counts,
        average_weight: if prompts.is_empty() { 0.0 } else { total_weight as f64 / prompts.len() as f64 },
    }
}

#[derive(Debug, Clone)]
pub struct PromptStats {
    pub total_prompts: usize,
    pub total_weight: u32,
    pub risk_distribution: HashMap<String, usize>,
    pub tag_distribution: HashMap<String, usize>,
    pub average_weight: f64,
}

/// Format a prompt for Claude Code execution
pub fn format_prompt_for_execution(prompt: &Prompt) -> String {
    let mut formatted = String::new();

    // Add metadata as comments if available
    if let Some(title) = &prompt.metadata.title {
        formatted.push_str(&format!("# {}\n\n", title));
    }

    if let Some(description) = &prompt.metadata.description {
        formatted.push_str(&format!("## Description\n{}\n\n", description));
    }

    if !prompt.metadata.tags.is_empty() {
        formatted.push_str(&format!("## Tags\n{}\n\n", prompt.metadata.tags.join(", ")));
    }

    formatted.push_str(&format!("## Risk Level\n{:?}\n\n", prompt.metadata.risk_level));

    // Add the actual prompt content
    formatted.push_str("## Task\n");
    formatted.push_str(&prompt.content);

    formatted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prompt_content_with_frontmatter() {
        let content = r#"---
title: "Test Prompt"
description: "A test prompt"
tags: ["test", "example"]
risk_level: "Low"
---

This is the prompt content."#;

        let (metadata, prompt_content) = parse_prompt_content(content).unwrap();
        assert_eq!(metadata.title, Some("Test Prompt".to_string()));
        assert_eq!(metadata.tags, vec!["test", "example"]);
        assert_eq!(prompt_content, "This is the prompt content.");
    }

    #[test]
    fn test_parse_prompt_content_without_frontmatter() {
        let content = "This is a simple prompt without frontmatter.";
        let (metadata, prompt_content) = parse_prompt_content(content).unwrap();
        assert_eq!(metadata.title, None);
        assert_eq!(prompt_content, content);
    }

    #[test]
    fn test_select_random_prompt_weighted() {
        let prompts = vec![
            Prompt {
                name: "low_weight".to_string(),
                file_path: PathBuf::from("test"),
                content: "test content".to_string(),
                weight: 1,
                metadata: PromptMetadata::default(),
            },
            Prompt {
                name: "high_weight".to_string(),
                file_path: PathBuf::from("test"),
                content: "test content".to_string(),
                weight: 10,
                metadata: PromptMetadata::default(),
            },
        ];

        // Run multiple times to check distribution
        let mut high_weight_count = 0;
        for _ in 0..100 {
            let selected = select_random_prompt(&prompts).unwrap();
            if selected.name == "high_weight" {
                high_weight_count += 1;
            }
        }

        // High weight prompt should be selected more often
        assert!(high_weight_count > 50);
    }
}