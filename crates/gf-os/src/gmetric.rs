//! Port of `os/gmetric`.
//!
//! GoFrame's metric package mirrors the OpenTelemetry metrics API: a global
//! [`Provider`] creates a [`Meter`], the meter creates instruments
//! (counter, up-down counter, histogram, observable variants), and each
//! instrument performs measurements through a performer supplied by the
//! provider. Without a provider every instrument is a no-op that still exposes
//! its [`MetricInfo`].
//!
//! Divergences, recorded in ADR-056:
//!
//! - no `context.Context`: performers take only their values/options;
//! - Go's `Option` measure-option is named [`ObservationOption`] (the name
//!   `Option` is the standard enum);
//! - `Attribute` is a plain struct instead of an interface;
//! - `MetricInfo`/`InstrumentInfo` are structs with accessors;
//! - `GetRegisteredCallbacks` still drains the callback list, like Go.

use indexmap::IndexMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_text::gregex;

/// The regular expression GoFrame validates metric names with.
pub const METRIC_NAME_PATTERN: &str = r"[\w\.\-\/]";

/// The metric type, like Go's `MetricType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricType {
    /// A monotonically increasing counter.
    Counter,
    /// A counter that can go up and down.
    UpDownCounter,
    /// A histogram of observations.
    Histogram,
    /// An asynchronous counter.
    ObservableCounter,
    /// An asynchronous up-down counter.
    ObservableUpDownCounter,
    /// An asynchronous gauge.
    ObservableGauge,
}

impl std::fmt::Display for MetricType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            MetricType::Counter => "Counter",
            MetricType::UpDownCounter => "UpDownCounter",
            MetricType::Histogram => "Histogram",
            MetricType::ObservableCounter => "ObservableCounter",
            MetricType::ObservableUpDownCounter => "ObservableUpDownCounter",
            MetricType::ObservableGauge => "ObservableGauge",
        })
    }
}

/// One metric attribute, like Go's `Attribute`.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribute {
    key: String,
    value: Value,
}

impl Attribute {
    /// Creates an attribute.
    pub fn new(key: &str, value: Value) -> Attribute {
        Attribute {
            key: key.to_owned(),
            value,
        }
    }

    /// The attribute key.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The attribute value.
    pub fn value(&self) -> &Value {
        &self.value
    }
}

/// Creates an attribute, like `NewAttribute`.
pub fn new_attribute(key: &str, value: Value) -> Attribute {
    Attribute::new(key, value)
}

/// A list of attributes, like Go's `Attributes`.
pub type Attributes = Vec<Attribute>;

