use crate::ids::ModUniqueId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyEdgeType {
    Required,
    Optional,
    ContentPackFor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub source_id: ModUniqueId,
    pub target_id: ModUniqueId,
    pub minimum_version: Option<String>,
    pub edge_type: DependencyEdgeType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyNode {
    pub unique_id: ModUniqueId,
    pub version: String,
    pub name: String,
    pub enabled: bool,
}

/// Canonical profile component dependency graph.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub nodes: HashMap<ModUniqueId, DependencyNode>,
    pub edges: Vec<DependencyEdge>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: DependencyNode) {
        self.nodes.insert(node.unique_id.clone(), node);
    }

    pub fn add_edge(&mut self, edge: DependencyEdge) {
        self.edges.push(edge);
    }

    pub fn get_node(&self, id: &ModUniqueId) -> Option<&DependencyNode> {
        self.nodes.get(id)
    }

    pub fn outgoing_edges<'a>(
        &'a self,
        source: &'a ModUniqueId,
    ) -> impl Iterator<Item = &'a DependencyEdge> {
        self.edges.iter().filter(move |e| &e.source_id == source)
    }

    pub fn incoming_edges<'a>(
        &'a self,
        target: &'a ModUniqueId,
    ) -> impl Iterator<Item = &'a DependencyEdge> {
        self.edges.iter().filter(move |e| &e.target_id == target)
    }

    pub fn reverse_dependents(&self, target: &ModUniqueId) -> HashSet<ModUniqueId> {
        let mut dependents = HashSet::new();
        let mut queue = vec![target.clone()];
        while let Some(current) = queue.pop() {
            for edge in self.incoming_edges(&current) {
                if edge.edge_type != DependencyEdgeType::Optional
                    && dependents.insert(edge.source_id.clone())
                {
                    queue.push(edge.source_id.clone());
                }
            }
        }
        dependents
    }

    pub fn detect_cycles(&self) -> Vec<Vec<ModUniqueId>> {
        let mut cycles = Vec::new();
        let mut visited = HashSet::new();
        let mut rec_stack = Vec::new();

        for node_id in self.nodes.keys() {
            if !visited.contains(node_id) {
                self.cycle_dfs(node_id, &mut visited, &mut rec_stack, &mut cycles);
            }
        }
        cycles
    }

    fn cycle_dfs(
        &self,
        node: &ModUniqueId,
        visited: &mut HashSet<ModUniqueId>,
        rec_stack: &mut Vec<ModUniqueId>,
        cycles: &mut Vec<Vec<ModUniqueId>>,
    ) {
        visited.insert(node.clone());
        rec_stack.push(node.clone());

        for edge in self.outgoing_edges(node) {
            if edge.edge_type == DependencyEdgeType::Optional {
                continue;
            }
            if let Some(pos) = rec_stack.iter().position(|id| id == &edge.target_id) {
                let mut cycle = rec_stack[pos..].to_vec();
                cycle.push(edge.target_id.clone());
                cycles.push(cycle);
            } else if !visited.contains(&edge.target_id) {
                self.cycle_dfs(&edge.target_id, visited, rec_stack, cycles);
            }
        }

        rec_stack.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dependency_graph_cycle_detection() {
        let mut graph = DependencyGraph::new();
        let a = ModUniqueId::new("ModA");
        let b = ModUniqueId::new("ModB");
        let c = ModUniqueId::new("ModC");

        graph.add_node(DependencyNode {
            unique_id: a.clone(),
            version: "1.0.0".into(),
            name: "Mod A".into(),
            enabled: true,
        });
        graph.add_node(DependencyNode {
            unique_id: b.clone(),
            version: "1.0.0".into(),
            name: "Mod B".into(),
            enabled: true,
        });
        graph.add_node(DependencyNode {
            unique_id: c.clone(),
            version: "1.0.0".into(),
            name: "Mod C".into(),
            enabled: true,
        });

        graph.add_edge(DependencyEdge {
            source_id: a.clone(),
            target_id: b.clone(),
            minimum_version: None,
            edge_type: DependencyEdgeType::Required,
        });
        graph.add_edge(DependencyEdge {
            source_id: b.clone(),
            target_id: c.clone(),
            minimum_version: None,
            edge_type: DependencyEdgeType::Required,
        });

        assert!(graph.detect_cycles().is_empty());

        // Introduce cycle C -> A
        graph.add_edge(DependencyEdge {
            source_id: c.clone(),
            target_id: a.clone(),
            minimum_version: None,
            edge_type: DependencyEdgeType::Required,
        });

        let cycles = graph.detect_cycles();
        assert!(!cycles.is_empty());
    }

    #[test]
    fn test_reverse_dependents() {
        let mut graph = DependencyGraph::new();
        let framework = ModUniqueId::new("FrameworkMod");
        let content_pack = ModUniqueId::new("ContentPackMod");

        graph.add_edge(DependencyEdge {
            source_id: content_pack.clone(),
            target_id: framework.clone(),
            minimum_version: None,
            edge_type: DependencyEdgeType::ContentPackFor,
        });

        let rev = graph.reverse_dependents(&framework);
        assert_eq!(rev.len(), 1);
        assert!(rev.contains(&content_pack));
    }
}
