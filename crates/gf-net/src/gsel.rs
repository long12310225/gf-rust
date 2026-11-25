//! Port of `net/gsel` — service load-balancing selectors.
//!
//! The port keeps the GoFrame selector set: round-robin, random, weight and
//! least-connection, plus the builder registry. There is no context
//! (ADR-034), so `pick`/`update` take no `context.Context`.
//!
//! Divergences, recorded in ADR-062:
//!
//! - `Pick` returns `Result<Option<Picked>, Error>`; a selector with no nodes
//!   returns `Ok(None)` like Go's `(nil, nil, nil)`;
//! - the least-connection `done` callback takes [`DoneInfo`] without a context;
//! - weight comes from the service metadata key `weight`; a node without that
//!   key is dropped from the weighted list, like Go's zero weight.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use gf_core::Value;
use gf_errors::gerror::Error;
use gf_util::grand;

use crate::gsvc::{MD_WEIGHT, Service};

/// One selectable node, like Go's `Node`.
#[derive(Clone)]
pub struct Node {
    service: Arc<dyn Service>,
    address: String,
}

impl Node {
    /// Creates a node for `service` at `address`.
    pub fn new(service: Arc<dyn Service>, address: &str) -> Node {
        Node {
            service,
            address: address.to_owned(),
        }
    }

    /// The bound service.
    pub fn service(&self) -> &Arc<dyn Service> {
        &self.service
    }

    /// The node address.
    pub fn address(&self) -> &str {
        &self.address
    }
}

/// Returns the comma-separated node addresses, like `Nodes.String`.
pub fn nodes_string(nodes: &[Node]) -> String {
    nodes
        .iter()
        .map(|node| node.address.clone())
        .collect::<Vec<String>>()
        .join(",")
}

/// The information passed to the done callback, like Go's `DoneInfo`.
#[derive(Clone, Debug)]
pub struct DoneInfo {
    /// The RPC error, if any.
    pub err: Option<String>,
    /// Whether any bytes were sent.
    pub bytes_sent: bool,
    /// Whether any bytes were received.
    pub bytes_received: bool,
    /// The server load report, if any.
    pub server_load: Value,
}

impl Default for DoneInfo {
    fn default() -> DoneInfo {
        DoneInfo {
            err: None,
            bytes_sent: false,
            bytes_received: false,
            server_load: Value::Null,
        }
    }
}

/// The callback invoked when an RPC is done, like Go's `DoneFunc`.
pub type DoneFunc = Arc<dyn Fn(DoneInfo) + Send + Sync>;

/// One picked node plus its done callback.
pub struct Picked {
    /// The picked node.
    pub node: Node,
    /// The done callback, when the selector tracks in-flight calls.
    pub done: Option<DoneFunc>,
}

/// The service selector, like Go's `Selector`.
pub trait Selector: Send + Sync {
    /// Picks one node, or `None` when no nodes are available.
    fn pick(&self) -> Result<Option<Picked>, Error>;
    /// Replaces the node list.
    fn update(&self, nodes: Vec<Node>) -> Result<(), Error>;
}

/// The round-robin selector, like `NewSelectorRoundRobin`.
pub struct SelectorRoundRobin {
    state: Mutex<(Vec<Node>, usize)>,
}

impl Default for SelectorRoundRobin {
    fn default() -> SelectorRoundRobin {
        SelectorRoundRobin::new()
    }
}

impl SelectorRoundRobin {
    /// Creates a round-robin selector.
    pub fn new() -> SelectorRoundRobin {
        SelectorRoundRobin {
            state: Mutex::new((Vec::new(), 0)),
        }
    }
}

impl Selector for SelectorRoundRobin {
    fn pick(&self) -> Result<Option<Picked>, Error> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.0.is_empty() {
            return Ok(None);
        }
        let node = state.0[state.1].clone();
        state.1 = (state.1 + 1) % state.0.len();
        Ok(Some(Picked { node, done: None }))
    }

    fn update(&self, nodes: Vec<Node>) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.0 = nodes;
        state.1 = 0;
        Ok(())
    }
}

/// The random selector, like `NewSelectorRandom`.
pub struct SelectorRandom {
    nodes: RwLock<Vec<Node>>,
}

impl Default for SelectorRandom {
    fn default() -> SelectorRandom {
        SelectorRandom::new()
    }
}

impl SelectorRandom {
    /// Creates a random selector.
    pub fn new() -> SelectorRandom {
        SelectorRandom {
            nodes: RwLock::new(Vec::new()),
        }
    }
}

impl Selector for SelectorRandom {
    fn pick(&self) -> Result<Option<Picked>, Error> {
        let nodes = self
            .nodes
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if nodes.is_empty() {
            return Ok(None);
        }
        let index = grand::intn(nodes.len() as i64) as usize;
        Ok(Some(Picked {
            node: nodes[index].clone(),
            done: None,
        }))
    }

