use cgmath::{Matrix4, Quaternion, Vector2, Vector3};
use engine::{
    assets::asset_cache::AssetCache,
    audio::AudioContext,
    scene::SceneObject,
};
use shipyard::EntityId;
use std::path::{Path, PathBuf};

use crate::{
    gui::{GuiComponent, ButtonHoverBehavior},
    input_context::InputContext,
    scripts::{Effect, GlobalEffect},
    time::Time,
    GameOptions,
};

use super::{
    mission_trait::{Mission, MissionTransition, MissionType},
    GlobalContext,
};

/// Events that can be triggered by asset validation UI components
#[derive(Debug, Clone)]
pub enum AssetValidationEvent {
    /// User clicked continue button (transition to main menu)
    Continue,
    /// User requested validation retry
    Retry,
    /// User requested help/instructions
    ShowHelp,
}

/// Asset validation mission that checks for required game files and guides user setup.
/// This mission runs at startup to ensure all necessary assets are available before
/// allowing the user to proceed to the main menu or gameplay.
pub struct AssetValidationMission {
    /// Current state of the validation process
    validation_state: AssetValidationState,
    /// UI elements to display validation progress and instructions
    ui_elements: Vec<GuiComponent<AssetValidationEvent>>,
    /// Background scene objects for visual appeal
    background_scene: Vec<SceneObject>,
    /// Configuration for which assets to check
    asset_config: AssetValidationConfig,
    /// Camera position for rendering
    camera_position: Vector3<f32>,
    /// Camera rotation for rendering
    camera_rotation: Quaternion<f32>,
}

/// Current state of the asset validation process
#[derive(Debug, Clone)]
enum AssetValidationState {
    /// Currently checking for required assets
    Checking { progress: f32 },
    /// Found missing assets that prevent gameplay
    MissingAssets { missing_files: Vec<MissingAsset> },
    /// All required assets are present and valid
    Valid,
    /// Validation error occurred (file system issues, etc.)
    Error { message: String },
    /// User requested transition to main menu (only valid after successful validation)
    TransitionRequested,
}

/// Information about a missing asset file
#[derive(Debug, Clone)]
struct MissingAsset {
    /// Path where the file should be located
    path: PathBuf,
    /// Description of what this file is used for
    description: String,
    /// Whether this asset is required or optional
    required: bool,
}

/// Configuration for asset validation
#[derive(Debug, Clone)]
struct AssetValidationConfig {
    /// Base directory for game assets
    asset_base_path: PathBuf,
    /// List of required assets to check
    required_assets: Vec<AssetRequirement>,
    /// List of optional assets to check
    optional_assets: Vec<AssetRequirement>,
}

/// Specification for a required asset
#[derive(Debug, Clone)]
struct AssetRequirement {
    /// Relative path from asset base
    path: PathBuf,
    /// Description for user-facing messages
    description: String,
    /// Optional size check (for basic validation)
    min_size: Option<u64>,
}


impl AssetValidationMission {
    /// Create a new asset validation mission with default configuration
    pub fn new() -> Self {
        let asset_config = AssetValidationConfig::default();

        Self {
            validation_state: AssetValidationState::Checking { progress: 0.0 },
            ui_elements: Vec::new(),
            background_scene: Vec::new(),
            asset_config,
            camera_position: Vector3::new(0.0, 0.0, 2.0),
            camera_rotation: Quaternion::new(1.0, 0.0, 0.0, 0.0),
        }
    }

    /// Create asset validation mission with custom asset path
    pub fn with_asset_path<P: AsRef<Path>>(asset_path: P) -> Self {
        let mut mission = Self::new();
        mission.asset_config.asset_base_path = asset_path.as_ref().to_path_buf();
        mission
    }

    /// Perform the actual asset validation check
    fn validate_assets(&mut self) {
        let mut missing_assets = Vec::new();
        let total_assets = self.asset_config.required_assets.len() + self.asset_config.optional_assets.len();
        let mut checked_assets = 0;

        // Check required assets
        for requirement in &self.asset_config.required_assets {
            let full_path = self.asset_config.asset_base_path.join(&requirement.path);

            if !self.check_asset_exists(&full_path, &requirement) {
                missing_assets.push(MissingAsset {
                    path: requirement.path.clone(),
                    description: requirement.description.clone(),
                    required: true,
                });
            }

            checked_assets += 1;
            self.validation_state = AssetValidationState::Checking {
                progress: checked_assets as f32 / total_assets as f32,
            };
        }

        // Check optional assets
        for requirement in &self.asset_config.optional_assets {
            let full_path = self.asset_config.asset_base_path.join(&requirement.path);

            if !self.check_asset_exists(&full_path, &requirement) {
                missing_assets.push(MissingAsset {
                    path: requirement.path.clone(),
                    description: requirement.description.clone(),
                    required: false,
                });
            }

            checked_assets += 1;
            self.validation_state = AssetValidationState::Checking {
                progress: checked_assets as f32 / total_assets as f32,
            };
        }

        // Determine final state
        let has_required_missing = missing_assets.iter().any(|asset| asset.required);

        if has_required_missing {
            self.validation_state = AssetValidationState::MissingAssets { missing_files: missing_assets };
        } else {
            self.validation_state = AssetValidationState::Valid;
        }
    }