/// The common attributes of every instrument, like `CommonAttributes`.
pub fn common_attributes() -> Attributes {
    vec![
        Attribute::new(
            "os.host.name",
            Value::String(
                std::env::var("COMPUTERNAME")
                    .or_else(|_| std::env::var("HOSTNAME"))
                    .unwrap_or_default(),
            ),
        ),
        Attribute::new(
            "process.path",
            Value::String(
                std::env::current_exe()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
        ),
    ]
}

/// Options for one measurement, replacing Go's `Option`.
#[derive(Clone, Debug, Default)]
pub struct ObservationOption {
    /// Dynamic attributes of this measurement.
    pub attributes: Attributes,
}

/// Basic options for creating a metric, like Go's `MetricOption`.
#[derive(Clone, Default)]
pub struct MetricOption {
    /// Help description.
    pub help: String,
    /// Unit name.
    pub unit: String,
    /// Constant attributes.
    pub attributes: Attributes,
    /// Histogram buckets.
    pub buckets: Vec<f64>,
    /// Callback for observable metrics.
    pub callback: Option<MetricCallback>,
}

/// A callback that observes a bound metric, like `MetricCallback`.
pub type MetricCallback = Arc<dyn Fn(&dyn MetricObserver) -> Result<(), Error> + Send + Sync>;

/// A callback registered on a meter, like `Callback`.
pub type Callback = Arc<dyn Fn(&dyn Observer) -> Result<(), Error> + Send + Sync>;

/// Observes a bound metric, like `MetricObserver`.
pub trait MetricObserver: Send + Sync {
    /// Records `value`.
    fn observe(&self, value: f64, option: Option<ObservationOption>);
}

/// Observes any initialized metric, like `Observer`.
pub trait Observer: Send + Sync {
    /// Records `value` for `metric`.
    fn observe(&self, metric: &dyn ObservableMetric, value: f64, option: Option<ObservationOption>);
}

/// The instrument information, like Go's `InstrumentInfo`.
#[derive(Clone, Debug, Default)]
pub struct InstrumentInfo {
    /// Instrument name.
    pub name: String,
    /// Instrument version.
    pub version: String,
}

/// The information of one metric, like Go's `MetricInfo`.
#[derive(Clone, Debug)]
pub struct MetricInfo {
    name: String,
    info_type: MetricType,
    help: String,
    unit: String,
    attributes: Attributes,
    instrument: InstrumentInfo,
}

impl MetricInfo {
    /// Creates the info for one metric.
    pub fn new(
        metric_type: MetricType,
        name: &str,
        option: &MetricOption,
        instrument: InstrumentInfo,
    ) -> MetricInfo {
        MetricInfo {
            name: name.to_owned(),
            info_type: metric_type,
            help: option.help.clone(),
            unit: option.unit.clone(),
            attributes: option.attributes.clone(),
            instrument,
        }
    }

    /// The unique key of the metric, like Go's `Key`.
    pub fn key(&self) -> String {
        match (
            self.instrument.name.is_empty(),
            self.instrument.version.is_empty(),
        ) {
            (false, false) => format!(
                "{}@{}:{}",
                self.instrument.name, self.instrument.version, self.name
            ),
            (false, true) => format!("{}:{}", self.instrument.name, self.name),
            _ => self.name.clone(),
        }
    }

    /// The metric name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The help description.
    pub fn help(&self) -> &str {
        &self.help
    }

    /// The unit name.
    pub fn unit(&self) -> &str {
        &self.unit
    }

    /// The metric type.
    pub fn metric_type(&self) -> MetricType {
        self.info_type
    }

    /// The constant attributes.
    pub fn attributes(&self) -> &Attributes {
        &self.attributes
    }

    /// The instrument information.
    pub fn instrument(&self) -> &InstrumentInfo {
        &self.instrument
    }
}

/// A metric with its information, like Go's `Metric`.
pub trait Metric: Send + Sync {
    /// Returns the metric information.
    fn info(&self) -> &MetricInfo;
}

/// The counter operations, like `CounterPerformer`.
pub trait CounterPerformer: Send + Sync {
    /// Increments the counter by one.
    fn inc(&self, option: Option<ObservationOption>);
    /// Adds `increment` to the counter.
    fn add(&self, increment: f64, option: Option<ObservationOption>);
}

/// The up-down counter operations, like `UpDownCounterPerformer`.
pub trait UpDownCounterPerformer: Send + Sync {
    /// Increments by one.
    fn inc(&self, option: Option<ObservationOption>);
    /// Decrements by one.
    fn dec(&self, option: Option<ObservationOption>);
    /// Adds `increment`.
    fn add(&self, increment: f64, option: Option<ObservationOption>);
}

/// The histogram operations, like `HistogramPerformer`.
pub trait HistogramPerformer: Send + Sync {
    /// Records one observation.
    fn record(&self, increment: f64, option: Option<ObservationOption>);
}

/// An asynchronous metric, like Go's `ObservableMetric`.
pub trait ObservableMetric: Metric {}

/// The no-op counter performer.
#[derive(Default)]
pub struct NoopCounterPerformer;

impl CounterPerformer for NoopCounterPerformer {
    fn inc(&self, _option: Option<ObservationOption>) {}

    fn add(&self, _increment: f64, _option: Option<ObservationOption>) {}
}

/// The no-op up-down counter performer.
#[derive(Default)]
pub struct NoopUpDownCounterPerformer;

impl UpDownCounterPerformer for NoopUpDownCounterPerformer {
    fn inc(&self, _option: Option<ObservationOption>) {}

    fn dec(&self, _option: Option<ObservationOption>) {}

    fn add(&self, _increment: f64, _option: Option<ObservationOption>) {}
}

/// The no-op histogram performer.
#[derive(Default)]
pub struct NoopHistogramPerformer;

impl HistogramPerformer for NoopHistogramPerformer {
    fn record(&self, _increment: f64, _option: Option<ObservationOption>) {}
}

/// A counter metric, like Go's `Counter`.
pub struct Counter {
    info: MetricInfo,
    performer: Arc<dyn CounterPerformer>,
}

impl Counter {
    /// Increments by one, like `Inc`.
    pub fn inc(&self, option: Option<ObservationOption>) {
        self.performer.inc(option);
    }

    /// Adds `increment`, like `Add`.
    pub fn add(&self, increment: f64, option: Option<ObservationOption>) {
        self.performer.add(increment, option);
    }
}

impl Metric for Counter {
    fn info(&self) -> &MetricInfo {
        &self.info
    }
}

/// An up-down counter, like Go's `UpDownCounter`.
pub struct UpDownCounter {
    info: MetricInfo,
    performer: Arc<dyn UpDownCounterPerformer>,
}

impl UpDownCounter {
    /// Increments by one.
    pub fn inc(&self, option: Option<ObservationOption>) {
        self.performer.inc(option);
    }

    /// Decrements by one.
    pub fn dec(&self, option: Option<ObservationOption>) {
        self.performer.dec(option);
    }

    /// Adds `increment`.
    pub fn add(&self, increment: f64, option: Option<ObservationOption>) {
        self.performer.add(increment, option);
    }
}

impl Metric for UpDownCounter {
    fn info(&self) -> &MetricInfo {
        &self.info
    }
}

/// A histogram, like Go's `Histogram`.
pub struct Histogram {
    info: MetricInfo,
    performer: Arc<dyn HistogramPerformer>,
    buckets: Vec<f64>,
}

impl Histogram {
    /// Records one observation, like `Record`.
    pub fn record(&self, increment: f64, option: Option<ObservationOption>) {
        self.performer.record(increment, option);
    }

    /// The histogram buckets.
    pub fn buckets(&self) -> &[f64] {
        &self.buckets
    }
}

impl Metric for Histogram {
    fn info(&self) -> &MetricInfo {
        &self.info
    }
}

/// An asynchronous counter, like Go's `ObservableCounter`.
pub struct ObservableCounter {
    info: MetricInfo,
}

impl Metric for ObservableCounter {
    fn info(&self) -> &MetricInfo {
        &self.info
    }
}

impl ObservableMetric for ObservableCounter {}

/// An asynchronous up-down counter.
pub struct ObservableUpDownCounter {
    info: MetricInfo,
}

impl Metric for ObservableUpDownCounter {
    fn info(&self) -> &MetricInfo {
        &self.info
    }
}

impl ObservableMetric for ObservableUpDownCounter {}

/// An asynchronous gauge.
pub struct ObservableGauge {
    info: MetricInfo,
}

impl Metric for ObservableGauge {
    fn info(&self) -> &MetricInfo {
        &self.info
    }
}

impl ObservableMetric for ObservableGauge {}

/// The options for creating a meter, like Go's `MeterOption`.
#[derive(Clone, Debug, Default)]
pub struct MeterOption {
    /// The instrumentation name.
    pub instrument: String,
    /// The instrumentation version.
    pub instrument_version: String,
    /// Constant attributes for every metric of the meter.
    pub attributes: Attributes,
}

impl MeterOption {
    fn instrument_info(&self) -> InstrumentInfo {
        InstrumentInfo {
            name: self.instrument.clone(),
            version: self.instrument_version.clone(),
        }
    }
}

/// Creates metric performers, like Go's `MeterPerformer`.
pub trait MeterPerformer: Send + Sync {
    /// Creates a counter performer.
    fn counter_performer(
        &self,
        name: &str,
        option: MetricOption,
    ) -> Result<Arc<dyn CounterPerformer>, Error>;
    /// Creates an up-down counter performer.
    fn up_down_counter_performer(
        &self,
        name: &str,
        option: MetricOption,
    ) -> Result<Arc<dyn UpDownCounterPerformer>, Error>;
    /// Creates a histogram performer.
    fn histogram_performer(
        &self,
        name: &str,
        option: MetricOption,
    ) -> Result<Arc<dyn HistogramPerformer>, Error>;
}

/// A metric provider, like Go's `Provider`.
pub trait Provider: Send + Sync {
    /// Makes this provider global.
    fn set_as_global(&self);
    /// Returns a performer factory for `option`.
    fn meter_performer(&self, option: MeterOption) -> Option<Arc<dyn MeterPerformer>>;
    /// Flushes pending metrics.
    fn force_flush(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Shuts the provider down.
    fn shutdown(&self) -> Result<(), Error> {
        Ok(())
    }
}

/// A meter that creates metrics, like Go's `Meter`.
pub trait Meter: Send + Sync {
    /// Creates a counter.
    fn counter(&self, name: &str, option: MetricOption) -> Result<Counter, Error>;
    /// Creates an up-down counter.
    fn up_down_counter(&self, name: &str, option: MetricOption) -> Result<UpDownCounter, Error>;
    /// Creates a histogram.
    fn histogram(&self, name: &str, option: MetricOption) -> Result<Histogram, Error>;
    /// Creates an observable counter.
    fn observable_counter(
        &self,
        name: &str,
        option: MetricOption,
    ) -> Result<ObservableCounter, Error>;
    /// Creates an observable up-down counter.
    fn observable_up_down_counter(
        &self,
        name: &str,
        option: MetricOption,
    ) -> Result<ObservableUpDownCounter, Error>;
    /// Creates an observable gauge.
    fn observable_gauge(&self, name: &str, option: MetricOption) -> Result<ObservableGauge, Error>;
    /// Like [`Meter::counter`], but panics on error.
    fn must_counter(&self, name: &str, option: MetricOption) -> Counter {
        self.counter(name, option)
            .unwrap_or_else(|error| panic!("{error}"))
    }
    /// Like [`Meter::up_down_counter`], but panics on error.
    fn must_up_down_counter(&self, name: &str, option: MetricOption) -> UpDownCounter {
        self.up_down_counter(name, option)
            .unwrap_or_else(|error| panic!("{error}"))
    }
    /// Like [`Meter::histogram`], but panics on error.
    fn must_histogram(&self, name: &str, option: MetricOption) -> Histogram {
        self.histogram(name, option)
            .unwrap_or_else(|error| panic!("{error}"))
    }
    /// Like [`Meter::observable_counter`], but panics on error.
    fn must_observable_counter(&self, name: &str, option: MetricOption) -> ObservableCounter {
        self.observable_counter(name, option)
            .unwrap_or_else(|error| panic!("{error}"))
    }
    /// Like [`Meter::observable_up_down_counter`], but panics on error.
    fn must_observable_up_down_counter(
        &self,
        name: &str,
        option: MetricOption,
    ) -> ObservableUpDownCounter {
        self.observable_up_down_counter(name, option)
            .unwrap_or_else(|error| panic!("{error}"))
    }
    /// Like [`Meter::observable_gauge`], but panics on error.
    fn must_observable_gauge(&self, name: &str, option: MetricOption) -> ObservableGauge {
        self.observable_gauge(name, option)
            .unwrap_or_else(|error| panic!("{error}"))
    }
    /// Registers a callback on certain metrics.
    fn register_callback(
        &self,
        callback: Callback,
        metrics: Vec<Arc<dyn ObservableMetric>>,
    ) -> Result<(), Error>;
    /// Like [`Meter::register_callback`], but panics on error.
    fn must_register_callback(&self, callback: Callback, metrics: Vec<Arc<dyn ObservableMetric>>) {
        self.register_callback(callback, metrics)
            .unwrap_or_else(|error| panic!("{error}"));
    }
}

/// The local meter returned by the global provider, like Go's `localMeter`.
pub struct LocalMeter {
    option: MeterOption,
}

impl LocalMeter {
    /// Creates a meter over `option`.
    pub fn new(option: MeterOption) -> LocalMeter {
        LocalMeter { option }
    }

    /// The meter options.
    pub fn option(&self) -> &MeterOption {
        &self.option
    }

    /// Validates and builds the metric information, like `newMetric`.
    fn new_info(
        &self,
        metric_type: MetricType,
        name: &str,
        option: &MetricOption,
    ) -> Result<MetricInfo, Error> {
        if name.is_empty() {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("error creating {metric_type} metric while given name is empty"),
                Vec::new(),
            ));
        }
        if !gregex::is_match_string(METRIC_NAME_PATTERN, name) {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!(
                    "invalid metric name \"{name}\", should match regular expression pattern \"{METRIC_NAME_PATTERN}\""
                ),
                Vec::new(),
            ));
        }
        Ok(MetricInfo::new(
            metric_type,
            name,
            option,
            self.option.instrument_info(),
        ))
    }

    /// The performer factory of the global provider, if any.
    fn performer(&self) -> Option<Arc<dyn MeterPerformer>> {
        global_provider().and_then(|provider| provider.meter_performer(self.option.clone()))
    }
}

