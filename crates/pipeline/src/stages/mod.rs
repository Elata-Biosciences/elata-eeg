//! Built-in pipeline stages

use crate::registry::StageRegistry;

pub mod acquire;
pub mod csv_sink;
pub mod eeg_source;
pub mod filter;
pub mod test_stage;
pub mod to_voltage;
pub mod websocket_sink;
pub mod gui_filter;

pub use acquire::AcquireFactory;
pub use csv_sink::CsvSinkFactory;
pub use eeg_source::EegSourceFactory;
pub use filter::FilterFactory;
pub use test_stage::StatefulTestStage;
pub use to_voltage::ToVoltageFactory;
pub use websocket_sink::WebsocketSinkFactory;
pub use gui_filter::TripleIirFactory;

/// Registers all built-in stages with the provided registry.
pub fn register_builtin_stages(registry: &mut StageRegistry) {
    registry.register("acquire", Box::new(AcquireFactory::default()));
    registry.register("eeg_source", Box::new(EegSourceFactory::default()));
    registry.register("to_voltage", Box::new(ToVoltageFactory::default()));
    registry.register("filter", Box::new(FilterFactory::default()));
    registry.register("csv_sink", Box::new(CsvSinkFactory::default()));
    registry.register(
        "websocket_sink",
        Box::new(WebsocketSinkFactory::default()),
    );
    registry.register("gui_filter", Box::new(TripleIirFactory::default()));
}