use std::collections::HashMap;

use crate::lexer::Span;
use crate::parser::{BinaryOperator, Expression, Program, Statement};

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

    if let Some(startup) = &program.startup {
        for statement in &startup.statements {
            check_statement(statement, &mut bindings)?;
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