    fn update(&self, nodes: Vec<Node>) -> Result<(), Error> {
        *self
            .nodes
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = nodes;
        Ok(())
    }
}

/// The weighted random selector, like `NewSelectorWeight`.
pub struct SelectorWeight {
    nodes: RwLock<Vec<Node>>,
}

impl Default for SelectorWeight {
    fn default() -> SelectorWeight {
        SelectorWeight::new()
    }
}

impl SelectorWeight {
    /// Creates a weighted random selector.
    pub fn new() -> SelectorWeight {
        SelectorWeight {
            nodes: RwLock::new(Vec::new()),
        }
    }

    /// The weight of one node from its `weight` metadata (missing = 0).
    fn weight(node: &Node) -> i64 {
        node.service()
            .get_metadata()
            .get(MD_WEIGHT)
            .map(|value| value.int64())
            .unwrap_or(0)
    }
}

impl Selector for SelectorWeight {
    fn pick(&self) -> Result<Option<Picked>, Error> {
        let nodes = self
            .nodes
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if nodes.is_empty() {
            return Ok(None);
        }
        let index = grand::intn(nodes.len() as i64) as usize;
        Ok(Some(Picked {
            node: nodes[index].clone(),
            done: None,
        }))
    }

    fn update(&self, nodes: Vec<Node>) -> Result<(), Error> {
        let mut weighted = Vec::new();
        for node in nodes {
            for _ in 0..SelectorWeight::weight(&node).max(0) {
                weighted.push(node.clone());
            }
        }
        *self
            .nodes
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = weighted;
        Ok(())
    }
}

/// The least-connection selector, like `NewSelectorLeastConnection`.
pub struct SelectorLeastConnection {
    nodes: Mutex<Vec<(Node, Arc<AtomicI64>)>>,
}

impl Default for SelectorLeastConnection {
    fn default() -> SelectorLeastConnection {
        SelectorLeastConnection::new()
    }
}

impl SelectorLeastConnection {
    /// Creates a least-connection selector.
    pub fn new() -> SelectorLeastConnection {
        SelectorLeastConnection {
            nodes: Mutex::new(Vec::new()),
        }
    }
}

impl Selector for SelectorLeastConnection {
    fn pick(&self) -> Result<Option<Picked>, Error> {
        let nodes = self
            .nodes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some((picked, counter)) = nodes
            .iter()
            .min_by_key(|(_, counter)| counter.load(Ordering::SeqCst))
            .cloned()
        else {
            return Ok(None);
        };
        counter.fetch_add(1, Ordering::SeqCst);
        let done_counter = Arc::clone(&counter);
        let done: DoneFunc = Arc::new(move |_info: DoneInfo| {
            done_counter.fetch_sub(1, Ordering::SeqCst);
        });
        Ok(Some(Picked {
            node: picked,
            done: Some(done),
        }))
    }

    fn update(&self, nodes: Vec<Node>) -> Result<(), Error> {
        let mut current = self
            .nodes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = nodes
            .into_iter()
            .map(|node| (node, Arc::new(AtomicI64::new(0))))
            .collect();
        Ok(())
    }
}

/// A selector builder, like Go's `Builder`.
pub trait Builder: Send + Sync {
    /// The builder name.
    fn name(&self) -> &str;
    /// Builds a selector.
    fn build(&self) -> Box<dyn Selector>;
}

/// The round-robin builder, like `NewBuilderRoundRobin`.
pub struct BuilderRoundRobin;

impl Builder for BuilderRoundRobin {
    fn name(&self) -> &str {
        "BalancerRoundRobin"
    }

    fn build(&self) -> Box<dyn Selector> {
        Box::new(SelectorRoundRobin::new())
    }
}

/// The random builder, like `NewBuilderRandom`.
pub struct BuilderRandom;

impl Builder for BuilderRandom {
    fn name(&self) -> &str {
        "BalancerRandom"
    }

    fn build(&self) -> Box<dyn Selector> {
        Box::new(SelectorRandom::new())
    }
}

/// The weight builder, like `NewBuilderWeight`.
pub struct BuilderWeight;

impl Builder for BuilderWeight {
    fn name(&self) -> &str {
        "BalancerWeight"
    }

    fn build(&self) -> Box<dyn Selector> {
        Box::new(SelectorWeight::new())
    }
}

/// The least-connection builder, like `NewBuilderLeastConnection`.
pub struct BuilderLeastConnection;

impl Builder for BuilderLeastConnection {
    fn name(&self) -> &str {
        "BalancerLeastConnection"
    }

    fn build(&self) -> Box<dyn Selector> {
        Box::new(SelectorLeastConnection::new())
    }
}

/// Returns a round-robin builder, like `NewBuilderRoundRobin`.
pub fn new_builder_round_robin() -> Arc<dyn Builder> {
    Arc::new(BuilderRoundRobin)
}

