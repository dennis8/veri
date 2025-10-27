use veri_core::config::Config;
use veri_core::telemetry::{ErrorCategory, RunEvent, TelemetryClient, TelemetryConfig};

pub struct TelemetryService {
    client: TelemetryClient,
}

impl TelemetryService {
    pub fn new(config: &Config) -> Self {
        let telemetry_config = TelemetryConfig {
            enabled: config.is_telemetry_enabled(),
            endpoint: config.telemetry().endpoint,
            collection_interval: config.telemetry().collection_interval.unwrap_or(300),
            collect_performance: true,
            collect_usage: true,
            max_queue_size: 1000,
        };

        Self {
            client: TelemetryClient::new(telemetry_config),
        }
    }

    pub fn print_status(&self, colorize: bool) {
        self.client.print_status(colorize);
    }

    pub fn record_run(&mut self, event: RunEvent) {
        self.client.record_run(event);
    }

    pub fn record_error(&mut self, category: ErrorCategory) {
        self.client.record_error(category);
    }
}