    /// Check if a specific asset exists and meets requirements
    fn check_asset_exists(&self, path: &Path, requirement: &AssetRequirement) -> bool {
        match std::fs::metadata(path) {
            Ok(metadata) => {
                // Check file size if specified
                if let Some(min_size) = requirement.min_size {
                    if metadata.len() < min_size {
                        return false;
                    }
                }
                true
            }
            Err(_) => false,
        }
    }

    /// Update UI elements based on current validation state
    fn update_ui_elements(&mut self) {
        self.ui_elements.clear();

        // Title text
        self.ui_elements.push(GuiComponent::Text {
            position: Vector2::new(0.0, 0.8),
            size: Vector2::new(0.8, 0.1),
            font: "default_font".to_string(),
            text: "System Shock 2 VR - Asset Validation".to_string(),
            alpha: 1.0,
        });

        match &self.validation_state {
            AssetValidationState::Checking { progress } => {
                self.ui_elements.push(GuiComponent::Text {
                    position: Vector2::new(0.0, 0.5),
                    size: Vector2::new(0.6, 0.08),
                    font: "default_font".to_string(),
                    text: "Checking game assets...".to_string(),
                    alpha: 1.0,
                });

                self.ui_elements.push(GuiComponent::Text {
                    position: Vector2::new(0.0, 0.3),
                    size: Vector2::new(0.4, 0.06),
                    font: "default_font".to_string(),
                    text: format!("Progress: {:.0}%", progress * 100.0),
                    alpha: 1.0,
                });
            }

            AssetValidationState::MissingAssets { missing_files } => {
                self.ui_elements.push(GuiComponent::Text {
                    position: Vector2::new(0.0, 0.6),
                    size: Vector2::new(0.6, 0.08),
                    font: "default_font".to_string(),
                    text: "Missing Required Assets".to_string(),
                    alpha: 1.0,
                });

                let required_missing: Vec<_> = missing_files.iter().filter(|asset| asset.required).collect();
                if !required_missing.is_empty() {
                    self.ui_elements.push(GuiComponent::Text {
                        position: Vector2::new(0.0, 0.4),
                        size: Vector2::new(0.8, 0.06),
                        font: "default_font".to_string(),
                        text: "Please ensure the following files are in your game directory:".to_string(),
                        alpha: 1.0,
                    });

                    for (i, asset) in required_missing.iter().enumerate() {
                        self.ui_elements.push(GuiComponent::Text {
                            position: Vector2::new(0.0, 0.2 - (i as f32 * 0.08)),
                            size: Vector2::new(0.9, 0.05),
                            font: "default_font".to_string(),
                            text: format!("• {} ({})", asset.path.display(), asset.description),
                            alpha: 1.0,
                        });
                    }
                }
            }

            AssetValidationState::Valid => {
                self.ui_elements.push(GuiComponent::Text {
                    position: Vector2::new(0.0, 0.5),
                    size: Vector2::new(0.7, 0.08),
                    font: "default_font".to_string(),
                    text: "All assets validated successfully!".to_string(),
                    alpha: 1.0,
                });

                self.ui_elements.push(GuiComponent::Button {
                    position: Vector2::new(0.0, 0.2),
                    size: Vector2::new(0.4, 0.1),
                    texture: "button_texture".to_string(),
                    on_click: Some(AssetValidationEvent::Continue),
                    on_grab: None,
                    hover: ButtonHoverBehavior::None,
                    alpha: 1.0,
                });
            }

            AssetValidationState::Error { message } => {
                self.ui_elements.push(GuiComponent::Text {
                    position: Vector2::new(0.0, 0.6),
                    size: Vector2::new(0.5, 0.08),
                    font: "default_font".to_string(),
                    text: "Validation Error".to_string(),
                    alpha: 1.0,
                });

                self.ui_elements.push(GuiComponent::Text {
                    position: Vector2::new(0.0, 0.4),
                    size: Vector2::new(0.8, 0.06),
                    font: "default_font".to_string(),
                    text: message.clone(),
                    alpha: 1.0,
                });
            }

            AssetValidationState::TransitionRequested => {
                self.ui_elements.push(GuiComponent::Text {
                    position: Vector2::new(0.0, 0.5),
                    size: Vector2::new(0.6, 0.08),
                    font: "default_font".to_string(),
                    text: "Transitioning to main menu...".to_string(),
                    alpha: 1.0,
                });
            }
        }
    }

    /// Handle user input for continuing to main menu
    fn handle_input(&mut self, input_context: &InputContext) {
        if let AssetValidationState::Valid = self.validation_state {
            // Check for trigger press on either hand
            if input_context.left_hand.trigger_value > 0.8 || input_context.right_hand.trigger_value > 0.8 {
                self.validation_state = AssetValidationState::TransitionRequested;
            }
        }
    }
}