/// Returns a random builder, like `NewBuilderRandom`.
pub fn new_builder_random() -> Arc<dyn Builder> {
    Arc::new(BuilderRandom)
}

/// Returns a weight builder, like `NewBuilderWeight`.
pub fn new_builder_weight() -> Arc<dyn Builder> {
    Arc::new(BuilderWeight)
}

/// Returns a least-connection builder, like `NewBuilderLeastConnection`.
pub fn new_builder_least_connection() -> Arc<dyn Builder> {
    Arc::new(BuilderLeastConnection)
}

fn default_builder_slot() -> &'static RwLock<Arc<dyn Builder>> {
    static BUILDER: OnceLock<RwLock<Arc<dyn Builder>>> = OnceLock::new();
    BUILDER.get_or_init(|| RwLock::new(Arc::new(BuilderRoundRobin)))
}

/// Sets the default builder, like `SetBuilder`.
pub fn set_builder(builder: Arc<dyn Builder>) {
    *default_builder_slot()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = builder;
}

/// Returns the default builder, like `GetBuilder`.
pub fn get_builder() -> Arc<dyn Builder> {
    Arc::clone(
        &default_builder_slot()
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gsvc::{Endpoints, LocalService};

    fn service(name: &str, weight: Option<i64>) -> Arc<dyn Service> {
        let mut service = LocalService::new_service_with_name(name);
        service.endpoints = Endpoints::new("127.0.0.1:8000").expect("endpoint");
        if let Some(weight) = weight {
            service.metadata.set(MD_WEIGHT, Value::Int(weight));
        }
        Arc::new(service)
    }

    fn nodes() -> Vec<Node> {
        vec![
            Node::new(service("a", None), "a:1"),
            Node::new(service("b", None), "b:2"),
            Node::new(service("c", None), "c:3"),
        ]
    }

    #[test]
    fn round_robin_cycles() {
        let selector = SelectorRoundRobin::new();
        assert!(selector.pick().expect("pick").is_none());
        selector.update(nodes()).expect("update");
        let mut addresses = Vec::new();
        for _ in 0..6 {
            if let Some(picked) = selector.pick().expect("pick") {
                addresses.push(picked.node.address().to_owned());
            }
        }
        assert_eq!(addresses, vec!["a:1", "b:2", "c:3", "a:1", "b:2", "c:3"]);
        assert_eq!(nodes_string(&nodes()), "a:1,b:2,c:3");
    }

    #[test]
    fn random_picks_only_members() {
        let selector = SelectorRandom::new();
        assert!(selector.pick().expect("pick").is_none());
        selector.update(nodes()).expect("update");
        for _ in 0..30 {
            let picked = selector.pick().expect("pick").expect("node");
            assert!(nodes_string(&nodes()).contains(picked.node.address()));
        }
    }

    #[test]
    fn weight_favours_heavier_nodes() {
        let selector = SelectorWeight::new();
        selector
            .update(vec![
                Node::new(service("light", Some(1)), "light:1"),
                Node::new(service("heavy", Some(5)), "heavy:2"),
                Node::new(service("zero", Some(0)), "zero:3"),
            ])
            .expect("update");
        let mut heavy = 0;
        for _ in 0..120 {
            let picked = selector.pick().expect("pick").expect("node");
            assert_ne!(picked.node.address(), "zero:3", "zero weight is dropped");
            if picked.node.address() == "heavy:2" {
                heavy += 1;
            }
        }
        assert!(heavy > 60, "heavy picked {heavy} times");
    }

    #[test]
    fn least_connection_tracks_inflight() {
        let selector = SelectorLeastConnection::new();
        selector.update(nodes()).expect("update");
        let first = selector.pick().expect("pick").expect("node");
        assert_eq!(first.node.address(), "a:1");
        let second = selector.pick().expect("pick").expect("node");
        assert_eq!(second.node.address(), "b:2");
        // Finishing the first call frees `a`, so it becomes the least loaded.
        (first.done.expect("done"))(DoneInfo::default());
        let third = selector.pick().expect("pick").expect("node");
        assert_eq!(third.node.address(), "a:1");
        assert!(second.done.is_some());
    }

    #[test]
    fn builders_and_default() {
        assert_eq!(get_builder().name(), "BalancerRoundRobin");
        assert_eq!(new_builder_random().name(), "BalancerRandom");
        assert_eq!(new_builder_weight().name(), "BalancerWeight");
        assert_eq!(
            new_builder_least_connection().name(),
            "BalancerLeastConnection"
        );
        assert!(new_builder_random().build().pick().expect("pick").is_none());
        set_builder(new_builder_random());
        assert_eq!(get_builder().name(), "BalancerRandom");
        set_builder(new_builder_round_robin());
    }

    #[test]
    fn metadata_without_weight_is_not_selectable() {
        let selector = SelectorWeight::new();
        selector.update(nodes()).expect("update");
        assert!(selector.pick().expect("pick").is_none());
    }
}
