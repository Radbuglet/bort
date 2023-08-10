use std::{any::TypeId, fmt::Write, rc::Rc};

use petgraph::{
    algo::toposort,
    graph::{EdgeIndex, NodeIndex},
    visit::EdgeRef,
    Graph,
};
use rustc_hash::FxHashMap;

// === Helpers === //

#[derive(Copy, Clone)]
pub struct DefInfo {
    pub name: &'static str,
    pub path: &'static str,
}

#[derive(Copy, Clone)]
pub enum Mutability {
    Mutable,
    Immutable,
}

impl Mutability {
    pub fn adjective(self) -> &'static str {
        match self {
            Mutability::Mutable => "mutably",
            Mutability::Immutable => "immutably",
        }
    }

    pub fn is_compatible_with(self, other: Mutability) -> bool {
        use Mutability::*;
        matches!((self, other), (Immutable, Immutable))
    }

    pub fn strictest(self, other: Mutability) -> Self {
        use Mutability::*;
        if matches!((self, other), (Immutable, Immutable)) {
            Immutable
        } else {
            Mutable
        }
    }
}

// === Validator === //

pub struct Validator {
    /// The graph of behavior namespaces connected by the behaviors which could possibly call into
    /// other namespaces.
    graph: Graph<Namespace, Rc<Behavior>>,

    /// A map from universe type to definition info.
    universe_def_infos: FxHashMap<TypeId, DefInfo>,

    /// A map from component type IDs to component type names.
    component_names: FxHashMap<TypeId, &'static str>,
}

struct Namespace {
    /// The `TypeId` of the universe in which this namespace lives.
    universe: TypeId,

    /// The location where this namespace was defined.
    def_info: DefInfo,

    /// The set of behaviors which borrow data in the namespace but don't actually call into any other
    /// behaviors.
    terminal_behaviors: Vec<Behavior>,
}

struct Behavior {
    /// The location where the behavior was defined.
    def_info: DefInfo,

    /// The set of components borrowed by the behavior.
    borrows: FxHashMap<TypeId, Mutability>,
}

impl Validator {
    pub fn validate(&mut self) {
        // Assuming our graph is a DAG, toposort the namespaces.
        let Ok(topos) = toposort(&self.graph, None) else {
			// If the graph is not a DAG, we know that it is invalid since a dependency issue could
			// be induced by taking the same borrowing edge several times.
			//
			// We generate a list of offending namespaces using "Tarjan's strongly connected components
			// algorithm." A strongly connected component (or SCC) is a set of nodes in a graph
			// where each node in the set has a path to another node in that set. We know that
			// finding the SCCs in a graph is an effective way of finding portions of the graph
			// containing cycles because:
			//
			// 1. If the graph contains a cycle, that cycle will be part of an SCC (although the SCC may
			//    contain more nodes than just it).
			// 2. If the graph contains an SCC, within that SCC, we can construct many simple cycles
			//    by taking any of the paths from any of the nodes to itself.
			//
			// Hence, determining SCCs is an effective way of printing out portions of the graph with
			// offending cycles.
			//
			// We decided to list out SCCs rather than simple cycles because, in the worst case scenario,
			// the number of simple cycles in a graph grows factorially w.r.t the number of vertices.
			// This is because, in a K^n graph, our cycles would be at least all possible permutations of
			// those `n` nodes.
			let mut sccs = FxHashMap::<TypeId, Vec<Vec<_>>>::default();

			petgraph::algo::TarjanScc::new().run(&self.graph, |nodes| {
				let universe = self.graph[nodes[0]].universe;
				sccs.entry(universe).or_default().push(nodes.to_vec());
			});

			// TODO: Pretty-print this information.

			panic!("Failed to validate behavior graph: behaviors may be called in a cycle, which could cause borrow violations.");
		};

        // Working in topological order, we populate the set of all components which could possibly
        // be borrowed when a namespace is called.
        struct ValidationCx<'a> {
            ty_map: &'a FxHashMap<TypeId, &'static str>,
            potentially_borrowed: Vec<FxHashMap<TypeId, (Mutability, Vec<EdgeIndex>)>>,
            err_msg_or_empty: String,
        }

        impl<'a> ValidationCx<'a> {
            pub fn new(ty_map: &'a FxHashMap<TypeId, &'static str>, node_count: usize) -> Self {
                Self {
                    ty_map,
                    potentially_borrowed: (0..node_count).map(|_| FxHashMap::default()).collect(),
                    err_msg_or_empty: String::new(),
                }
            }

            pub fn validate_behavior(&mut self, node: NodeIndex, behavior: &Behavior) {
                let f = &mut self.err_msg_or_empty;
                let pbs = &self.potentially_borrowed[node.index()];

                for (&req_ty, &req_mut) in &behavior.borrows {
                    // If the request is compatible with the PBS, ignore it.
                    let Some((pre_mut, pre_contrib)) = pbs.get(&req_ty) else { continue };

                    if pre_mut.is_compatible_with(req_mut) {
                        return;
                    }

                    // Otherwise, log out the error chain.
                    // TODO: Pretty-print the chain of borrows.
                    write!(
                        f,
                        "Behavior {} ({}) borrows component {} {} even though it may have already been borrowed {}.",
                        behavior.def_info.name,
						behavior.def_info.path,
						self.ty_map[&req_ty],
						req_mut.adjective(),
						pre_mut.adjective(),
                    )
                    .unwrap();
                }
            }

            pub fn extend_borrows(
                &mut self,
                calling_edge: EdgeIndex,
                calling_bhv: &Behavior,
                callee: NodeIndex,
            ) {
                let callee_pbs = &mut self.potentially_borrowed[callee.index()];

                for (&req_ty, &req_mut) in &calling_bhv.borrows {
                    match callee_pbs.entry(req_ty) {
                        std::collections::hash_map::Entry::Occupied(entry) => {
                            let (pbs_mut, pbs_requesters) = entry.into_mut();
                            *pbs_mut = pbs_mut.strictest(req_mut);
                            pbs_requesters.push(calling_edge);
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert((req_mut, vec![calling_edge]));
                        }
                    }
                }
            }
        }

        let mut cx = ValidationCx::new(&self.component_names, self.graph.node_count());

        for src_idx in topos {
            let src = &self.graph[src_idx];

            // For every terminal behavior, check it against the PBS.
            for terminal in &src.terminal_behaviors {
                cx.validate_behavior(src_idx, terminal);
            }

            // For every non-terminal behavior, check it against the PBS and then extend the future
            // nodes.
            for edge in self.graph.edges(src_idx) {
                let edge_bhv = &self.graph[edge.id()];
                cx.validate_behavior(src_idx, edge_bhv);
                cx.extend_borrows(edge.id(), edge_bhv, edge.target());
            }
        }

        // If we had any errors while validating this graph
        if !cx.err_msg_or_empty.is_empty() {
            panic!(
                "Failed to validate behavior graph:\n\n{}",
                cx.err_msg_or_empty
            );
        }

        // Otherwise, the graph is fully valid.
    }
}
