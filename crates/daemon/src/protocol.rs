use serde::{Deserialize, Serialize};
use sensors::types::AdcConfig;

/// A message from a client proposing a new configuration.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConfigProposal {
    /// The proposed new ADC configuration.
    pub config: AdcConfig,
    /// An optional transient identifier for the request.
    #[serde(default)]
    pub request_id: Option<String>,
}

/// A message from the server, either broadcasting an applied configuration
/// or rejecting a proposal.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// Broadcasts the latest, successfully applied configuration.
    Applied {
        config: AdcConfig,
        /// An optional monotonically increasing revision number.
        #[serde(default)]
        revision: Option<u64>,
    },
    /// Informs a single client that its proposal was rejected.
    Rejected {
        reason: String,
        /// The `request_id` from the original proposal, if any.
        #[serde(default)]
        request_id: Option<String>,
    },
}