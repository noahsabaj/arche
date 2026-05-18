use std::collections::HashMap;
use std::fmt::Write;

use crate::parser::{BinaryOperator, Expression, Program, Statement};

pub fn emit_machine(program: &Program) -> String {
    let mut emitter = MachineEmitter::new();
    emitter.emit_program(program);
    emitter.output
}

struct MachineEmitter {
    output: String,
    locals: HashMap<String, usize>,
    next_slot: usize,
    next_temp: usize,
}

impl MachineEmitter {
    fn new() -> Self {
        Self {
            output: String::new(),
            locals: HashMap::new(),
            next_slot: 0,
            next_temp: 0,
        }
    }

    fn emit_program(&mut self, program: &Program) {
        self.output.push_str("function startup");

        let Some(startup) = &program.startup else {
            return;
        };

        for statement in &startup.statements {
            if let Statement::Let(let_statement) = statement {
                let slot = self.next_slot;
                self.next_slot += 1;
                self.locals.insert(let_statement.name.clone(), slot);
                let _ = write!(
                    self.output,
                    "\n  local {}: {} slot {}",
                    let_statement.name, let_statement.type_name.name, slot
                );
            }
        }

        for statement in &startup.statements {
            match statement {
                Statement::Let(let_statement) => {
                    let value = self.emit_expression(&let_statement.initializer);
                    let slot = self.locals[&let_statement.name];
                    let _ = write!(self.output, "\n  store slot {slot}, {value}");
                }
                Statement::Run(_) => {
                    let _ = write!(self.output, "\n  run unsupported");
                }
                Statement::Spawn(_) => {
                    let _ = write!(self.output, "\n  spawn unsupported");
                }
                Statement::Resource(_) => {
                    let _ = write!(self.output, "\n  resource unsupported");
                }
                Statement::Exit(exit) => {
                    let value = self.emit_expression(&exit.expression);
                    let _ = write!(self.output, "\n  exit {value}");
                }
            }
        }
    }

    fn emit_expression(&mut self, expression: &Expression) -> String {
        match expression {
            Expression::Integer(integer) => {
                let temp = self.allocate_temp();
                let _ = write!(self.output, "\n  {temp} = i32.const {}", integer.value);
                temp
            }
            Expression::Identifier { name, .. } => {
                let slot = self.locals[name];
                let temp = self.allocate_temp();
                let _ = write!(self.output, "\n  {temp} = load slot {slot}");
                temp
            }
            Expression::FieldAccess { .. } => {
                let temp = self.allocate_temp();
                let _ = write!(self.output, "\n  {temp} = unsupported.field");
                temp
            }
            Expression::Binary(binary) => {
                let left = self.emit_expression(&binary.left);
                let right = self.emit_expression(&binary.right);
                let temp = self.allocate_temp();
                match binary.operator {
                    BinaryOperator::Add => {
                        let _ = write!(self.output, "\n  {temp} = i32.add {left}, {right}");
                    }
                    BinaryOperator::Subtract => {
                        let _ = write!(self.output, "\n  {temp} = i32.sub {left}, {right}");
                    }
                    BinaryOperator::Multiply => {
                        let _ = write!(self.output, "\n  {temp} = i32.mul {left}, {right}");
                    }
                }
                temp
            }
        }
    }

    fn allocate_temp(&mut self) -> String {
        let temp = format!("%{}", self.next_temp);
        self.next_temp += 1;
        temp
    }
}
