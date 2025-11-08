use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, error, info};

use crate::agent::{AgentKind, AgentOutput, AutomationAgent};
use crate::claude_code::ClaudeCodeManager;
use crate::codex::CodexCodeManager;
use crate::config::Config;
use crate::git::{detect_active_sessions, ensure_clean_working_directory};
use crate::github::PRMonitor;
use crate::prompts::{Prompt, discover_prompts, select_random_prompt};

/// Main orchestrator that manages the autonomous agent execution cycle
pub struct Orchestrator {
    config: Config,
    agent: Box<dyn AutomationAgent>,
    pr_monitor: PRMonitor,
    state: OrchestrationState,
    available_prompts: Vec<Prompt>,
}

/// State of the current orchestration cycle
#[derive(Debug, Clone)]
pub struct OrchestrationState {
    pub last_run: Option<Instant>,
    pub current_cycle: Option<OrchestrationCycle>,
    pub cycles_completed: u32,
    pub is_running: bool,
    pub should_stop: bool,
}

/// Information about a single orchestration cycle
#[derive(Debug, Clone)]
pub struct OrchestrationCycle {
    pub id: String,
    pub start_time: Instant,
    pub selected_prompt: String,
    pub agent_session_id: Option<String>,
    pub created_pr_number: Option<u32>,
    pub phase: CyclePhase,
    pub execution_log: Vec<String>,
}

/// Current phase of the orchestration cycle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CyclePhase {
    Initializing,
    CheckingPrerequisites,
    SelectingPrompt,
    ExecutingAgent,
    MonitoringPR,
    WaitingForCI,
    Completed,
    Failed(String),
}

impl Orchestrator {
    /// Create a new orchestrator instance
    pub async fn new(config: Config, agent_kind: AgentKind) -> Result<Self> {
        let agent_config = config.clone();
        let pr_monitor = PRMonitor::new(config.clone());

        let agent: Box<dyn AutomationAgent> = match agent_kind {
            AgentKind::Claude => Box::new(ClaudeCodeManager::new(agent_config)),
            AgentKind::Codex => Box::new(CodexCodeManager::new(agent_config)),
        };

        let state = OrchestrationState {
            last_run: None,
            current_cycle: None,
            cycles_completed: 0,
            is_running: false,
            should_stop: false,
        };

        let available_prompts = discover_prompts(&config).await?;
        info!(
            "Loaded {} prompts for orchestration",
            available_prompts.len()
        );

        Ok(Self {
            config,
            agent,
            pr_monitor,
            state,
            available_prompts,
        })
    }

    /// Start the main orchestration loop
    pub async fn start_orchestration(&mut self) -> Result<()> {
        info!("🚀 Starting Shodan orchestration loop");

        if self.available_prompts.is_empty() {
            return Err(anyhow::anyhow!("No prompts available for orchestration"));
        }

        self.state.is_running = true;
        self.state.should_stop = false;

        info!("🤖 Using agent: {}", self.agent.display_name());

        // Parse interval from config
        let interval = Duration::from_secs(
            self.config
                .parse_orchestration_interval()
                .context("Failed to parse orchestration interval")?,
        );

        info!("📅 Orchestration interval: {:?}", interval);
        info!("🎯 Available prompts: {}", self.available_prompts.len());

        while !self.state.should_stop {
            // Check if enough time has passed since last run
            if self.should_run_cycle() {
                info!("⏰ Time for new orchestration cycle");

                match self.run_orchestration_cycle().await {
                    Ok(_) => {
                        self.state.cycles_completed += 1;
                        info!(
                            "✅ Orchestration cycle #{} completed successfully",
                            self.state.cycles_completed
                        );
                    }
                    Err(e) => {
                        error!("❌ Orchestration cycle failed: {}", e);
                        // Continue running even if one cycle fails
                    }
                }

                self.state.last_run = Some(Instant::now());
                self.state.current_cycle = None;
            } else {
                // Calculate time until next run
                let next_run_time = self.time_until_next_run(interval);
                debug!("⏳ Next run in {:?}", next_run_time);
            }

            // Sleep for a short interval before checking again
            sleep(Duration::from_secs(60)).await; // Check every minute
        }

        self.state.is_running = false;
        info!("🛑 Orchestration loop stopped");
        Ok(())
    }

