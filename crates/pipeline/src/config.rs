//! Pipeline configuration data structures.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents the entire system configuration for the pipeline.
/// This struct is versioned and designed for forward-compatibility.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemConfig {
    pub version: String,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    pub stages: Vec<StageConfig>,
}

/// Represents the configuration for a single stage in the pipeline.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub stage_type: String,
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub inputs: Vec<String>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub channel_capacity: Option<usize>,
}
    
    #[cfg(test)]
    mod tests {
        use super::*;
        use serde_json::json;
    
        #[test]
        fn test_deserialize_system_config() {
            let json_str = r#"
            {
                "version": "1.0",
                "metadata": {
                    "name": "Test Pipeline"
                },
                "stages": [
                    {
                        "name": "stage1",
                        "type": "filter",
                        "params": {
                            "lowpass": 50.0
                        },
                        "inputs": ["source1"]
                    }
                ]
            }
            "#;
    
            let config: SystemConfig = serde_json::from_str(json_str).unwrap();
    
            assert_eq!(config.version, "1.0");
            assert_eq!(config.metadata["name"], json!("Test Pipeline"));
            assert_eq!(config.stages.len(), 1);
    
            let stage = &config.stages[0];
            assert_eq!(stage.name, "stage1");
            assert_eq!(stage.stage_type, "filter");
            assert_eq!(stage.params["lowpass"], json!(50.0));
            assert_eq!(stage.inputs, vec!["source1"]);
        }
    }