impl Default for AssetValidationConfig {
    fn default() -> Self {
        Self {
            asset_base_path: PathBuf::from("Data"),
            required_assets: vec![
                AssetRequirement {
                    path: PathBuf::from("shock2.gam"),
                    description: "Core game database".to_string(),
                    min_size: Some(1024), // At least 1KB
                },
                AssetRequirement {
                    path: PathBuf::from("medsci1.mis"),
                    description: "Medical/Science level 1".to_string(),
                    min_size: Some(1024),
                },
                AssetRequirement {
                    path: PathBuf::from("iface"),
                    description: "Interface resources directory".to_string(),
                    min_size: None,
                },
            ],
            optional_assets: vec![
                AssetRequirement {
                    path: PathBuf::from("cutscenes"),
                    description: "Cutscene video files".to_string(),
                    min_size: None,
                },
            ],
        }
    }
}

impl Mission for AssetValidationMission {
    fn update(
        &mut self,
        _time: &Time,
        _asset_cache: &mut AssetCache,
        input_context: &InputContext,
    ) -> Vec<Effect> {
        // Handle asset validation state machine
        match &self.validation_state {
            AssetValidationState::Checking { .. } => {
                // Continue validation process
                self.validate_assets();
            }
            AssetValidationState::Valid => {
                // Handle user input for continuing
                self.handle_input(input_context);
            }
            _ => {
                // No updates needed for other states
            }
        }

        // Update UI elements
        self.update_ui_elements();

        // No effects generated by asset validation
        Vec::new()
    }

    fn render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _options: &GameOptions,
    ) -> (Vec<SceneObject>, Vector3<f32>, Quaternion<f32>) {
        // Convert UI elements to scene objects
        let scene_objects = self.background_scene.clone();

        // UI elements are now using the existing GuiComponent system
        // This will be implemented when integrating with the actual GUI rendering system

        (scene_objects, self.camera_position, self.camera_rotation)
    }

    fn handle_effects(
        &mut self,
        _effects: Vec<Effect>,
        _global_context: &GlobalContext,
        _game_options: &GameOptions,
        _asset_cache: &mut AssetCache,
        _audio_context: &mut AudioContext<EntityId, String>,
    ) -> Vec<GlobalEffect> {
        // Asset validation doesn't handle effects
        Vec::new()
    }

    fn render_per_eye(
        &mut self,
        _asset_cache: &mut AssetCache,
        _view: Matrix4<f32>,
        _projection: Matrix4<f32>,
        _screen_size: Vector2<f32>,
    ) -> Vec<SceneObject> {
        // No per-eye specific rendering for asset validation
        Vec::new()
    }

    fn finish_render(
        &mut self,
        _asset_cache: &mut AssetCache,
        _view: Matrix4<f32>,
        _projection: Matrix4<f32>,
        _screen_size: Vector2<f32>,
    ) {
        // No additional render finalization needed
    }

    fn mission_type(&self) -> MissionType {
        MissionType::AssetValidation
    }

    fn should_transition(&self) -> Option<MissionTransition> {
        match &self.validation_state {
            AssetValidationState::TransitionRequested => Some(MissionTransition::ToMainMenu),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asset_validation_mission_creation() {
        let mission = AssetValidationMission::new();
        assert_eq!(mission.mission_type(), MissionType::AssetValidation);
        assert!(mission.should_transition().is_none());
    }

    #[test]
    fn test_asset_validation_with_missing_files() {
        let mut mission = AssetValidationMission::with_asset_path("/nonexistent/path");

        // Validate assets (should find missing files)
        mission.validate_assets();

        match &mission.validation_state {
            AssetValidationState::MissingAssets { missing_files } => {
                assert!(!missing_files.is_empty());
                // Should have missing shock2.gam
                assert!(missing_files.iter().any(|asset| asset.path.to_string_lossy().contains("shock2.gam")));
            }
            _ => panic!("Expected MissingAssets state"),
        }
    }

    #[test]
    fn test_transition_after_validation() {
        let mut mission = AssetValidationMission::new();
        mission.validation_state = AssetValidationState::Valid;

        // Simulate trigger press
        let mut input_context = InputContext::default();
        input_context.left_hand.trigger_value = 1.0;

        mission.handle_input(&input_context);

        assert!(matches!(mission.validation_state, AssetValidationState::TransitionRequested));
        assert!(matches!(mission.should_transition(), Some(MissionTransition::ToMainMenu)));
    }

    #[test]
    fn test_ui_elements_update() {
        let mut mission = AssetValidationMission::new();

        // Test initial state
        mission.update_ui_elements();
        assert!(!mission.ui_elements.is_empty());

        // Should have title element (text component with title text)
        assert!(mission.ui_elements.iter().any(|element| {
            matches!(element, GuiComponent::Text { text, .. } if text.contains("System Shock 2 VR"))
        }));
    }

    #[test]
    fn test_camera_position() {
        let mission = AssetValidationMission::new();
        // Test basic properties
        assert_eq!(mission.mission_type(), MissionType::AssetValidation);
        assert_eq!(mission.camera_position, Vector3::new(0.0, 0.0, 2.0));
        assert_eq!(mission.camera_rotation, Quaternion::new(1.0, 0.0, 0.0, 0.0));
    }
}