    /// Run a single orchestration cycle once
    pub async fn run_once(&mut self) -> Result<OrchestrationCycle> {
        info!("🎯 Running single orchestration cycle");
        self.run_orchestration_cycle().await
    }

    /// Stop the orchestration loop
    pub fn stop(&mut self) {
        info!("🛑 Stopping orchestration loop");
        self.state.should_stop = true;
    }

    /// Get current orchestration state
    pub fn get_state(&self) -> &OrchestrationState {
        &self.state
    }

    /// Check if we should run a new cycle based on timing
    fn should_run_cycle(&self) -> bool {
        if let Some(last_run) = self.state.last_run {
            let interval = Duration::from_secs(
                self.config.parse_orchestration_interval().unwrap_or(3600), // Default to 1 hour
            );
            last_run.elapsed() >= interval
        } else {
            true // First run
        }
    }

    /// Calculate time until next run
    fn time_until_next_run(&self, interval: Duration) -> Duration {
        if let Some(last_run) = self.state.last_run {
            let elapsed = last_run.elapsed();
            if elapsed >= interval {
                Duration::from_secs(0)
            } else {
                interval - elapsed
            }
        } else {
            Duration::from_secs(0)
        }
    }

    /// Run a complete orchestration cycle
    async fn run_orchestration_cycle(&mut self) -> Result<OrchestrationCycle> {
        let cycle_id = generate_cycle_id();
        info!("🔄 Starting orchestration cycle: {}", cycle_id);

        let mut cycle = OrchestrationCycle {
            id: cycle_id.clone(),
            start_time: Instant::now(),
            selected_prompt: String::new(),
            agent_session_id: None,
            created_pr_number: None,
            phase: CyclePhase::Initializing,
            execution_log: Vec::new(),
        };

        cycle.log("🔄 Starting orchestration cycle");
        self.state.current_cycle = Some(cycle.clone());

        // Phase 1: Check prerequisites
        cycle.phase = CyclePhase::CheckingPrerequisites;
        cycle.log("📋 Checking prerequisites");
        self.check_prerequisites(&mut cycle).await?;

        // Phase 2: Select prompt
        cycle.phase = CyclePhase::SelectingPrompt;
        cycle.log("🎲 Selecting random prompt");
        let selected_prompt = self.select_prompt(&mut cycle).await?;

        // Phase 3: Execute agent
        cycle.phase = CyclePhase::ExecutingAgent;
        let agent_name = self.agent.display_name();
        cycle.log(&format!(
            "🤖 Executing {} with prompt: {}",
            agent_name, selected_prompt.name
        ));
        let agent_output = self.execute_agent(&mut cycle, &selected_prompt).await?;

        // Phase 4: Monitor for PR creation
        cycle.phase = CyclePhase::MonitoringPR;
        cycle.log("👀 Monitoring for PR creation");
        if let Some(pr_number) = self.detect_pr_creation(&mut cycle, &agent_output).await? {
            cycle.created_pr_number = Some(pr_number);

            // Phase 5: Wait for CI to pass
            cycle.phase = CyclePhase::WaitingForCI;
            cycle.log(&format!("⏳ Waiting for PR #{} CI to pass", pr_number));
            self.wait_for_pr_ready(&mut cycle, pr_number).await?;
        } else {
            cycle.log("ℹ️  No PR created - cycle complete");
        }

        cycle.phase = CyclePhase::Completed;
        cycle.log(&format!(
            "✅ Orchestration cycle completed in {:.2}s",
            cycle.start_time.elapsed().as_secs_f64()
        ));

        info!("✅ Orchestration cycle {} completed successfully", cycle.id);
        Ok(cycle)
    }