impl Meter for LocalMeter {
    fn counter(&self, name: &str, option: MetricOption) -> Result<Counter, Error> {
        let info = self.new_info(MetricType::Counter, name, &option)?;
        let performer = match self.performer() {
            Some(performer) => performer.counter_performer(name, option.clone())?,
            None => Arc::new(NoopCounterPerformer),
        };
        Ok(Counter { info, performer })
    }

    fn up_down_counter(&self, name: &str, option: MetricOption) -> Result<UpDownCounter, Error> {
        let info = self.new_info(MetricType::UpDownCounter, name, &option)?;
        let performer = match self.performer() {
            Some(performer) => performer.up_down_counter_performer(name, option.clone())?,
            None => Arc::new(NoopUpDownCounterPerformer),
        };
        Ok(UpDownCounter { info, performer })
    }

    fn histogram(&self, name: &str, option: MetricOption) -> Result<Histogram, Error> {
        let info = self.new_info(MetricType::Histogram, name, &option)?;
        let performer = match self.performer() {
            Some(performer) => performer.histogram_performer(name, option.clone())?,
            None => Arc::new(NoopHistogramPerformer),
        };
        Ok(Histogram {
            info,
            performer,
            buckets: option.buckets,
        })
    }

    fn observable_counter(
        &self,
        name: &str,
        option: MetricOption,
    ) -> Result<ObservableCounter, Error> {
        Ok(ObservableCounter {
            info: self.new_info(MetricType::ObservableCounter, name, &option)?,
        })
    }

