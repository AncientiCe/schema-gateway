use prometheus::{
    Counter, CounterVec, Encoder, HistogramOpts, HistogramVec, Opts, Registry, TextEncoder,
};

/// Metrics collection for the schema gateway
pub struct Metrics {
    pub http_requests_total: CounterVec,
    pub http_request_duration_seconds: HistogramVec,
    pub validation_attempts_total: CounterVec,
    pub validation_success_total: CounterVec,
    pub validation_failures_total: CounterVec,
    pub upstream_requests_total: CounterVec,
    pub upstream_request_duration_seconds: HistogramVec,
    pub upstream_errors_total: CounterVec,
    pub schema_cache_hits_total: Counter,
    pub schema_cache_misses_total: Counter,
    pub routes_not_found_total: CounterVec,
    registry: Registry,
}

impl Metrics {
    /// Create a new Metrics instance with all metrics registered
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        // HTTP request metrics
        let http_requests_total = CounterVec::new(
            Opts::new("http_requests_total", "Total number of HTTP requests"),
            &["method", "route", "status"],
        )?;
        registry.register(Box::new(http_requests_total.clone()))?;

        let http_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "http_request_duration_seconds",
                "HTTP request duration in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
            &["method", "route"],
        )?;
        registry.register(Box::new(http_request_duration_seconds.clone()))?;

        // Validation metrics
        let validation_attempts_total = CounterVec::new(
            Opts::new(
                "validation_attempts_total",
                "Total number of validation attempts",
            ),
            &["validation_type"],
        )?;
        registry.register(Box::new(validation_attempts_total.clone()))?;

        let validation_success_total = CounterVec::new(
            Opts::new(
                "validation_success_total",
                "Total number of successful validations",
            ),
            &["validation_type"],
        )?;
        registry.register(Box::new(validation_success_total.clone()))?;

        let validation_failures_total = CounterVec::new(
            Opts::new(
                "validation_failures_total",
                "Total number of validation failures",
            ),
            &["validation_type", "error_type"],
        )?;
        registry.register(Box::new(validation_failures_total.clone()))?;

        // Upstream metrics
        let upstream_requests_total = CounterVec::new(
            Opts::new(
                "upstream_requests_total",
                "Total number of upstream requests",
            ),
            &["status"],
        )?;
        registry.register(Box::new(upstream_requests_total.clone()))?;

        let upstream_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "upstream_request_duration_seconds",
                "Upstream request duration in seconds",
            )
            .buckets(vec![
                0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ]),
            &[],
        )?;
        registry.register(Box::new(upstream_request_duration_seconds.clone()))?;

        let upstream_errors_total = CounterVec::new(
            Opts::new("upstream_errors_total", "Total number of upstream errors"),
            &["error_type"],
        )?;
        registry.register(Box::new(upstream_errors_total.clone()))?;

        // Cache metrics
        let schema_cache_hits_total = Counter::with_opts(Opts::new(
            "schema_cache_hits_total",
            "Total number of schema cache hits",
        ))?;
        registry.register(Box::new(schema_cache_hits_total.clone()))?;

        let schema_cache_misses_total = Counter::with_opts(Opts::new(
            "schema_cache_misses_total",
            "Total number of schema cache misses",
        ))?;
        registry.register(Box::new(schema_cache_misses_total.clone()))?;

        // Route metrics
        let routes_not_found_total = CounterVec::new(
            Opts::new("routes_not_found_total", "Total number of 404 responses"),
            &["method"],
        )?;
        registry.register(Box::new(routes_not_found_total.clone()))?;

        Ok(Metrics {
            http_requests_total,
            http_request_duration_seconds,
            validation_attempts_total,
            validation_success_total,
            validation_failures_total,
            upstream_requests_total,
            upstream_request_duration_seconds,
            upstream_errors_total,
            schema_cache_hits_total,
            schema_cache_misses_total,
            routes_not_found_total,
            registry,
        })
    }

    /// Gather all metrics and encode them in Prometheus format
    pub fn gather(&self) -> Result<String, prometheus::Error> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8_lossy(&buffer).to_string())
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new().expect("Failed to create metrics")
    }
}
