//! Recursive effect fixed point analysis and exception propagation for M27-C4.

use std::collections::{BTreeMap, BTreeSet};

use arche_frontend::{Span, SymbolicType};

/// An effect set capturing capability requirements and exception throws.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EffectSummary {
    pub requires: BTreeSet<String>,
    pub throws: BTreeSet<SymbolicType>,
}

impl EffectSummary {
    /// Creates an empty effect summary.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Unions another effect summary into this one.
    pub fn union_with(&mut self, other: &Self) {
        for req in &other.requires {
            self.requires.insert(req.clone());
        }
        for thr in &other.throws {
            self.throws.insert(thr.clone());
        }
    }

    /// Eliminates caught exception types from the throws set.
    pub fn handle_exceptions(&mut self, caught: &[SymbolicType]) {
        self.throws.retain(|t| !caught.contains(t));
    }
}

/// An edge in the function effect dependency graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectCallEdge {
    pub caller: String,
    pub callee: String,
    pub caught_exceptions: Vec<SymbolicType>,
    pub span: Option<Span>,
}

/// Computes the least fixed point for `requires` and `throws` effects across a call graph.
pub fn compute_effect_fixed_points(
    initial_effects: &BTreeMap<String, EffectSummary>,
    edges: &[EffectCallEdge],
) -> BTreeMap<String, EffectSummary> {
    let mut summaries = initial_effects.clone();
    let mut changed = true;

    // Run fixpoint iteration until effect sets stabilize
    while changed {
        changed = false;
        for edge in edges {
            let callee_summary = summaries.get(&edge.callee).cloned().unwrap_or_default();
            let mut propagated = callee_summary;
            propagated.handle_exceptions(&edge.caught_exceptions);

            let caller_summary = summaries.entry(edge.caller.clone()).or_default();
            let prev_requires_len = caller_summary.requires.len();
            let prev_throws_len = caller_summary.throws.len();

            caller_summary.union_with(&propagated);

            if caller_summary.requires.len() != prev_requires_len
                || caller_summary.throws.len() != prev_throws_len
            {
                changed = true;
            }
        }
    }

    summaries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_recursive_effect_propagation() {
        let mut initial = BTreeMap::new();
        let mut leaf_effects = EffectSummary::empty();
        leaf_effects.requires.insert("Network".into());
        leaf_effects.throws.insert(SymbolicType::I32);
        initial.insert("leaf_fn".into(), leaf_effects);
        initial.insert("caller_fn".into(), EffectSummary::empty());

        let edges = vec![EffectCallEdge {
            caller: "caller_fn".into(),
            callee: "leaf_fn".into(),
            caught_exceptions: Vec::new(),
            span: None,
        }];

        let result = compute_effect_fixed_points(&initial, &edges);
        let caller = result.get("caller_fn").unwrap();
        assert!(caller.requires.contains("Network"));
        assert!(caller.throws.contains(&SymbolicType::I32));
    }

    #[test]
    fn catch_block_eliminates_caught_exceptions() {
        let mut initial = BTreeMap::new();
        let mut leaf_effects = EffectSummary::empty();
        leaf_effects.throws.insert(SymbolicType::I32);
        leaf_effects.throws.insert(SymbolicType::Bool);
        initial.insert("leaf_fn".into(), leaf_effects);
        initial.insert("caller_fn".into(), EffectSummary::empty());

        let edges = vec![EffectCallEdge {
            caller: "caller_fn".into(),
            callee: "leaf_fn".into(),
            caught_exceptions: vec![SymbolicType::I32],
            span: None,
        }];

        let result = compute_effect_fixed_points(&initial, &edges);
        let caller = result.get("caller_fn").unwrap();
        assert!(!caller.throws.contains(&SymbolicType::I32));
        assert!(caller.throws.contains(&SymbolicType::Bool));
    }

    #[test]
    fn mutual_recursion_reaches_least_fixed_point() {
        let mut initial = BTreeMap::new();
        let mut fn_a_effects = EffectSummary::empty();
        fn_a_effects.requires.insert("IO".into());
        initial.insert("fn_a".into(), fn_a_effects);

        let mut fn_b_effects = EffectSummary::empty();
        fn_b_effects.throws.insert(SymbolicType::Char);
        initial.insert("fn_b".into(), fn_b_effects);

        let edges = vec![
            EffectCallEdge {
                caller: "fn_a".into(),
                callee: "fn_b".into(),
                caught_exceptions: Vec::new(),
                span: None,
            },
            EffectCallEdge {
                caller: "fn_b".into(),
                callee: "fn_a".into(),
                caught_exceptions: Vec::new(),
                span: None,
            },
        ];

        let result = compute_effect_fixed_points(&initial, &edges);
        let a = result.get("fn_a").unwrap();
        let b = result.get("fn_b").unwrap();

        assert!(a.requires.contains("IO"));
        assert!(a.throws.contains(&SymbolicType::Char));
        assert!(b.requires.contains("IO"));
        assert!(b.throws.contains(&SymbolicType::Char));
    }
}