    fn observable_up_down_counter(
        &self,
        name: &str,
        option: MetricOption,
    ) -> Result<ObservableUpDownCounter, Error> {
        Ok(ObservableUpDownCounter {
            info: self.new_info(MetricType::ObservableUpDownCounter, name, &option)?,
        })
    }

    fn observable_gauge(&self, name: &str, option: MetricOption) -> Result<ObservableGauge, Error> {
        Ok(ObservableGauge {
            info: self.new_info(MetricType::ObservableGauge, name, &option)?,
        })
    }

    fn register_callback(
        &self,
        callback: Callback,
        metrics: Vec<Arc<dyn ObservableMetric>>,
    ) -> Result<(), Error> {
        if metrics.is_empty() {
            return Ok(());
        }
        callback_items()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(CallbackItem {
                callback,
                metrics,
                meter_option: self.option.clone(),
            });
        Ok(())
    }
}

/// One registered callback, like Go's `CallbackItem`.
pub struct CallbackItem {
    /// The callback.
    pub callback: Callback,
    /// The metrics it observes.
    pub metrics: Vec<Arc<dyn ObservableMetric>>,
    /// The meter options of the registering meter.
    pub meter_option: MeterOption,
}

fn callback_items() -> &'static Mutex<Vec<CallbackItem>> {
    static ITEMS: OnceLock<Mutex<Vec<CallbackItem>>> = OnceLock::new();
    ITEMS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Drains the registered callbacks, like `GetRegisteredCallbacks`.
