use std::collections::{HashMap, HashSet};

use crate::lexer::Span;
use crate::parser::{
    BinaryOperator, Expression, Program, ScheduleItem, Statement, SystemParamKind,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckError {
    pub span: Span,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Type {
    I32,
}

pub fn check_program(program: &Program) -> Result<(), CheckError> {
    let mut bindings = HashMap::new();

    check_schedules(program)?;
    check_system_params(program)?;

    if let Some(startup) = &program.startup {
        for statement in &startup.statements {
            check_statement(statement, &mut bindings)?;
        }
    }

    Ok(())
}

fn check_schedules(program: &Program) -> Result<(), CheckError> {
    let systems = program
        .systems
        .iter()
        .map(|system| system.name.as_str())
        .collect::<HashSet<_>>();

    for schedule in &program.schedules {
        for item in &schedule.items {
            match item {
                ScheduleItem::Run {
                    system_name,
                    system_span,
                } => {
                    if !systems.contains(system_name.as_str()) {
                        return Err(CheckError {
                            span: *system_span,
                            message: format!("unknown system `{system_name}` in schedule"),
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

fn check_system_params(program: &Program) -> Result<(), CheckError> {
    let resources = program
        .resources
        .iter()
        .map(|resource| resource.name.as_str())
        .collect::<HashSet<_>>();

    for system in &program.systems {
        for param in &system.params {
            match &param.kind {
                SystemParamKind::ReadResource {
                    resource_name,
                    resource_span,
                } => {
                    if !resources.contains(resource_name.as_str()) {
                        return Err(CheckError {
                            span: *resource_span,
                            message: format!(
                                "unknown resource `{resource_name}` in system parameter"
                            ),
                        });
                    }
                }
                SystemParamKind::Query { .. } => {}
            }
        }
    }

    Ok(())
}

fn check_statement(
    statement: &Statement,
    bindings: &mut HashMap<String, Type>,
) -> Result<(), CheckError> {
    match statement {
        Statement::Let(let_statement) => {
            let initializer_type = check_expression(&let_statement.initializer, bindings)?;

            if initializer_type == Type::I32 && let_statement.type_name.name != "i32" {
                return Err(CheckError {
                    span: let_statement.type_name.span,
                    message: "expected i32 binding for arithmetic expression".to_string(),
                });
            }

            bindings.insert(let_statement.name.clone(), initializer_type);
            Ok(())
        }
        Statement::Exit(exit) => {
            check_expression(&exit.expression, bindings)?;
            Ok(())
        }
        Statement::Run(_) => Err(CheckError {
            span: crate::lexer::Span { start: 0, end: 0 },
            message: "startup run checking is not implemented yet".to_string(),
        }),
        Statement::Spawn(_) => Err(CheckError {
            span: crate::lexer::Span { start: 0, end: 0 },
            message: "spawn checking is not implemented yet".to_string(),
        }),
        Statement::Resource(_) => Err(CheckError {
            span: crate::lexer::Span { start: 0, end: 0 },
            message: "resource checking is not implemented yet".to_string(),
        }),
    }
}

fn check_expression(
    expression: &Expression,
    bindings: &HashMap<String, Type>,
) -> Result<Type, CheckError> {
    match expression {
        Expression::Integer(_) => Ok(Type::I32),
        Expression::Identifier { name, span } => match bindings.get(name) {
            Some(binding_type) => Ok(*binding_type),
            None => Err(CheckError {
                span: *span,
                message: format!("unknown local variable `{name}`"),
            }),
        },
        Expression::FieldAccess { field_span, .. } => Err(CheckError {
            span: *field_span,
            message: "field access checking is not implemented yet".to_string(),
        }),
        Expression::Binary(binary) => {
            match binary.operator {
                BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::Multiply => {}
            }

            let left_type = check_expression(&binary.left, bindings)?;
            let right_type = check_expression(&binary.right, bindings)?;

            if left_type == Type::I32 && right_type == Type::I32 {
                Ok(Type::I32)
            } else {
                Err(CheckError {
                    span: expression_span(expression),
                    message: "expected i32 operands for arithmetic expression".to_string(),
                })
            }
        }
    }
}

fn expression_span(expression: &Expression) -> Span {
    match expression {
        Expression::Integer(integer) => integer.span,
        Expression::Identifier { span, .. } => *span,
        Expression::FieldAccess { target, .. } => expression_span(target),
        Expression::Binary(binary) => expression_span(&binary.left),
    }
}
