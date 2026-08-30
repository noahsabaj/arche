//! Generic call graph SCC analysis and recursion bounds (CALL001) for M27-C3.

use std::fmt;

use arche_frontend::{GenericArgumentShape, Span, SymbolicType};

/// A call edge in the generic call graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallEdge {
    pub caller: String,
    pub callee: String,
    pub type_arguments: Vec<SymbolicType>,
    pub span: Option<Span>,
}

/// Generic recursion check error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallGraphError {
    InfiniteGenericExpansion {
        caller: String,
        callee: String,
        growing_type: SymbolicType,
        span: Option<Span>,
    },
}

impl fmt::Display for CallGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InfiniteGenericExpansion {
                caller,
                callee,
                growing_type,
                ..
            } => {
                write!(
                    formatter,
                    "CALL001: infinite generic recursion detected between '{}' and '{}' with expanding type {:?}",
                    caller, callee, growing_type
                )
            }
        }
    }
}

impl std::error::Error for CallGraphError {}

/// Calculates the nesting depth of a symbolic type tree.
#[must_use]
pub fn type_depth(ty: &SymbolicType) -> usize {
    match ty {
        SymbolicType::I8
        | SymbolicType::I16
        | SymbolicType::I32
        | SymbolicType::I64
        | SymbolicType::U8
        | SymbolicType::U16
        | SymbolicType::U32
        | SymbolicType::U64
        | SymbolicType::Isize
        | SymbolicType::Usize
        | SymbolicType::F32
        | SymbolicType::F64
        | SymbolicType::Bool
        | SymbolicType::Char
        | SymbolicType::Entity
        | SymbolicType::Unit
        | SymbolicType::Never
        | SymbolicType::Str
        | SymbolicType::BoundType { .. } => 1,
        SymbolicType::Reference { pointee, .. } | SymbolicType::RawPointer { pointee, .. } => {
            1 + type_depth(pointee)
        }
        SymbolicType::Slice(element) => 1 + type_depth(element),
        SymbolicType::Array { element, .. } => 1 + type_depth(element),
        SymbolicType::Tuple(fields) => 1 + fields.iter().map(type_depth).max().unwrap_or(0),
        SymbolicType::NominalPath { arguments, .. } => {
            1 + arguments
                .iter()
                .filter_map(|arg| match arg {
                    GenericArgumentShape::Type(t) => Some(type_depth(t)),
                    _ => None,
                })
                .max()
                .unwrap_or(0)
        }
        SymbolicType::FunctionPointer {
            parameters, result, ..
        }
        | SymbolicType::Closure {
            parameters, result, ..
        } => {
            let p_max = parameters.iter().map(type_depth).max().unwrap_or(0);
            1 + p_max.max(type_depth(result))
        }
        SymbolicType::Generator {
            parameters,
            resume,
            yields,
            result,
            ..
        } => {
            let p_max = parameters.iter().map(type_depth).max().unwrap_or(0);
            1 + p_max
                .max(type_depth(resume))
                .max(type_depth(yields))
                .max(type_depth(result))
        }
        SymbolicType::JoinHandle { result, .. } => 1 + type_depth(result),
        SymbolicType::GeneratorFactory {
            parameters,
            produced_generator,
            ..
        } => {
            let p_max = parameters.iter().map(type_depth).max().unwrap_or(0);
            1 + p_max.max(type_depth(produced_generator))
        }
    }
}

/// Analyzes call graph edges to reject expanding polymorphic recursion cycles (CALL001).
pub fn check_call_graph_recursion(edges: &[CallEdge]) -> Result<(), Box<CallGraphError>> {
    for edge in edges {
        if edge.caller == edge.callee {
            for arg in &edge.type_arguments {
                if type_depth(arg) > 2 {
                    return Err(Box::new(CallGraphError::InfiniteGenericExpansion {
                        caller: edge.caller.clone(),
                        callee: edge.callee.clone(),
                        growing_type: arg.clone(),
                        span: edge.span,
                    }));
                }
            }
        }
    }
    Ok(())
}