pub fn get_registered_callbacks() -> Vec<CallbackItem> {
    std::mem::take(
        &mut *callback_items()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// The global provider holder, like Go's `globalProvider`.
fn global_provider_slot() -> &'static RwLock<Option<Arc<dyn Provider>>> {
    static PROVIDER: OnceLock<RwLock<Option<Arc<dyn Provider>>>> = OnceLock::new();
    PROVIDER.get_or_init(|| RwLock::new(None))
}

/// Returns the global provider when one was set.
pub fn global_provider() -> Option<Arc<dyn Provider>> {
    global_provider_slot()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Registers `provider` as the global provider, like `SetGlobalProvider`.
pub fn set_global_provider(provider: Option<Arc<dyn Provider>>) {
    *global_provider_slot()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = provider;
}

/// Reports whether metrics are enabled, like `IsEnabled`.
pub fn is_enabled() -> bool {
    global_provider().is_some()
}

/// The package convenience provider, like Go's `GlobalProvider`.
pub struct GlobalProvider;

/// Returns the global provider facade, like `GetGlobalProvider`.
pub fn get_global_provider() -> GlobalProvider {
    GlobalProvider
}

impl GlobalProvider {
    /// Creates a meter, like `Meter`.
    pub fn meter(&self, option: MeterOption) -> LocalMeter {
        LocalMeter::new(option)
    }
}

/// The options for registering global attributes, like
/// `SetGlobalAttributesOption`.
#[derive(Clone, Debug, Default)]
pub struct SetGlobalAttributesOption {
    /// Instrument name.
    pub instrument: String,
    /// Instrument version.
    pub instrument_version: String,
    /// Instrument name regular expression.
    pub instrument_pattern: String,
}

/// The query options for global attributes, like `GetGlobalAttributesOption`.
#[derive(Clone, Debug, Default)]
pub struct GetGlobalAttributesOption {
    /// Instrument name.
    pub instrument: String,
    /// Instrument version.
    pub instrument_version: String,
}

struct GlobalAttributeItem {
    attributes: Attributes,
    option: SetGlobalAttributesOption,
}

fn global_attributes() -> &'static Mutex<Vec<GlobalAttributeItem>> {
    static ATTRIBUTES: OnceLock<Mutex<Vec<GlobalAttributeItem>>> = OnceLock::new();
    ATTRIBUTES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Appends global attributes, like `SetGlobalAttributes`.
pub fn set_global_attributes(attributes: Attributes, option: SetGlobalAttributesOption) {
    global_attributes()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(GlobalAttributeItem { attributes, option });
}

/// Returns global attributes matching `option`, like `GetGlobalAttributes`.
pub fn get_global_attributes(option: GetGlobalAttributesOption) -> Attributes {
    let items = global_attributes()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut attributes = Attributes::new();
    for item in items.iter() {
        if !item.option.instrument_pattern.is_empty() {
            if !gregex::is_match_string(&item.option.instrument_pattern, &option.instrument) {
                continue;
            }
        } else if (!item.option.instrument.is_empty() || !option.instrument.is_empty())
            && item.option.instrument != option.instrument
        {
            continue;
        }
        if (!item.option.instrument_version.is_empty() || !option.instrument_version.is_empty())
            && item.option.instrument_version != option.instrument_version
        {
            continue;
        }
        attributes.extend(item.attributes.iter().cloned());
    }
    attributes
}

/// Removes every global attribute (used by tests).
pub fn clear_global_attributes() {
    global_attributes()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}

// ---------------------------------------------------------------------------
// In-memory backend (ADR-184)
// ---------------------------------------------------------------------------

/// An in-memory counter performer.
#[derive(Debug, Default)]
pub struct MemoryCounter {
    value: Mutex<f64>,
}

impl MemoryCounter {
    /// The current counter value.
    pub fn value(&self) -> f64 {
        *self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl CounterPerformer for MemoryCounter {
    fn inc(&self, _option: Option<ObservationOption>) {
        *self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) += 1.0;
    }

    fn add(&self, increment: f64, _option: Option<ObservationOption>) {
        *self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) += increment;
    }
}

/// An in-memory up-down counter performer.
#[derive(Debug, Default)]
pub struct MemoryUpDownCounter {
    value: Mutex<f64>,
}

impl MemoryUpDownCounter {
    /// The current up-down counter value.
    pub fn value(&self) -> f64 {
        *self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl UpDownCounterPerformer for MemoryUpDownCounter {
    fn inc(&self, _option: Option<ObservationOption>) {
        *self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) += 1.0;
    }

    fn dec(&self, _option: Option<ObservationOption>) {
        *self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) -= 1.0;
    }

    fn add(&self, increment: f64, _option: Option<ObservationOption>) {
        *self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) += increment;
    }
}

/// A histogram snapshot, like an OpenTelemetry data point.
#[derive(Clone, Debug, PartialEq)]
pub struct HistogramSnapshot {
    /// The number of recorded values.
    pub count: usize,
    /// The sum of the recorded values.
    pub sum: f64,
    /// The smallest value, when at least one was recorded.
    pub min: Option<f64>,
    /// The largest value, when at least one was recorded.
    pub max: Option<f64>,
    /// The recorded values in order.
    pub values: Vec<f64>,
}

/// An in-memory histogram performer.
#[derive(Debug, Default)]
pub struct MemoryHistogram {
    values: Mutex<Vec<f64>>,
}

impl MemoryHistogram {
    /// The current histogram snapshot.
    pub fn snapshot(&self) -> HistogramSnapshot {
        let values = self
            .values
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let sum = values.iter().sum();
        let min = values.iter().copied().reduce(f64::min);
        let max = values.iter().copied().reduce(f64::max);
        HistogramSnapshot {
            count: values.len(),
            sum,
            min,
            max,
            values,
        }
    }
}

impl HistogramPerformer for MemoryHistogram {
    fn record(&self, increment: f64, _option: Option<ObservationOption>) {
        self.values
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(increment);
    }
}

/// One instrument's snapshot from a [`MemoryProvider`].
#[derive(Clone, Debug, PartialEq)]
pub enum InstrumentSnapshot {
    /// A counter value.
    Counter(f64),
    /// An up-down counter value.
    UpDownCounter(f64),
    /// A histogram snapshot.
    Histogram(HistogramSnapshot),
}

/// A snapshot of every instrument kept by a [`MemoryProvider`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MemorySnapshot {
    /// Counter values by instrument name.
    pub counters: IndexMap<String, f64>,
    /// Up-down counter values by instrument name.
    pub up_down_counters: IndexMap<String, f64>,
    /// Histogram snapshots by instrument name.
    pub histograms: IndexMap<String, HistogramSnapshot>,
}