    /// Check prerequisites before starting the cycle
    async fn check_prerequisites(&mut self, cycle: &mut OrchestrationCycle) -> Result<()> {
        let agent_name = self.agent.display_name();
        let process_identifier = self.agent.process_identifier();
        cycle.log(&format!("🔍 Checking for active {} sessions", agent_name));

        // Check if the agent is already running
        let active_sessions = detect_active_sessions(process_identifier).await?;
        if !active_sessions.is_empty() {
            let msg = format!(
                "Found {} active {} sessions - waiting",
                active_sessions.len(),
                agent_name
            );
            cycle.log(&msg);
            return Err(anyhow::anyhow!("{} is already active", agent_name));
        }

        cycle.log("🧹 Ensuring clean git state");

        // Ensure clean git state
        ensure_clean_working_directory(&self.config, process_identifier)
            .await
            .context("Working directory is not clean")?;

        cycle.log("✅ Prerequisites satisfied");
        Ok(())
    }

    /// Select a random prompt for execution, prioritizing check-pr-state if PRs have unaddressed comments
    async fn select_prompt(&mut self, cycle: &mut OrchestrationCycle) -> Result<Prompt> {
        cycle.log("🔍 Checking for PRs with unaddressed comments");

        // Check if any open PRs have unaddressed comments
        // match self.pr_monitor.check_all_prs_for_comments().await {
        //     Ok(prs_with_comments) => {
        //         if !prs_with_comments.is_empty() {
        //             cycle.log(&format!("📢 Found {} PRs with unaddressed feedback", prs_with_comments.len()));

        //             // Log details about the PRs with comments
        //             for pr_comments in &prs_with_comments {
        //                 cycle.log(&format!("  PR #{}: {} unresolved comments",
        //                                   pr_comments.pr_number,
        //                                   pr_comments.unresolved_comments.len()));
        //             }

        //             // Try to find the check-pr-state prompt
        //             if let Some(check_pr_prompt) = self.available_prompts.iter()
        //                 .find(|p| p.name == "check-pr-state.md") {
        //                 cycle.selected_prompt = check_pr_prompt.name.clone();
        //                 cycle.log(&format!("🎯 Prioritizing check-pr-state prompt due to unaddressed PR feedback"));
        //                 info!("🎯 Selected check-pr-state prompt due to unaddressed PR feedback");
        //                 return Ok(check_pr_prompt.clone());
        //             } else {
        //                 cycle.log("⚠️  check-pr-state.md prompt not found, falling back to random selection");
        //             }
        //         } else {
        //             cycle.log("✅ No PRs with unaddressed feedback found");
        //         }
        //     }
        //     Err(e) => {
        //         cycle.log(&format!("⚠️  Failed to check PRs for comments: {}", e));
        //         // Continue with normal prompt selection on error
        //     }
        // }

        // Default to random selection
        let selected = select_random_prompt(&self.available_prompts)?;

        cycle.selected_prompt = selected.name.clone();
        cycle.log(&format!(
            "🎯 Selected prompt: {} (weight: {}, risk: {:?})",
            selected.name, selected.weight, selected.metadata.risk_level
        ));

        info!("🎯 Selected prompt: {}", selected.name);
        Ok(selected.clone())
    }