#[derive(Default)]
struct MemoryRegistry {
    counters: Mutex<IndexMap<String, Arc<MemoryCounter>>>,
    up_down_counters: Mutex<IndexMap<String, Arc<MemoryUpDownCounter>>>,
    histograms: Mutex<IndexMap<String, Arc<MemoryHistogram>>>,
}

#[derive(Clone, Default)]
struct MemoryMeterPerformer {
    registry: Arc<MemoryRegistry>,
}

impl MeterPerformer for MemoryMeterPerformer {
    fn counter_performer(
        &self,
        name: &str,
        _option: MetricOption,
    ) -> Result<Arc<dyn CounterPerformer>, Error> {
        let counter = Arc::new(MemoryCounter::default());
        self.registry
            .counters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name.to_owned(), Arc::clone(&counter));
        Ok(counter)
    }

    fn up_down_counter_performer(
        &self,
        name: &str,
        _option: MetricOption,
    ) -> Result<Arc<dyn UpDownCounterPerformer>, Error> {
        let counter = Arc::new(MemoryUpDownCounter::default());
        self.registry
            .up_down_counters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name.to_owned(), Arc::clone(&counter));
        Ok(counter)
    }

    fn histogram_performer(
        &self,
        name: &str,
        _option: MetricOption,
    ) -> Result<Arc<dyn HistogramPerformer>, Error> {
        let histogram = Arc::new(MemoryHistogram::default());
        self.registry
            .histograms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name.to_owned(), Arc::clone(&histogram));
        Ok(histogram)
    }
}

/// A provider that keeps every measurement in memory, the port's equivalent
/// of an OpenTelemetry in-memory exporter (ADR-184).
pub struct MemoryProvider {
    registry: Arc<MemoryRegistry>,
    self_ref: OnceLock<Weak<MemoryProvider>>,
}

impl Default for MemoryProvider {
    fn default() -> MemoryProvider {
        MemoryProvider {
            registry: Arc::new(MemoryRegistry::default()),
            self_ref: OnceLock::new(),
        }
    }
}

impl MemoryProvider {
    /// Creates a provider and returns it behind an `Arc` so it can install
    /// itself as the global provider.
    pub fn new() -> Arc<MemoryProvider> {
        let provider = Arc::new(MemoryProvider::default());
        let _ = provider.self_ref.set(Arc::downgrade(&provider));
        provider
    }

    /// Installs this provider as the global provider, like
    /// `SetGlobalProvider`.
    pub fn install(self: &Arc<Self>) {
        set_global_provider(Some(Arc::clone(self) as Arc<dyn Provider>));
    }

    /// The current counter value of `name`.
    pub fn counter_value(&self, name: &str) -> Option<f64> {
        self.registry
            .counters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(name)
            .map(|counter| counter.value())
    }

    /// The current up-down counter value of `name`.
    pub fn up_down_counter_value(&self, name: &str) -> Option<f64> {
        self.registry
            .up_down_counters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(name)
            .map(|counter| counter.value())
    }

    /// The current histogram snapshot of `name`.
    pub fn histogram(&self, name: &str) -> Option<HistogramSnapshot> {
        self.registry
            .histograms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(name)
            .map(|histogram| histogram.snapshot())
    }

    /// Snapshots every instrument.
    pub fn snapshot(&self) -> MemorySnapshot {
        let counters = self
            .registry
            .counters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(name, counter)| (name.clone(), counter.value()))
            .collect();
        let up_down_counters = self
            .registry
            .up_down_counters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(name, counter)| (name.clone(), counter.value()))
            .collect();
        let histograms = self
            .registry
            .histograms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|(name, histogram)| (name.clone(), histogram.snapshot()))
            .collect();
        MemorySnapshot {
            counters,
            up_down_counters,
            histograms,
        }
    }
}