    /// Execute the selected automation agent with the provided prompt
    async fn execute_agent(
        &mut self,
        cycle: &mut OrchestrationCycle,
        prompt: &Prompt,
    ) -> Result<AgentOutput> {
        let agent_name = self.agent.display_name();
        cycle.log(&format!("🤖 Starting {} session", agent_name));

        let session_id = self
            .agent
            .start_session(prompt)
            .await
            .with_context(|| format!("Failed to start {} session", agent_name))?;

        cycle.agent_session_id = Some(session_id.clone());
        cycle.log(&format!("📝 {} session ID: {}", agent_name, session_id));

        cycle.log(&format!("⏳ Waiting for {} to complete", agent_name));
        let output = self
            .agent
            .wait_for_completion(&session_id)
            .await
            .with_context(|| format!("{} session failed or timed out", agent_name))?;

        if output.success {
            cycle.log(&format!(
                "✅ {} completed successfully in {:.2}s",
                agent_name, output.execution_time_seconds
            ));

            if !output.files_created.is_empty() {
                cycle.log(&format!("📁 Created {} files", output.files_created.len()));
            }
            if !output.files_modified.is_empty() {
                cycle.log(&format!(
                    "✏️  Modified {} files",
                    output.files_modified.len()
                ));
            }
        } else {
            let error_msg = output.error.unwrap_or_else(|| "Unknown error".to_string());
            cycle.log(&format!("❌ {} failed: {}", agent_name, error_msg));
            return Err(anyhow::anyhow!(
                "{} execution failed: {}",
                agent_name,
                error_msg
            ));
        }

        self.agent.cleanup_completed_sessions();

        Ok(output)
    }

    /// Detect if a PR was created from the agent execution
    async fn detect_pr_creation(
        &mut self,
        cycle: &mut OrchestrationCycle,
        output: &AgentOutput,
    ) -> Result<Option<u32>> {
        cycle.log("🔍 Checking for PR creation");

        // Check if git changes indicate PR creation
        if let Some(git_changes) = &output.git_changes {
            if let Some(pr_number) = git_changes.pr_created {
                cycle.log(&format!("🎉 Detected PR creation: #{}", pr_number));
                return Ok(Some(pr_number));
            }

            if let Some(branch) = &git_changes.branch_created {
                cycle.log(&format!("🌿 New branch created: {}", branch));
                // TODO: Could implement additional PR detection logic here
                // For now, we assume if a branch was created, a PR might be coming
            }
        }

        cycle.log("ℹ️  No PR detected from agent output");
        Ok(None)
    }

    /// Wait for PR to become ready (CI passes)
    async fn wait_for_pr_ready(
        &mut self,
        cycle: &mut OrchestrationCycle,
        pr_number: u32,
    ) -> Result<()> {
        cycle.log(&format!("⏳ Starting to monitor PR #{}", pr_number));

        let timeout = Duration::from_secs(
            self.config.parse_ci_wait_time().unwrap_or(1800), // Default 30 minutes
        );

        match self.pr_monitor.wait_for_pr_ready(pr_number, timeout).await {
            Ok(status) => match status.merge_status {
                _ if status.merge_status.required_checks_passing => {
                    cycle.log(&format!("✅ PR #{} is ready for merge", pr_number));
                }
                _ => {
                    cycle.log(&format!(
                        "⚠️  PR #{} completed but not ready for merge",
                        pr_number
                    ));
                }
            },
            Err(e) => {
                cycle.log(&format!("⚠️  PR #{} monitoring ended: {}", pr_number, e));

                // Get failure analysis if available
                if let Ok(analysis) = self.pr_monitor.analyze_pr_failures(pr_number).await {
                    if !analysis.error_logs.is_empty() {
                        cycle.log(&format!(
                            "📋 Failure analysis available with {} log entries",
                            analysis.error_logs.len()
                        ));
                    }
                }
            }
        }

        Ok(())
    }
}

impl OrchestrationCycle {
    /// Add a log entry to the cycle
    fn log(&mut self, message: &str) {
        let timestamp = format!("[{:.2}s]", self.start_time.elapsed().as_secs_f64());
        let log_entry = format!("{} {}", timestamp, message);
        self.execution_log.push(log_entry.clone());
        info!("{}", log_entry);
    }
}

/// Generate a unique cycle ID
fn generate_cycle_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("cycle-{}-{:04x}", timestamp, rand::random::<u16>())
}

impl Default for OrchestrationState {
    fn default() -> Self {
        Self {
            last_run: None,
            current_cycle: None,
            cycles_completed: 0,
            is_running: false,
            should_stop: false,
        }
    }
}