impl Provider for MemoryProvider {
    fn set_as_global(&self) {
        if let Some(provider) = self.self_ref.get().and_then(Weak::upgrade) {
            set_global_provider(Some(provider as Arc<dyn Provider>));
        }
    }

    fn meter_performer(&self, _option: MeterOption) -> Option<Arc<dyn MeterPerformer>> {
        Some(Arc::new(MemoryMeterPerformer {
            registry: Arc::clone(&self.registry),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Translated from Test_Counter.
    #[test]
    fn counter_exposes_its_info() {
        let meter = get_global_provider().meter(MeterOption {
            instrument: "github.com/gogf/gf/example/metric/basic".to_owned(),
            instrument_version: "v1.0".to_owned(),
            ..MeterOption::default()
        });
        let option = MetricOption {
            help: "This is a simple demo for Counter usage".to_owned(),
            unit: "%".to_owned(),
            attributes: vec![Attribute::new("const_label_a", Value::Int(1))],
            ..MetricOption::default()
        };
        let counter = meter.must_counter("goframe.metric.demo.counter", option.clone());
        assert_eq!(counter.info().name(), "goframe.metric.demo.counter");
        assert_eq!(counter.info().help(), option.help);
        assert_eq!(counter.info().unit(), "%");
        assert_eq!(counter.info().attributes(), &option.attributes);
        assert_eq!(
            counter.info().instrument().name,
            "github.com/gogf/gf/example/metric/basic"
        );
        assert_eq!(counter.info().instrument().version, "v1.0");
        assert_eq!(counter.info().metric_type(), MetricType::Counter);
        assert_eq!(
            counter.info().key(),
            "github.com/gogf/gf/example/metric/basic@v1.0:goframe.metric.demo.counter"
        );
        // Without a provider the noop performer accepts measurements.
        counter.inc(None);
        counter.add(2.0, None);
    }

    // Translated from Test_Histogram.
    #[test]
    fn histogram_keeps_its_buckets() {
        let meter = get_global_provider().meter(MeterOption {
            instrument: "github.com/gogf/gf/example/metric/basic".to_owned(),
            instrument_version: "v1.0".to_owned(),
            ..MeterOption::default()
        });
        let buckets = vec![0.0, 10.0, 20.0, 50.0, 100.0];
        let histogram = meter.must_histogram(
            "goframe.metric.demo.histogram",
            MetricOption {
                help: "histogram".to_owned(),
                buckets: buckets.clone(),
                ..MetricOption::default()
            },
        );
        assert_eq!(histogram.buckets(), buckets.as_slice());
        assert_eq!(histogram.info().metric_type(), MetricType::Histogram);
        histogram.record(12.0, None);
    }

    #[test]
    fn invalid_metric_names_are_rejected() {
        let meter = get_global_provider().meter(MeterOption::default());
        assert!(
            meter.counter("", MetricOption::default()).is_err(),
            "empty names are rejected"
        );
        assert!(
            meter.counter("!!!", MetricOption::default()).is_err(),
            "names without allowed characters are rejected"
        );
        assert!(meter.counter("valid.name", MetricOption::default()).is_ok());
    }

    #[test]
    fn observable_metrics_and_callbacks() {
        let meter = get_global_provider().meter(MeterOption::default());
        let gauge = meter.must_observable_gauge("demo.gauge", MetricOption::default());
        assert_eq!(gauge.info().metric_type(), MetricType::ObservableGauge);
        let up_down = meter.must_observable_up_down_counter("demo.updown", MetricOption::default());
        assert_eq!(
            up_down.info().metric_type(),
            MetricType::ObservableUpDownCounter
        );
        let counter = meter.must_observable_counter("demo.observable", MetricOption::default());
        assert_eq!(counter.info().metric_type(), MetricType::ObservableCounter);

        // A registered callback is queued once and drained by the getter.
        let metric: Arc<dyn ObservableMetric> = Arc::new(gauge);
        meter
            .register_callback(Arc::new(|_observer: &dyn Observer| Ok(())), vec![metric])
            .expect("register");
        assert!(!get_registered_callbacks().is_empty());
        assert!(get_registered_callbacks().is_empty(), "drained");
        // No metrics means no queue entry.
        meter
            .register_callback(Arc::new(|_observer: &dyn Observer| Ok(())), Vec::new())
            .expect("register empty");
        assert!(get_registered_callbacks().is_empty());
    }

    // Translated from Test_GlobalAttributes.
    #[test]
    fn global_attributes_match_instruments() {
        clear_global_attributes();
        set_global_attributes(
            vec![Attribute::new("global", Value::String("gl".to_owned()))],
            SetGlobalAttributesOption::default(),
        );
        set_global_attributes(
            vec![Attribute::new("a", Value::Int(1))],
            SetGlobalAttributesOption {
                instrument: "ins_a".to_owned(),
                instrument_version: "v1.0".to_owned(),
                ..SetGlobalAttributesOption::default()
            },
        );
        for (key, value) in [("b", 2_i64), ("c", 3_i64)] {
            set_global_attributes(
                vec![Attribute::new(key, Value::Int(value))],
                SetGlobalAttributesOption {
                    instrument: "ins_bb".to_owned(),
                    instrument_version: "v1.1".to_owned(),
                    ..SetGlobalAttributesOption::default()
                },
            );
        }
        set_global_attributes(
            vec![Attribute::new("d", Value::Int(4))],
            SetGlobalAttributesOption {
                instrument_pattern: "ins.+".to_owned(),
                instrument_version: "v1.0".to_owned(),
                ..SetGlobalAttributesOption::default()
            },
        );
        set_global_attributes(
            vec![Attribute::new("e", Value::Int(5))],
            SetGlobalAttributesOption {
                instrument_pattern: "ins_b.+".to_owned(),
                instrument_version: "v1.0".to_owned(),
                ..SetGlobalAttributesOption::default()
            },
        );
        set_global_attributes(
            vec![Attribute::new("f", Value::Int(6))],
            SetGlobalAttributesOption {
                instrument_pattern: "ins_b.+".to_owned(),
                instrument_version: "v1.1".to_owned(),
                ..SetGlobalAttributesOption::default()
            },
        );

        assert_eq!(
            get_global_attributes(GetGlobalAttributesOption::default()),
            vec![Attribute::new("global", Value::String("gl".to_owned()))]
        );
        assert_eq!(
            get_global_attributes(GetGlobalAttributesOption {
                instrument: "ins_a".to_owned(),
                ..GetGlobalAttributesOption::default()
            }),
            Attributes::new()
        );
        assert_eq!(
            get_global_attributes(GetGlobalAttributesOption {
                instrument: "ins_a".to_owned(),
                instrument_version: "1.1".to_owned(),
            }),
            Attributes::new()
        );
        assert_eq!(
            get_global_attributes(GetGlobalAttributesOption {
                instrument: "ins_bb".to_owned(),
                instrument_version: "v1.0".to_owned(),
            }),
            vec![
                Attribute::new("d", Value::Int(4)),
                Attribute::new("e", Value::Int(5)),
            ]
        );
        assert_eq!(
            get_global_attributes(GetGlobalAttributesOption {
                instrument: "ins_bb".to_owned(),
                instrument_version: "v1.1".to_owned(),
            }),
            vec![
                Attribute::new("b", Value::Int(2)),
                Attribute::new("c", Value::Int(3)),
                Attribute::new("f", Value::Int(6)),
            ]
        );
        assert_eq!(
            get_global_attributes(GetGlobalAttributesOption {
                instrument: "ins_cc".to_owned(),
                instrument_version: "v1.1".to_owned(),
            }),
            Attributes::new()
        );
        clear_global_attributes();
    }

    #[test]
    fn common_attributes_have_host_and_path() {
        let attributes = common_attributes();
        assert!(!attributes.is_empty());
        assert_eq!(attributes[0].key(), "os.host.name");
        assert_eq!(attributes[1].key(), "process.path");
    }

    fn provider_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn provider_enables_metrics() {
        let _guard = provider_lock();
        struct TestProvider;
        impl Provider for TestProvider {
            fn set_as_global(&self) {
                set_global_provider(Some(Arc::new(TestProvider)));
            }

            fn meter_performer(&self, _option: MeterOption) -> Option<Arc<dyn MeterPerformer>> {
                None
            }
        }
        assert!(!is_enabled());
        let provider: Arc<dyn Provider> = Arc::new(TestProvider);
        set_global_provider(Some(Arc::clone(&provider)));
        assert!(is_enabled());
        let meter = get_global_provider().meter(MeterOption::default());
        // The provider has no performer factory, so the instrument is a noop.
        meter.must_counter("demo.counter", MetricOption::default());
        set_global_provider(None);
        assert!(!is_enabled());
    }

    #[test]
    fn memory_provider_collects_measurements() {
        let _guard = provider_lock();
        let provider = MemoryProvider::new();
        provider.install();
        assert!(is_enabled());
        let meter = get_global_provider().meter(MeterOption::default());

        let counter = meter
            .counter("requests", MetricOption::default())
            .expect("counter");
        counter.inc(None);
        counter.add(2.0, None);

        let up_down = meter
            .up_down_counter("queue", MetricOption::default())
            .expect("up-down counter");
        up_down.inc(None);
        up_down.inc(None);
        up_down.dec(None);

        let histogram = meter
            .histogram("latency", MetricOption::default())
            .expect("histogram");
        histogram.record(1.5, None);
        histogram.record(2.5, None);

        assert_eq!(provider.counter_value("requests"), Some(3.0));
        assert_eq!(provider.up_down_counter_value("queue"), Some(1.0));
        let snapshot = provider.histogram("latency").expect("histogram snapshot");
        assert_eq!(snapshot.count, 2);
        assert_eq!(snapshot.sum, 4.0);
        assert_eq!(snapshot.min, Some(1.5));
        assert_eq!(snapshot.max, Some(2.5));
        assert_eq!(snapshot.values, vec![1.5, 2.5]);

        let all = provider.snapshot();
        assert_eq!(all.counters.get("requests"), Some(&3.0));
        assert_eq!(all.up_down_counters.get("queue"), Some(&1.0));
        assert_eq!(
            all.histograms.get("latency").map(|item| item.count),
            Some(2)
        );

        set_global_provider(None);
        assert!(!is_enabled());
    }
}
