//! Canonical C2 golden printers (design doc 5082-5098).
//!
//! Test/debug contracts, not public artifact formats: one UTF-8/LF
//! S-expression envelope per text, `ARCHE-TYPE-TEXT 1` for the checked-type
//! universe and `ARCHE-TRAIT-TEXT 1` for trait tables, keys, and impl
//! descriptors. Node and field names are lowercase ASCII with hyphens,
//! integers are minimal decimal, identity bytes are uppercase hex, strings
//! are JSON-quoted, and nothing host-dependent is ever printed.

use std::fmt::Write as _;

use arche_frontend::{
    DeclarationKind, GenericArgumentShape, GenericParameterKind, IntegerType, Mutability,
    SemanticDeclarationPath, SymbolicConstExpression, SymbolicConstNode,
    SymbolicDeclarationPayloadSkeleton, SymbolicLifetime, SymbolicType, TargetRoot,
};

use crate::body_check::{CheckedBodyCallee, CheckedBodyPatternAnalysis};
use crate::coercion::{CheckedCoercion, CoercionKind};
use crate::literal::FloatType;
use crate::model::{C2CheckedWorkspace, C2Resolution, C2TypeProducer};
use crate::pattern::FloatType as PatternFloatType;
use crate::pattern::IntegerType as PatternIntegerType;
use crate::pattern::{
    ArmReachability, BindingMode, DecisionTree, IrrefutablePatternAnalysis, OwnershipFactKind,
    PatternBindingFact, PatternConst, PatternLiteral, PatternMatchAnalysis, PatternProjection,
    PatternTest, PatternType, PendingPatternTest, ReferenceMutability, SequenceLengthConstraint,
    TypedBinding, TypedPattern, TypedPatternArm, TypedPatternKind, TypedRangeEndpoint,
};
use crate::sealed::{PrimitiveDomain, PrimitiveOperatorTrait, SealedPrimitiveOperator};
use crate::typing::{
    BinaryTypeOperator, CheckedExpression, CheckedExpressionKind, CheckedPrimitiveSelection,
    UnaryTypeOperator,
};
use crate::PendingC4Dependencies;
use arche_frontend::{
    CallTrait, CaptureMode, GeneratorTarget, SemanticBodyKind, Span, SymbolicCapture,
};

/// A shape the fixed envelope refuses to print: a pending (unresolved)
/// skeleton leaf, which cannot occur in a fully checked workspace, or a
/// non-UTF-8 package scope, which has no canonical string spelling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoldenEncodeError {
    UnprintableType,
    NonUtf8Scope,
}

struct Printer {
    output: String,
}

impl Printer {
    fn new(header: &str) -> Self {
        let mut output = String::new();
        output.push_str(header);
        output.push('\n');
        Self { output }
    }

    fn finish(mut self) -> String {
        self.output.push('\n');
        self.output
    }

    fn form(
        &mut self,
        name: &str,
        body: impl FnOnce(&mut Self) -> Result<(), GoldenEncodeError>,
    ) -> Result<(), GoldenEncodeError> {
        self.output.push('(');
        self.output.push_str(name);
        body(self)?;
        self.output.push(')');
        Ok(())
    }

    fn field(
        &mut self,
        name: &str,
        body: impl FnOnce(&mut Self) -> Result<(), GoldenEncodeError>,
    ) -> Result<(), GoldenEncodeError> {
        self.output.push(' ');
        self.form(name, body)
    }

    fn atom(&mut self, value: &str) {
        self.output.push(' ');
        self.output.push_str(value);
    }

    fn boolean(&mut self, value: bool) {
        self.atom(if value { "true" } else { "false" });
    }

    fn unsigned(&mut self, value: u64) {
        self.output.push(' ');
        let _ = write!(self.output, "{value}");
    }

    fn hex(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.output.push(' ');
        for byte in bytes {
            let _ = write!(self.output, "{byte:02X}");
        }
    }

    fn string(&mut self, value: &str) {
        self.output.push(' ');
        self.output.push('"');
        for character in value.chars() {
            match character {
                '"' => self.output.push_str("\\\""),
                '\\' => self.output.push_str("\\\\"),
                '\u{0008}' => self.output.push_str("\\b"),
                '\u{000c}' => self.output.push_str("\\f"),
                '\n' => self.output.push_str("\\n"),
                '\r' => self.output.push_str("\\r"),
                '\t' => self.output.push_str("\\t"),
                control if control <= '\u{001f}' => {
                    let _ = write!(self.output, "\\u{:04X}", u32::from(control));
                }
                other => self.output.push(other),
            }
        }
        self.output.push('"');
    }

    fn resolution(&mut self, resolution: &C2Resolution) -> Result<(), GoldenEncodeError> {
        self.field("resolution", |printer| match resolution {
            C2Resolution::Complete => {
                printer.atom("complete");
                Ok(())
            }
            C2Resolution::NeedsCtfe(obligations) => printer.field("needs-ctfe", |printer| {
                for obligation in obligations.as_slice() {
                    printer.hex(obligation.canonical_bytes());
                }
                Ok(())
            }),
        })
    }

    fn pending_c4(&mut self, pending: &PendingC4Dependencies) -> Result<(), GoldenEncodeError> {
        self.field("pending-c4", |printer| {
            for dependency in pending.as_slice() {
                printer.hex(dependency.canonical_bytes());
            }
            Ok(())
        })
    }

    fn lifetime(&mut self, lifetime: &SymbolicLifetime) -> Result<(), GoldenEncodeError> {
        match lifetime {
            SymbolicLifetime::Static => {
                self.atom("static");
                Ok(())
            }
            SymbolicLifetime::ErasedLocal => {
                self.atom("erased-local");
                Ok(())
            }
            SymbolicLifetime::Bound { depth, index } => self.field("bound-lifetime", |printer| {
                printer.unsigned(*depth);
                printer.unsigned(*index);
                Ok(())
            }),
        }
    }

    fn const_expression(
        &mut self,
        expression: &SymbolicConstExpression,
    ) -> Result<(), GoldenEncodeError> {
        let integer = integer_type_atom(expression.integer_type);
        self.field("const", |printer| {
            printer.atom(integer);
            printer.const_node(&expression.node)
        })
    }

    fn const_node(&mut self, node: &SymbolicConstNode) -> Result<(), GoldenEncodeError> {
        let binary = |printer: &mut Self,
                      name: &str,
                      left: &SymbolicConstExpression,
                      right: &SymbolicConstExpression| {
            printer.field(name, |printer| {
                printer.const_expression(left)?;
                printer.const_expression(right)
            })
        };
        match node {
            SymbolicConstNode::IntegerLiteral(bytes) => self.field("literal", |printer| {
                printer.hex(bytes);
                Ok(())
            }),
            SymbolicConstNode::Bound { depth, index } => self.field("bound-const", |printer| {
                printer.unsigned(*depth);
                printer.unsigned(*index);
                Ok(())
            }),
            SymbolicConstNode::ConstDefinitionPath(path) => {
                self.field("const-path", |printer| printer.declaration_path(path))
            }
            SymbolicConstNode::WrappingNeg(inner) => {
                self.field("wrapping-neg", |printer| printer.const_expression(inner))
            }
            SymbolicConstNode::BitNot(inner) => {
                self.field("bit-not", |printer| printer.const_expression(inner))
            }
            SymbolicConstNode::WrappingMul(left, right) => {
                binary(self, "wrapping-mul", left, right)
            }
            SymbolicConstNode::IntegerDivide(left, right) => {
                binary(self, "integer-divide", left, right)
            }
            SymbolicConstNode::IntegerRemainder(left, right) => {
                binary(self, "integer-remainder", left, right)
            }
            SymbolicConstNode::WrappingAdd(left, right) => {
                binary(self, "wrapping-add", left, right)
            }
            SymbolicConstNode::WrappingSub(left, right) => {
                binary(self, "wrapping-sub", left, right)
            }
            SymbolicConstNode::MaskedShiftLeft(left, right) => {
                binary(self, "masked-shift-left", left, right)
            }
            SymbolicConstNode::MaskedShiftRight(left, right) => {
                binary(self, "masked-shift-right", left, right)
            }
            SymbolicConstNode::BitAnd(left, right) => binary(self, "bit-and", left, right),
            SymbolicConstNode::BitXor(left, right) => binary(self, "bit-xor", left, right),
            SymbolicConstNode::BitOr(left, right) => binary(self, "bit-or", left, right),
        }
    }

    fn declaration_path(
        &mut self,
        path: &SemanticDeclarationPath,
    ) -> Result<(), GoldenEncodeError> {
        self.field("path", |printer| {
            printer.string(&path.registry_origin);
            printer.string(&path.package_name);
            match &path.target {
                TargetRoot::Library => printer.atom("library"),
                TargetRoot::Binary(name) => {
                    printer.field("binary", |printer| {
                        printer.string(name);
                        Ok(())
                    })?;
                }
                TargetRoot::Environment(name) => {
                    printer.field("environment", |printer| {
                        printer.string(name);
                        Ok(())
                    })?;
                }
            }
            for module in &path.modules {
                printer.string(module);
            }
            printer.atom(declaration_kind_atom(path.kind));
            printer.string(&path.name);
            Ok(())
        })
    }

    fn symbolic_type(&mut self, ty: &SymbolicType) -> Result<(), GoldenEncodeError> {
        match ty {
            SymbolicType::I8 => {
                self.atom("i8");
                Ok(())
            }
            SymbolicType::I16 => {
                self.atom("i16");
                Ok(())
            }
            SymbolicType::I32 => {
                self.atom("i32");
                Ok(())
            }
            SymbolicType::I64 => {
                self.atom("i64");
                Ok(())
            }
            SymbolicType::U8 => {
                self.atom("u8");
                Ok(())
            }
            SymbolicType::U16 => {
                self.atom("u16");
                Ok(())
            }
            SymbolicType::U32 => {
                self.atom("u32");
                Ok(())
            }
            SymbolicType::U64 => {
                self.atom("u64");
                Ok(())
            }
            SymbolicType::Isize => {
                self.atom("isize");
                Ok(())
            }
            SymbolicType::Usize => {
                self.atom("usize");
                Ok(())
            }
            SymbolicType::F32 => {
                self.atom("f32");
                Ok(())
            }
            SymbolicType::F64 => {
                self.atom("f64");
                Ok(())
            }
            SymbolicType::Bool => {
                self.atom("bool");
                Ok(())
            }
            SymbolicType::Char => {
                self.atom("char");
                Ok(())
            }
            SymbolicType::Entity => {
                self.atom("entity");
                Ok(())
            }
            SymbolicType::Unit => {
                self.atom("unit");
                Ok(())
            }
            SymbolicType::Never => {
                self.atom("never");
                Ok(())
            }
            SymbolicType::Str => {
                self.atom("str");
                Ok(())
            }
            SymbolicType::BoundType { depth, index } => self.field("bound-type", |printer| {
                printer.unsigned(*depth);
                printer.unsigned(*index);
                Ok(())
            }),
            SymbolicType::Slice(element) => {
                self.field("slice", |printer| printer.symbolic_type(element))
            }
            SymbolicType::Array { element, length } => self.field("array", |printer| {
                printer.symbolic_type(element)?;
                printer.const_expression(length)
            }),
            SymbolicType::Tuple(fields) => self.field("tuple", |printer| {
                for field in fields {
                    printer.symbolic_type(field)?;
                }
                Ok(())
            }),
            SymbolicType::Reference {
                mutability,
                lifetime,
                pointee,
            } => self.field("reference", |printer| {
                printer.atom(mutability_atom(*mutability));
                printer.lifetime(lifetime)?;
                printer.symbolic_type(pointee)
            }),
            SymbolicType::RawPointer {
                mutability,
                pointee,
            } => self.field("raw-pointer", |printer| {
                printer.atom(mutability_atom(*mutability));
                printer.symbolic_type(pointee)
            }),
            SymbolicType::NominalPath {
                declaration,
                arguments,
            } => self.field("nominal", |printer| {
                printer.declaration_path(declaration)?;
                printer.field("arguments", |printer| {
                    for argument in arguments {
                        printer.generic_argument(argument)?;
                    }
                    Ok(())
                })
            }),
            SymbolicType::FunctionPointer {
                unsafe_,
                parameters,
                result,
                requires,
                throws,
            } => self.field("fn-pointer", |printer| {
                printer.boolean(*unsafe_);
                printer.field("parameters", |printer| {
                    for parameter in parameters {
                        printer.symbolic_type(parameter)?;
                    }
                    Ok(())
                })?;
                printer.field("result", |printer| printer.symbolic_type(result))?;
                printer.field("requires", |printer| {
                    for member in requires.members() {
                        printer.symbolic_type(member)?;
                    }
                    Ok(())
                })?;
                printer.field("throws", |printer| {
                    for member in throws.members() {
                        printer.symbolic_type(member)?;
                    }
                    Ok(())
                })
            }),
            SymbolicType::JoinHandle { result, throws } => self.field("join-handle", |printer| {
                printer.field("result", |printer| printer.symbolic_type(result))?;
                printer.field("throws", |printer| {
                    for member in throws.members() {
                        printer.symbolic_type(member)?;
                    }
                    Ok(())
                })
            }),
            SymbolicType::Closure {
                owner,
                expression_ordinal,
                captures,
                parameters,
                result,
                requires,
                throws,
                arguments,
            } => self.field("closure", |printer| {
                printer.declaration_path(owner)?;
                printer.field("ordinal", |printer| {
                    printer.unsigned(*expression_ordinal);
                    Ok(())
                })?;
                printer.captures(captures)?;
                printer.field("parameters", |printer| {
                    for parameter in parameters {
                        printer.symbolic_type(parameter)?;
                    }
                    Ok(())
                })?;
                printer.field("result", |printer| printer.symbolic_type(result))?;
                printer.effect_members("requires", requires)?;
                printer.effect_members("throws", throws)?;
                printer.field("arguments", |printer| {
                    for argument in arguments {
                        printer.generic_argument(argument)?;
                    }
                    Ok(())
                })
            }),
            SymbolicType::Generator {
                target,
                captures,
                parameters,
                factory_unsafe,
                resume,
                yields,
                result,
                requires,
                throws,
            } => self.field("generator", |printer| {
                printer.generator_target(target)?;
                printer.captures(captures)?;
                printer.field("parameters", |printer| {
                    for parameter in parameters {
                        printer.symbolic_type(parameter)?;
                    }
                    Ok(())
                })?;
                printer.field("factory-unsafe", |printer| {
                    printer.boolean(*factory_unsafe);
                    Ok(())
                })?;
                printer.field("resume", |printer| printer.symbolic_type(resume))?;
                printer.field("yields", |printer| printer.symbolic_type(yields))?;
                printer.field("result", |printer| printer.symbolic_type(result))?;
                printer.effect_members("requires", requires)?;
                printer.effect_members("throws", throws)
            }),
            SymbolicType::GeneratorFactory {
                target,
                captures,
                call_trait,
                parameters,
                factory_unsafe,
                produced_generator,
            } => self.field("generator-factory", |printer| {
                printer.generator_target(target)?;
                printer.captures(captures)?;
                printer.field("call-trait", |printer| {
                    printer.atom(match call_trait {
                        CallTrait::Fn => "fn",
                        CallTrait::FnMut => "fn-mut",
                        CallTrait::FnOnce => "fn-once",
                    });
                    Ok(())
                })?;
                printer.field("parameters", |printer| {
                    for parameter in parameters {
                        printer.symbolic_type(parameter)?;
                    }
                    Ok(())
                })?;
                printer.field("factory-unsafe", |printer| {
                    printer.boolean(*factory_unsafe);
                    Ok(())
                })?;
                printer.field("produced-generator", |printer| {
                    printer.symbolic_type(produced_generator)
                })
            }),
        }
    }

    fn effect_members(
        &mut self,
        name: &str,
        set: &arche_frontend::SymbolicTypeEffectSet,
    ) -> Result<(), GoldenEncodeError> {
        self.field(name, |printer| {
            for member in set.members() {
                printer.symbolic_type(member)?;
            }
            Ok(())
        })
    }

    fn captures(&mut self, captures: &[SymbolicCapture]) -> Result<(), GoldenEncodeError> {
        self.field("captures", |printer| {
            for capture in captures {
                printer.field("capture", |printer| {
                    printer.unsigned(capture.ordinal);
                    printer.atom(match capture.mode {
                        CaptureMode::Shared => "shared",
                        CaptureMode::Mutable => "mutable",
                        CaptureMode::Move => "move",
                    });
                    printer.symbolic_type(&capture.ty)
                })?;
            }
            Ok(())
        })
    }

    fn pattern_analysis(
        &mut self,
        analysis: &CheckedBodyPatternAnalysis,
    ) -> Result<(), GoldenEncodeError> {
        match analysis {
            CheckedBodyPatternAnalysis::Irrefutable(IrrefutablePatternAnalysis::Complete(
                pattern,
            )) => self.field("irrefutable-complete", |printer| {
                printer.typed_pattern(pattern)
            }),
            CheckedBodyPatternAnalysis::Irrefutable(IrrefutablePatternAnalysis::NeedsCtfe {
                pattern,
                dependencies,
            }) => self.field("irrefutable-needs-ctfe", |printer| {
                printer.typed_pattern(pattern)?;
                printer.field("dependencies", |printer| {
                    for dependency in dependencies.iter() {
                        printer.pattern_const(dependency)?;
                    }
                    Ok(())
                })
            }),
            CheckedBodyPatternAnalysis::Refutable(PatternMatchAnalysis::Complete(complete)) => self
                .field("refutable-complete", |printer| {
                    printer.pattern_arms(complete.arms())?;
                    printer.field("reachability", |printer| {
                        for reachability in complete.reachability() {
                            printer.atom(match reachability {
                                ArmReachability::Reachable => "reachable",
                            });
                        }
                        Ok(())
                    })?;
                    printer.field("tree", |printer| printer.decision_tree(complete.tree()))
                }),
            CheckedBodyPatternAnalysis::Refutable(PatternMatchAnalysis::NeedsCtfe(pending)) => self
                .field("refutable-needs-ctfe", |printer| {
                    printer.pattern_arms(pending.arms())?;
                    printer.field("dependencies", |printer| {
                        for dependency in pending.dependencies() {
                            printer.pattern_const(dependency)?;
                        }
                        Ok(())
                    })?;
                    printer.field("tree", |printer| printer.decision_tree(pending.tree()))
                }),
        }
    }

    fn pattern_arms(&mut self, arms: &[TypedPatternArm]) -> Result<(), GoldenEncodeError> {
        self.field("arms", |printer| {
            for arm in arms {
                printer.field("arm", |printer| {
                    printer.boolean(arm.has_guard());
                    printer.typed_pattern(arm.pattern())
                })?;
            }
            Ok(())
        })
    }

    fn decision_tree(&mut self, tree: &DecisionTree) -> Result<(), GoldenEncodeError> {
        match tree {
            DecisionTree::Fail => {
                self.atom("fail");
                Ok(())
            }
            DecisionTree::Leaf {
                arm_index,
                bindings,
            } => self.field("leaf", |printer| {
                printer.unsigned(*arm_index as u64);
                printer.binding_facts(bindings)
            }),
            DecisionTree::Guard {
                arm_index,
                bindings,
                on_true,
                on_false,
            } => self.field("guard", |printer| {
                printer.unsigned(*arm_index as u64);
                printer.binding_facts(bindings)?;
                printer.field("on-true", |printer| printer.decision_tree(on_true))?;
                printer.field("on-false", |printer| printer.decision_tree(on_false))
            }),
            DecisionTree::Test {
                path,
                test,
                on_match,
                on_mismatch,
            } => self.field("test", |printer| {
                printer.projection_path(path)?;
                printer.pattern_test(test)?;
                printer.field("on-match", |printer| printer.decision_tree(on_match))?;
                printer.field("on-mismatch", |printer| printer.decision_tree(on_mismatch))
            }),
            DecisionTree::NeedsCtfe {
                path,
                test,
                on_match,
                on_mismatch,
            } => self.field("needs-ctfe", |printer| {
                printer.projection_path(path)?;
                printer.pending_pattern_test(test)?;
                printer.field("on-match", |printer| printer.decision_tree(on_match))?;
                printer.field("on-mismatch", |printer| printer.decision_tree(on_mismatch))
            }),
        }
    }

    fn binding_facts(&mut self, facts: &[PatternBindingFact]) -> Result<(), GoldenEncodeError> {
        self.field("bindings", |printer| {
            for fact in facts {
                printer.field("fact", |printer| {
                    printer.typed_binding(fact.binding())?;
                    printer.projection_path(fact.path())?;
                    printer.atom(match fact.ownership() {
                        OwnershipFactKind::Move => "move",
                        OwnershipFactKind::Ref => "ref",
                        OwnershipFactKind::RefMut => "ref-mut",
                    });
                    Ok(())
                })?;
            }
            Ok(())
        })
    }

    fn projection_path(&mut self, path: &[PatternProjection]) -> Result<(), GoldenEncodeError> {
        self.field("path", |printer| {
            for projection in path {
                match projection {
                    PatternProjection::InsertedDeref(mutability) => {
                        printer.field("inserted-deref", |printer| {
                            printer.atom(reference_mutability_atom(*mutability));
                            Ok(())
                        })?;
                    }
                    PatternProjection::ExplicitDeref(mutability) => {
                        printer.field("explicit-deref", |printer| {
                            printer.atom(reference_mutability_atom(*mutability));
                            Ok(())
                        })?;
                    }
                    PatternProjection::TupleField(index) => {
                        printer.field("tuple-field", |printer| {
                            printer.unsigned(*index as u64);
                            Ok(())
                        })?;
                    }
                    PatternProjection::ArrayElement(index) => {
                        printer.field("array-element", |printer| {
                            printer.unsigned(*index as u64);
                            Ok(())
                        })?;
                    }
                    PatternProjection::SliceElementFromStart(index) => {
                        printer.field("slice-from-start", |printer| {
                            printer.unsigned(*index as u64);
                            Ok(())
                        })?;
                    }
                    PatternProjection::SliceElementFromEnd(index) => {
                        printer.field("slice-from-end", |printer| {
                            printer.unsigned(*index as u64);
                            Ok(())
                        })?;
                    }
                    PatternProjection::RecordField {
                        record_name,
                        field_index,
                        field,
                    } => {
                        printer.field("record-field", |printer| {
                            printer.string(record_name);
                            printer.unsigned(*field_index as u64);
                            printer.string(field);
                            Ok(())
                        })?;
                    }
                    PatternProjection::EnumField {
                        variant_index,
                        field_index,
                    } => {
                        printer.field("enum-field", |printer| {
                            printer.unsigned(*variant_index as u64);
                            printer.unsigned(*field_index as u64);
                            Ok(())
                        })?;
                    }
                }
            }
            Ok(())
        })
    }

    fn pattern_test(&mut self, test: &PatternTest) -> Result<(), GoldenEncodeError> {
        self.field("check", |printer| match test {
            PatternTest::Bool(value) => printer.field("bool", |printer| {
                printer.boolean(*value);
                Ok(())
            }),
            PatternTest::SignedRange {
                start,
                end,
                inclusive,
            } => printer.field("signed-range", |printer| {
                printer.output.push(' ');
                let _ = write!(printer.output, "{start}");
                printer.output.push(' ');
                let _ = write!(printer.output, "{end}");
                printer.boolean(*inclusive);
                Ok(())
            }),
            PatternTest::UnsignedRange {
                start,
                end,
                inclusive,
            } => printer.field("unsigned-range", |printer| {
                printer.output.push(' ');
                let _ = write!(printer.output, "{start}");
                printer.output.push(' ');
                let _ = write!(printer.output, "{end}");
                printer.boolean(*inclusive);
                Ok(())
            }),
            PatternTest::CharRange {
                start,
                end,
                inclusive,
            } => printer.field("char-range", |printer| {
                let mut buffer = [0_u8; 4];
                printer.string(start.encode_utf8(&mut buffer));
                let mut buffer = [0_u8; 4];
                printer.string(end.encode_utf8(&mut buffer));
                printer.boolean(*inclusive);
                Ok(())
            }),
            PatternTest::String(value) => printer.field("string", |printer| {
                printer.string(value);
                Ok(())
            }),
            PatternTest::SliceLength(constraint) => printer.field("slice-length", |printer| {
                printer.length_constraint(*constraint);
                Ok(())
            }),
            PatternTest::EnumVariant {
                enum_name,
                variant_index,
                variant,
            } => printer.field("enum-variant", |printer| {
                printer.string(enum_name);
                printer.unsigned(*variant_index as u64);
                printer.string(variant);
                Ok(())
            }),
        })
    }

    fn pending_pattern_test(&mut self, test: &PendingPatternTest) -> Result<(), GoldenEncodeError> {
        self.field("pending-check", |printer| match test {
            PendingPatternTest::ConstEquals(dependency) => {
                printer.field("const-equals", |printer| printer.pattern_const(dependency))
            }
            PendingPatternTest::Range {
                ty,
                start,
                end,
                inclusive,
            } => printer.field("range", |printer| {
                printer.pattern_type(ty)?;
                printer.range_endpoint(start)?;
                printer.range_endpoint(end)?;
                printer.boolean(*inclusive);
                Ok(())
            }),
            PendingPatternTest::ArrayLength { length, constraint } => {
                printer.field("array-length", |printer| {
                    printer.pattern_const(length)?;
                    printer.length_constraint(*constraint);
                    Ok(())
                })
            }
        })
    }

    fn length_constraint(&mut self, constraint: SequenceLengthConstraint) {
        match constraint {
            SequenceLengthConstraint::Exact(length) => {
                self.output.push_str(" (exact");
                self.unsigned(length as u64);
                self.output.push(')');
            }
            SequenceLengthConstraint::AtLeast(length) => {
                self.output.push_str(" (at-least");
                self.unsigned(length as u64);
                self.output.push(')');
            }
        }
    }

    fn range_endpoint(&mut self, endpoint: &TypedRangeEndpoint) -> Result<(), GoldenEncodeError> {
        match endpoint {
            TypedRangeEndpoint::Literal(literal) => {
                self.field("literal", |printer| printer.pattern_literal(literal))
            }
            TypedRangeEndpoint::NeedsCtfe(dependency) => {
                self.field("needs-ctfe", |printer| printer.pattern_const(dependency))
            }
        }
    }

    fn pattern_literal(&mut self, literal: &PatternLiteral) -> Result<(), GoldenEncodeError> {
        match literal {
            PatternLiteral::Bool(value) => {
                self.boolean(*value);
                Ok(())
            }
            PatternLiteral::Signed(value) => {
                self.output.push(' ');
                let _ = write!(self.output, "{value}");
                Ok(())
            }
            PatternLiteral::Unsigned(value) => {
                self.output.push(' ');
                let _ = write!(self.output, "{value}");
                Ok(())
            }
            PatternLiteral::Char(value) => {
                let mut buffer = [0_u8; 4];
                self.string(value.encode_utf8(&mut buffer));
                Ok(())
            }
            PatternLiteral::String(value) => {
                self.string(value);
                Ok(())
            }
        }
    }

    fn pattern_const(&mut self, dependency: &PatternConst) -> Result<(), GoldenEncodeError> {
        self.field("const-dep", |printer| {
            printer.string(dependency.dependency());
            printer.pattern_type(dependency.ty())
        })
    }

    fn pattern_type(&mut self, ty: &PatternType) -> Result<(), GoldenEncodeError> {
        match ty {
            PatternType::Unit => {
                self.atom("unit");
                Ok(())
            }
            PatternType::Bool => {
                self.atom("bool");
                Ok(())
            }
            PatternType::Integer(integer) => match integer {
                PatternIntegerType::Signed(bits) => self.field("signed", |printer| {
                    printer.unsigned(u64::from(*bits));
                    Ok(())
                }),
                PatternIntegerType::Unsigned(bits) => self.field("unsigned", |printer| {
                    printer.unsigned(u64::from(*bits));
                    Ok(())
                }),
            },
            PatternType::Char => {
                self.atom("char");
                Ok(())
            }
            PatternType::String => {
                self.atom("string");
                Ok(())
            }
            PatternType::Str => {
                self.atom("str");
                Ok(())
            }
            PatternType::Tuple(fields) => self.field("tuple", |printer| {
                for field in fields.iter() {
                    printer.pattern_type(field)?;
                }
                Ok(())
            }),
            PatternType::Array { element, length } => self.field("array", |printer| {
                printer.pattern_type(element)?;
                printer.unsigned(*length as u64);
                Ok(())
            }),
            PatternType::SymbolicArray { element, length } => {
                self.field("symbolic-array", |printer| {
                    printer.pattern_type(element)?;
                    printer.pattern_const(length)
                })
            }
            PatternType::Slice(element) => {
                self.field("slice", |printer| printer.pattern_type(element))
            }
            PatternType::Record(record) => self.field("record", |printer| {
                printer.string(record.name());
                Ok(())
            }),
            PatternType::Enum(en) => self.field("enum", |printer| {
                printer.string(en.name());
                Ok(())
            }),
            PatternType::Reference {
                mutability,
                referent,
            } => self.field("reference", |printer| {
                printer.atom(reference_mutability_atom(*mutability));
                printer.pattern_type(referent)
            }),
            PatternType::Float(float) => {
                self.atom(match float {
                    PatternFloatType::F32 => "f32",
                    PatternFloatType::F64 => "f64",
                });
                Ok(())
            }
            PatternType::Opaque(name) => self.field("opaque", |printer| {
                printer.string(name);
                Ok(())
            }),
            PatternType::Unsupported(name) => self.field("unsupported", |printer| {
                printer.string(name);
                Ok(())
            }),
        }
    }

    fn typed_binding(&mut self, binding: &TypedBinding) -> Result<(), GoldenEncodeError> {
        self.field("binding", |printer| {
            printer.string(binding.name());
            printer.field("matched", |printer| {
                printer.pattern_type(binding.matched_type())
            })?;
            printer.field("bound", |printer| {
                printer.pattern_type(binding.binding_type())
            })?;
            printer.atom(match binding.mode() {
                BindingMode::Move => "move",
                BindingMode::Ref => "ref",
                BindingMode::RefMut => "ref-mut",
            });
            printer.boolean(binding.variable_mutable());
            Ok(())
        })
    }

    fn typed_pattern(&mut self, pattern: &TypedPattern) -> Result<(), GoldenEncodeError> {
        self.field("pattern", |printer| {
            printer.field("ty", |printer| printer.pattern_type(pattern.ty()))?;
            match pattern.kind() {
                TypedPatternKind::Wildcard => {
                    printer.atom("wildcard");
                    Ok(())
                }
                TypedPatternKind::Unit => {
                    printer.atom("unit");
                    Ok(())
                }
                TypedPatternKind::Binding(binding) => printer.typed_binding(binding),
                TypedPatternKind::Literal(literal) => {
                    printer.field("literal", |printer| printer.pattern_literal(literal))
                }
                TypedPatternKind::NeedsCtfe(dependency) => {
                    printer.field("needs-ctfe", |printer| printer.pattern_const(dependency))
                }
                TypedPatternKind::Dereference {
                    mutability,
                    inserted,
                    pattern,
                } => printer.field("dereference", |printer| {
                    printer.atom(reference_mutability_atom(*mutability));
                    printer.boolean(*inserted);
                    printer.typed_pattern(pattern)
                }),
                TypedPatternKind::Tuple(fields) => printer.field("tuple", |printer| {
                    for field in fields.iter() {
                        printer.typed_pattern(field)?;
                    }
                    Ok(())
                }),
                TypedPatternKind::Slice {
                    elements,
                    prefix_length,
                    suffix_length,
                } => printer.field("slice", |printer| {
                    printer.unsigned(*prefix_length as u64);
                    printer.unsigned(*suffix_length as u64);
                    for element in elements.iter() {
                        printer.typed_pattern(element)?;
                    }
                    Ok(())
                }),
                TypedPatternKind::DynamicSlice {
                    prefix,
                    has_rest,
                    suffix,
                } => printer.field("dynamic-slice", |printer| {
                    printer.boolean(*has_rest);
                    printer.field("prefix", |printer| {
                        for element in prefix.iter() {
                            printer.typed_pattern(element)?;
                        }
                        Ok(())
                    })?;
                    printer.field("suffix", |printer| {
                        for element in suffix.iter() {
                            printer.typed_pattern(element)?;
                        }
                        Ok(())
                    })
                }),
                TypedPatternKind::SymbolicSlice {
                    prefix,
                    has_rest,
                    suffix,
                    length,
                } => printer.field("symbolic-slice", |printer| {
                    printer.boolean(*has_rest);
                    printer.pattern_const(length)?;
                    printer.field("prefix", |printer| {
                        for element in prefix.iter() {
                            printer.typed_pattern(element)?;
                        }
                        Ok(())
                    })?;
                    printer.field("suffix", |printer| {
                        for element in suffix.iter() {
                            printer.typed_pattern(element)?;
                        }
                        Ok(())
                    })
                }),
                TypedPatternKind::Record {
                    record_name,
                    field_names,
                    fields,
                } => printer.field("record", |printer| {
                    printer.string(record_name);
                    printer.field("field-names", |printer| {
                        for name in field_names.iter() {
                            printer.string(name);
                        }
                        Ok(())
                    })?;
                    printer.field("fields", |printer| {
                        for field in fields.iter() {
                            printer.typed_pattern(field)?;
                        }
                        Ok(())
                    })
                }),
                TypedPatternKind::Constructor {
                    enum_name,
                    variant_index,
                    variant,
                    fields,
                } => printer.field("constructor", |printer| {
                    printer.string(enum_name);
                    printer.unsigned(*variant_index as u64);
                    printer.string(variant);
                    printer.field("fields", |printer| {
                        for field in fields.iter() {
                            printer.typed_pattern(field)?;
                        }
                        Ok(())
                    })
                }),
                TypedPatternKind::RecordConstructor {
                    enum_name,
                    variant_index,
                    variant,
                    field_names,
                    fields,
                } => printer.field("record-constructor", |printer| {
                    printer.string(enum_name);
                    printer.unsigned(*variant_index as u64);
                    printer.string(variant);
                    printer.field("field-names", |printer| {
                        for name in field_names.iter() {
                            printer.string(name);
                        }
                        Ok(())
                    })?;
                    printer.field("fields", |printer| {
                        for field in fields.iter() {
                            printer.typed_pattern(field)?;
                        }
                        Ok(())
                    })
                }),
                TypedPatternKind::Range {
                    start,
                    end,
                    inclusive,
                } => printer.field("range", |printer| {
                    printer.range_endpoint(start)?;
                    printer.range_endpoint(end)?;
                    printer.boolean(*inclusive);
                    Ok(())
                }),
                TypedPatternKind::At { binding, pattern } => printer.field("at", |printer| {
                    printer.typed_binding(binding)?;
                    printer.typed_pattern(pattern)
                }),
                TypedPatternKind::Or(alternatives) => printer.field("or", |printer| {
                    for alternative in alternatives.iter() {
                        printer.typed_pattern(alternative)?;
                    }
                    Ok(())
                }),
            }
        })
    }

    fn generator_target(&mut self, target: &GeneratorTarget) -> Result<(), GoldenEncodeError> {
        self.field("target", |printer| match target {
            GeneratorTarget::Named {
                declaration,
                arguments,
                hidden_lifetime_binders,
            } => printer.field("named", |printer| {
                printer.declaration_path(declaration)?;
                printer.field("arguments", |printer| {
                    for argument in arguments {
                        printer.generic_argument(argument)?;
                    }
                    Ok(())
                })?;
                printer.field("hidden-lifetime-binders", |printer| {
                    for binder in hidden_lifetime_binders {
                        printer.unsigned(*binder);
                    }
                    Ok(())
                })
            }),
            GeneratorTarget::Anonymous {
                owner,
                expression_ordinal,
                arguments,
            } => printer.field("anonymous", |printer| {
                printer.declaration_path(owner)?;
                printer.field("ordinal", |printer| {
                    printer.unsigned(*expression_ordinal);
                    Ok(())
                })?;
                printer.field("arguments", |printer| {
                    for argument in arguments {
                        printer.generic_argument(argument)?;
                    }
                    Ok(())
                })
            }),
        })
    }

    fn generic_argument(
        &mut self,
        argument: &GenericArgumentShape,
    ) -> Result<(), GoldenEncodeError> {
        match argument {
            GenericArgumentShape::Type(ty) => self.symbolic_type(ty),
            GenericArgumentShape::Lifetime(lifetime) => self.lifetime(lifetime),
            GenericArgumentShape::IntegerConst(expression) => self.const_expression(expression),
        }
    }

    fn span(&mut self, span: Span) {
        self.unsigned(span.file.0);
        self.unsigned(span.start.byte);
        self.unsigned(span.start.line);
        self.unsigned(span.start.column);
        self.unsigned(span.end.byte);
        self.unsigned(span.end.line);
        self.unsigned(span.end.column);
    }

    fn coercion(&mut self, coercion: CheckedCoercion) -> Result<(), GoldenEncodeError> {
        self.field("coercion", |printer| {
            printer.atom(match coercion.kind() {
                CoercionKind::Identity => "identity",
                CoercionKind::NeverToAny => "never-to-any",
                CoercionKind::LifetimeShortening => "lifetime-shortening",
                CoercionKind::MutableReborrowToShared => "mutable-reborrow-to-shared",
                CoercionKind::ArrayReferenceToSlice => "array-reference-to-slice",
                CoercionKind::FunctionPointer => "function-pointer",
                CoercionKind::NoncapturingClosureToFunctionPointer => {
                    "noncapturing-closure-to-function-pointer"
                }
            });
            printer.boolean(coercion.effects_pending_c4());
            Ok(())
        })
    }

    fn sealed_operator(
        &mut self,
        operator: &SealedPrimitiveOperator,
    ) -> Result<(), GoldenEncodeError> {
        self.field("sealed", |printer| {
            printer.atom(match operator.trait_kind() {
                PrimitiveOperatorTrait::Neg => "neg",
                PrimitiveOperatorTrait::LogicalNot => "logical-not",
                PrimitiveOperatorTrait::BitNot => "bit-not",
                PrimitiveOperatorTrait::Add => "add",
                PrimitiveOperatorTrait::Sub => "sub",
                PrimitiveOperatorTrait::Mul => "mul",
                PrimitiveOperatorTrait::Div => "div",
                PrimitiveOperatorTrait::Rem => "rem",
                PrimitiveOperatorTrait::ShiftLeft => "shift-left",
                PrimitiveOperatorTrait::ShiftRight => "shift-right",
                PrimitiveOperatorTrait::BitAnd => "bit-and",
                PrimitiveOperatorTrait::BitXor => "bit-xor",
                PrimitiveOperatorTrait::BitOr => "bit-or",
                PrimitiveOperatorTrait::Eq => "eq",
                PrimitiveOperatorTrait::Ord => "ord",
            });
            printer.field("self", |printer| {
                printer.symbolic_type(operator.self_type())
            })?;
            printer.field("arguments", |printer| {
                for argument in operator.arguments() {
                    printer.symbolic_type(argument)?;
                }
                Ok(())
            })?;
            printer.atom(match operator.domain() {
                PrimitiveDomain::Unit => "unit",
                PrimitiveDomain::Bool => "bool",
                PrimitiveDomain::SignedInteger => "signed-integer",
                PrimitiveDomain::UnsignedInteger => "unsigned-integer",
                PrimitiveDomain::Float => "float",
                PrimitiveDomain::Char => "char",
                PrimitiveDomain::Entity => "entity",
                PrimitiveDomain::RawPointer => "raw-pointer",
            });
            Ok(())
        })
    }

    fn primitive_selection(
        &mut self,
        selection: &CheckedPrimitiveSelection,
    ) -> Result<(), GoldenEncodeError> {
        self.field("selection", |printer| match selection {
            CheckedPrimitiveSelection::Sealed(operator) => printer.sealed_operator(operator),
            CheckedPrimitiveSelection::BooleanLogical => {
                printer.atom("boolean-logical");
                Ok(())
            }
            CheckedPrimitiveSelection::FloatComparison(float) => {
                printer.field("float-comparison", |printer| {
                    printer.atom(match float {
                        FloatType::F32 => "f32",
                        FloatType::F64 => "f64",
                    });
                    Ok(())
                })
            }
        })
    }

    fn checked_expression(
        &mut self,
        expression: &CheckedExpression,
    ) -> Result<(), GoldenEncodeError> {
        self.field("expr", |printer| {
            printer.field("natural", |printer| {
                printer.symbolic_type(expression.natural_type())
            })?;
            printer.field("ty", |printer| printer.symbolic_type(expression.ty()))?;
            if let Some(coercion) = expression.coercion() {
                printer.coercion(coercion)?;
            }
            printer.expression_kind(expression.kind())
        })
    }

    fn expression_kind(&mut self, kind: &CheckedExpressionKind) -> Result<(), GoldenEncodeError> {
        match kind {
            CheckedExpressionKind::Known => {
                self.atom("known");
                Ok(())
            }
            CheckedExpressionKind::IntegerLiteral {
                unary_negative,
                little_endian_bits,
            } => self.field("integer-literal", |printer| {
                printer.boolean(*unary_negative);
                printer.hex(little_endian_bits);
                Ok(())
            }),
            CheckedExpressionKind::FloatLiteral {
                unary_negative,
                raw_bits,
                little_endian_bits,
            } => self.field("float-literal", |printer| {
                printer.boolean(*unary_negative);
                printer.output.push(' ');
                if little_endian_bits.len() == 4 {
                    let _ = write!(printer.output, "0x{raw_bits:08X}");
                } else {
                    let _ = write!(printer.output, "0x{raw_bits:016X}");
                }
                printer.hex(little_endian_bits);
                Ok(())
            }),
            CheckedExpressionKind::Character(value) => self.field("character", |printer| {
                let mut buffer = [0_u8; 4];
                printer.string(value.encode_utf8(&mut buffer));
                Ok(())
            }),
            CheckedExpressionKind::String(value) => self.field("string", |printer| {
                printer.string(value);
                Ok(())
            }),
            CheckedExpressionKind::Boolean(value) => self.field("boolean", |printer| {
                printer.boolean(*value);
                Ok(())
            }),
            CheckedExpressionKind::Unit => {
                self.atom("unit");
                Ok(())
            }
            CheckedExpressionKind::Tuple(elements) => self.field("tuple", |printer| {
                for element in elements {
                    printer.checked_expression(element)?;
                }
                Ok(())
            }),
            CheckedExpressionKind::Array(elements) => self.field("array", |printer| {
                for element in elements {
                    printer.checked_expression(element)?;
                }
                Ok(())
            }),
            CheckedExpressionKind::ArrayRepeat { value, length } => {
                self.field("array-repeat", |printer| {
                    printer.checked_expression(value)?;
                    printer.const_expression(length)
                })
            }
            CheckedExpressionKind::Borrow {
                mutability,
                lifetime,
                value,
            } => self.field("borrow", |printer| {
                printer.atom(mutability_atom(*mutability));
                printer.lifetime(lifetime)?;
                printer.checked_expression(value)
            }),
            CheckedExpressionKind::Block { statements, tail } => self.field("block", |printer| {
                printer.field("statements", |printer| {
                    for statement in statements {
                        printer.checked_expression(statement)?;
                    }
                    Ok(())
                })?;
                printer.field("tail", |printer| match tail {
                    Some(tail) => printer.checked_expression(tail),
                    None => Ok(()),
                })
            }),
            CheckedExpressionKind::If {
                condition,
                then_branch,
                else_branch,
            } => self.field("if", |printer| {
                printer.checked_expression(condition)?;
                printer.checked_expression(then_branch)?;
                match else_branch {
                    Some(else_branch) => printer.checked_expression(else_branch),
                    None => Ok(()),
                }
            }),
            CheckedExpressionKind::While { condition, body } => self.field("while", |printer| {
                printer.checked_expression(condition)?;
                printer.checked_expression(body)
            }),
            CheckedExpressionKind::Loop { body, break_count } => self.field("loop", |printer| {
                printer.unsigned(*break_count as u64);
                printer.checked_expression(body)
            }),
            CheckedExpressionKind::Return(value) => self.field("return", |printer| match value {
                Some(value) => printer.checked_expression(value),
                None => Ok(()),
            }),
            CheckedExpressionKind::Break(value) => self.field("break", |printer| match value {
                Some(value) => printer.checked_expression(value),
                None => Ok(()),
            }),
            CheckedExpressionKind::Continue => {
                self.atom("continue");
                Ok(())
            }
            CheckedExpressionKind::Unary {
                operator,
                operand,
                selection,
            } => self.field("unary", |printer| {
                printer.atom(spell_unary_operator(*operator));
                printer.primitive_selection(selection)?;
                printer.checked_expression(operand)
            }),
            CheckedExpressionKind::Binary {
                operator,
                left,
                right,
                selection,
            } => self.field("binary", |printer| {
                printer.atom(spell_binary_operator(*operator));
                printer.primitive_selection(selection)?;
                printer.checked_expression(left)?;
                printer.checked_expression(right)
            }),
            CheckedExpressionKind::Assignment { value } => {
                self.field("assignment", |printer| printer.checked_expression(value))
            }
            CheckedExpressionKind::AddAssignment { value, selection } => {
                self.field("add-assignment", |printer| {
                    printer.sealed_operator(selection)?;
                    printer.checked_expression(value)
                })
            }
            CheckedExpressionKind::Coerce {
                value,
                target,
                coercion,
            } => self.field("coerce", |printer| {
                printer.field("target", |printer| printer.symbolic_type(target))?;
                printer.coercion(*coercion)?;
                printer.checked_expression(value)
            }),
        }
    }

    fn callee(&mut self, callee: &CheckedBodyCallee) -> Result<(), GoldenEncodeError> {
        self.field("callee", |printer| match callee {
            CheckedBodyCallee::DirectItem(item) => printer.field("direct-item", |printer| {
                printer.unsigned(item.0);
                Ok(())
            }),
            CheckedBodyCallee::AssociatedItem(item) => {
                printer.field("associated-item", |printer| {
                    printer.unsigned(item.0);
                    Ok(())
                })
            }
            CheckedBodyCallee::FunctionPointer => {
                printer.atom("function-pointer");
                Ok(())
            }
            CheckedBodyCallee::ClosureValue => {
                printer.atom("closure-value");
                Ok(())
            }
            CheckedBodyCallee::GeneratorFactoryValue => {
                printer.atom("generator-factory-value");
                Ok(())
            }
            CheckedBodyCallee::GeneratorResume => {
                printer.atom("generator-resume");
                Ok(())
            }
            CheckedBodyCallee::EmbeddedMethod(method) => {
                printer.field("embedded-method", |printer| {
                    printer.unsigned(u64::from(method.ordinal()));
                    Ok(())
                })
            }
            CheckedBodyCallee::EmbeddedDefinition(definition) => {
                printer.field("embedded-definition", |printer| {
                    printer.unsigned(u64::from(definition.ordinal()));
                    Ok(())
                })
            }
            CheckedBodyCallee::TraitMethod { trait_path, method } => {
                printer.field("trait-method", |printer| {
                    printer.declaration_path(trait_path)?;
                    printer.string(method);
                    Ok(())
                })
            }
            CheckedBodyCallee::QueryIteration => {
                printer.atom("query-iteration");
                Ok(())
            }
            CheckedBodyCallee::CommandSpawn => {
                printer.atom("command-spawn");
                Ok(())
            }
        })
    }
}

/// Infallible canonical spelling of a symbolic type for diagnostic text.
pub(crate) fn spell_symbolic_type(ty: &SymbolicType) -> String {
    let mut printer = Printer::new("");
    printer
        .symbolic_type(ty)
        .expect("the symbolic type printer is total");
    printer.output.split_off(1).trim_start().to_owned()
}

/// Canonical spelling of a compiler trait kind for diagnostic text.
pub(crate) const fn compiler_trait_kind_atom(
    kind: arche_frontend::embedded_core::CompilerTraitKind,
) -> &'static str {
    use arche_frontend::embedded_core::CompilerTraitKind as K;
    match kind {
        K::Add => "Add",
        K::BitAnd => "BitAnd",
        K::BitNot => "BitNot",
        K::BitOr => "BitOr",
        K::BitXor => "BitXor",
        K::Clone => "Clone",
        K::Copy => "Copy",
        K::Div => "Div",
        K::Drop => "Drop",
        K::EcsKey => "EcsKey",
        K::EcsValue => "EcsValue",
        K::Eq => "Eq",
        K::Fn => "Fn",
        K::FnMut => "FnMut",
        K::FnOnce => "FnOnce",
        K::From => "From",
        K::IntoIterator => "IntoIterator",
        K::Iterator => "Iterator",
        K::LogicalNot => "LogicalNot",
        K::Mul => "Mul",
        K::Neg => "Neg",
        K::Ord => "Ord",
        K::Rem => "Rem",
        K::Send => "Send",
        K::ShiftLeft => "ShiftLeft",
        K::ShiftRight => "ShiftRight",
        K::Sub => "Sub",
        K::Sync => "Sync",
        K::TryFrom => "TryFrom",
        K::Unpin => "Unpin",
        K::UnwindPayload => "UnwindPayload",
    }
}

pub(crate) fn spell_unary_operator(operator: UnaryTypeOperator) -> &'static str {
    match operator {
        UnaryTypeOperator::Negate => "negate",
        UnaryTypeOperator::LogicalNot => "logical-not",
        UnaryTypeOperator::BitNot => "bit-not",
    }
}

pub(crate) fn spell_binary_operator(operator: BinaryTypeOperator) -> &'static str {
    match operator {
        BinaryTypeOperator::LogicalOr => "logical-or",
        BinaryTypeOperator::LogicalAnd => "logical-and",
        BinaryTypeOperator::BitOr => "bit-or",
        BinaryTypeOperator::BitXor => "bit-xor",
        BinaryTypeOperator::BitAnd => "bit-and",
        BinaryTypeOperator::Equal => "equal",
        BinaryTypeOperator::NotEqual => "not-equal",
        BinaryTypeOperator::Less => "less",
        BinaryTypeOperator::LessEqual => "less-equal",
        BinaryTypeOperator::Greater => "greater",
        BinaryTypeOperator::GreaterEqual => "greater-equal",
        BinaryTypeOperator::ShiftLeft => "shift-left",
        BinaryTypeOperator::ShiftRight => "shift-right",
        BinaryTypeOperator::Add => "add",
        BinaryTypeOperator::Subtract => "subtract",
        BinaryTypeOperator::Multiply => "multiply",
        BinaryTypeOperator::Divide => "divide",
        BinaryTypeOperator::Remainder => "remainder",
    }
}

/// Canonical prose for a generic-parameter kind in diagnostic text.
pub(crate) fn generic_parameter_prose(kind: &GenericParameterKind) -> String {
    match kind {
        GenericParameterKind::Type => "type".to_owned(),
        GenericParameterKind::Lifetime => "lifetime".to_owned(),
        GenericParameterKind::IntegerConst(integer) => {
            format!("const {}", integer_type_atom(*integer))
        }
    }
}

const fn reference_mutability_atom(mutability: ReferenceMutability) -> &'static str {
    match mutability {
        ReferenceMutability::Shared => "shared",
        ReferenceMutability::Mutable => "mutable",
    }
}

const fn mutability_atom(mutability: Mutability) -> &'static str {
    match mutability {
        Mutability::Shared => "shared",
        Mutability::Mutable => "mutable",
    }
}

pub(crate) const fn integer_type_atom(integer: IntegerType) -> &'static str {
    match integer {
        IntegerType::I8 => "i8",
        IntegerType::I16 => "i16",
        IntegerType::I32 => "i32",
        IntegerType::I64 => "i64",
        IntegerType::U8 => "u8",
        IntegerType::U16 => "u16",
        IntegerType::U32 => "u32",
        IntegerType::U64 => "u64",
        IntegerType::Isize => "isize",
        IntegerType::Usize => "usize",
    }
}

pub(crate) const fn declaration_kind_atom(kind: DeclarationKind) -> &'static str {
    match kind {
        DeclarationKind::World => "world",
        DeclarationKind::Component => "component",
        DeclarationKind::Resource => "resource",
        DeclarationKind::Tag => "tag",
        DeclarationKind::System => "system",
        DeclarationKind::Schedule => "schedule",
        DeclarationKind::Function => "function",
        DeclarationKind::Generator => "generator",
        DeclarationKind::Struct => "struct",
        DeclarationKind::Enum => "enum",
        DeclarationKind::Trait => "trait",
        DeclarationKind::Impl => "impl",
        DeclarationKind::TypeAlias => "type-alias",
        DeclarationKind::Const => "const",
        DeclarationKind::Static => "static",
        DeclarationKind::Query => "query",
    }
}

const fn generic_parameter_atom(kind: &GenericParameterKind) -> &'static str {
    match kind {
        GenericParameterKind::Type => "type",
        GenericParameterKind::Lifetime => "lifetime",
        GenericParameterKind::IntegerConst(integer) => integer_type_atom(*integer),
    }
}

/// Prints the `ARCHE-TYPE-TEXT 1` golden: the checked-type universe (targets
/// and per-producer checked-type rows) of one fully checked C2 workspace.
pub fn dump_type_text(workspace: &C2CheckedWorkspace) -> Result<String, GoldenEncodeError> {
    let mut printer = Printer::new("ARCHE-TYPE-TEXT 1");
    printer.form("c2-type-universe", |printer| {
        printer.field("targets", |printer| {
            for target in workspace.targets() {
                printer.field("target", |printer| {
                    printer.field("package", |printer| {
                        printer.atom(&target.package().to_string());
                        Ok(())
                    })?;
                    let scope = std::str::from_utf8(target.package_scope().as_bytes())
                        .map_err(|_| GoldenEncodeError::NonUtf8Scope)?;
                    printer.field("scope", |printer| {
                        printer.string(scope);
                        Ok(())
                    })?;
                    printer.field("target-id", |printer| {
                        printer.unsigned(target.target().0);
                        Ok(())
                    })?;
                    printer.resolution(target.resolution())?;
                    printer.pending_c4(target.pending_c4())
                })?;
            }
            Ok(())
        })?;
        printer.field("checked-types", |printer| {
            let indexes = workspace.indexes();
            for offset in 0..indexes.checked_type_count() {
                let index = indexes
                    .checked_type_index(offset as u64)
                    .expect("offset within checked-type table");
                let view = indexes
                    .checked_type(&index)
                    .expect("index minted by this table");
                printer.field("checked-type", |printer| {
                    printer.field("producer", |printer| match view.producer() {
                        C2TypeProducer::Declaration(item) => {
                            printer.field("declaration", |printer| {
                                printer.unsigned(item.0);
                                Ok(())
                            })
                        }
                        C2TypeProducer::Body(body) => printer.field("body", |printer| {
                            printer.unsigned(body.0);
                            Ok(())
                        }),
                    })?;
                    printer.resolution(view.resolution())?;
                    printer.pending_c4(view.pending_c4())
                })?;
            }
            Ok(())
        })?;
        printer.field("bodies", |printer| {
            for body in workspace.bodies().bodies() {
                printer.field("body", |printer| {
                    printer.field("id", |printer| {
                        printer.unsigned(body.id().0);
                        Ok(())
                    })?;
                    printer.field("owner", |printer| {
                        printer.unsigned(body.owner().0);
                        Ok(())
                    })?;
                    printer.field("kind", |printer| {
                        printer.atom(match body.kind() {
                            SemanticBodyKind::Declaration => "declaration",
                            SemanticBodyKind::Closure => "closure",
                            SemanticBodyKind::Generator => "generator",
                            SemanticBodyKind::WorldInitializer => "world-initializer",
                            SemanticBodyKind::ArrayLength => "array-length",
                            SemanticBodyKind::RepeatCount => "repeat-count",
                            SemanticBodyKind::IntegerGenericArgument => "integer-generic-argument",
                        });
                        Ok(())
                    })?;
                    printer.field("span", |printer| {
                        printer.span(body.span());
                        Ok(())
                    })?;
                    printer.resolution(body.resolution())?;
                    printer.pending_c4(body.pending_c4())?;
                    printer.field("locals", |printer| {
                        for local in body.locals() {
                            printer.field("local", |printer| {
                                printer.unsigned(local.local().owner.0);
                                printer.unsigned(local.local().ordinal);
                                printer.symbolic_type(local.ty())
                            })?;
                        }
                        Ok(())
                    })?;
                    printer.field("expressions", |printer| {
                        for expression in body.expressions() {
                            printer.field("expression", |printer| {
                                printer.field("span", |printer| {
                                    printer.span(expression.span());
                                    Ok(())
                                })?;
                                printer.checked_expression(expression.expression())
                            })?;
                        }
                        Ok(())
                    })?;
                    printer.field("patterns", |printer| {
                        for pattern in body.patterns() {
                            printer.field("pattern", |printer| {
                                printer.field("span", |printer| {
                                    printer.span(pattern.span());
                                    Ok(())
                                })?;
                                printer.pattern_analysis(pattern.analysis())
                            })?;
                        }
                        Ok(())
                    })?;
                    printer.field("calls", |printer| {
                        for call in body.calls() {
                            printer.field("call", |printer| {
                                printer.field("span", |printer| {
                                    printer.span(call.span());
                                    Ok(())
                                })?;
                                printer.callee(call.callee())?;
                                printer
                                    .field("result", |printer| printer.symbolic_type(call.result()))
                            })?;
                        }
                        Ok(())
                    })
                })?;
            }
            Ok(())
        })
    })?;
    Ok(printer.finish())
}

/// Prints the `ARCHE-TRAIT-TEXT 1` golden: per-declaration rows with trait
/// method tables and ordinary-impl candidate descriptors (keys, heads, and
/// canonical environments) from the checked declaration facts.
pub fn dump_trait_text(workspace: &C2CheckedWorkspace) -> Result<String, GoldenEncodeError> {
    let mut printer = Printer::new("ARCHE-TRAIT-TEXT 1");
    printer.form("c2-trait-universe", |printer| {
        for declaration in workspace.declarations().declarations() {
            printer.field("declaration", |printer| {
                printer.field("name", |printer| {
                    printer.string(declaration.name());
                    Ok(())
                })?;
                printer.field("kind", |printer| {
                    printer.atom(declaration_kind_atom(declaration.kind()));
                    Ok(())
                })?;
                printer.field("package", |printer| {
                    printer.atom(&declaration.package().to_string());
                    Ok(())
                })?;
                printer.field("target-id", |printer| {
                    printer.unsigned(declaration.target().0);
                    Ok(())
                })?;
                printer.field("session-key", |printer| {
                    printer.hex(declaration.session_traversal_bytes());
                    Ok(())
                })?;
                printer.resolution(declaration.resolution())?;
                printer.pending_c4(declaration.pending_c4())?;
                if let SymbolicDeclarationPayloadSkeleton::Trait { methods } =
                    &declaration.declaration_shape().payload
                {
                    printer.field("trait", |printer| {
                        printer.field("methods", |printer| {
                            for method in methods {
                                printer.string(&method.name);
                            }
                            Ok(())
                        })
                    })?;
                }
                if declaration.kind() == DeclarationKind::Impl {
                    match declaration.ordinary_impl_candidate() {
                        None => printer.field("impl", |printer| {
                            printer.field("inherent", |printer| {
                                // A checked row's impl payload target is
                                // post-C2 resolved by construction.
                                let SymbolicDeclarationPayloadSkeleton::Impl { target, .. } =
                                    &declaration.declaration_shape().payload
                                else {
                                    return Err(GoldenEncodeError::UnprintableType);
                                };
                                let arche_frontend::SymbolicTypeShapeSkeleton::Resolved {
                                    value,
                                    ..
                                } = target
                                else {
                                    return Err(GoldenEncodeError::UnprintableType);
                                };
                                printer.field("target", |printer| printer.symbolic_type(value))
                            })
                        })?,
                        Some(candidate) => printer.field("impl", |printer| {
                            printer.field("candidate", |printer| {
                                printer.field("default", |printer| {
                                    printer.boolean(candidate.is_default());
                                    Ok(())
                                })?;
                                printer.field("generic-parameters", |printer| {
                                    for parameter in candidate.generic_parameters() {
                                        printer.atom(generic_parameter_atom(parameter));
                                    }
                                    Ok(())
                                })?;
                                printer.field("head", |printer| {
                                    printer.field("trait-key", |printer| {
                                        printer.hex(candidate.head().trait_key().canonical_bytes());
                                        Ok(())
                                    })?;
                                    printer.field("self", |printer| {
                                        printer.symbolic_type(candidate.head().self_type())
                                    })?;
                                    printer.field("arguments", |printer| {
                                        for argument in candidate.head().arguments() {
                                            printer.generic_argument(argument)?;
                                        }
                                        Ok(())
                                    })
                                })?;
                                printer.field("environment", |printer| {
                                    for predicate in candidate.environment().predicates() {
                                        printer.hex(predicate.canonical_bytes());
                                    }
                                    Ok(())
                                })
                            })
                        })?,
                    }
                }
                Ok(())
            })?;
        }
        Ok(())
    })?;
    Ok(printer.finish())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use arche_frontend::{check_workspace_c1, FrontendOutput};
    use arche_package::{load_workspace, resolve, ManifestRequest, RegistrySnapshot};
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::checker::check_workspace_c2;

    fn corpus_frontend(name: &str) -> FrontendOutput {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../tests/m27c2/v1")
            .join(name);
        let workspace = load_workspace(&ManifestRequest::discover_from(&root)).unwrap();
        let graph = resolve(&workspace, &RegistrySnapshot::empty()).unwrap();
        check_workspace_c1(&workspace, &graph, &[]).unwrap()
    }

    fn digest_hex(text: &str) -> String {
        let digest: [u8; 32] = Sha256::digest(text.as_bytes()).into();
        digest.iter().map(|byte| format!("{byte:02X}")).collect()
    }

    fn assert_envelope(text: &str, header: &str) {
        assert!(text.starts_with(header), "{header} missing");
        assert!(text.ends_with(")\n"), "root close + final LF missing");
        assert!(!text.ends_with("\n\n"), "excess trailing LF");
        assert!(
            !text.contains(env!("CARGO_MANIFEST_DIR")),
            "host path leaked into a golden"
        );
        assert!(
            !text.contains("tests/m27c2"),
            "corpus path leaked into a golden"
        );
        assert_eq!(
            text.matches('\n').count(),
            2,
            "one root expression + final LF"
        );
        let opens = text.matches('(').count();
        let closes = text.matches(')').count();
        assert_eq!(opens, closes, "unbalanced S-expression");
        assert!(opens > 2, "empty golden");
    }

    #[test]
    fn printer_units_pin_exact_spellings() {
        let mut printer = Printer::new("X 1");
        printer
            .symbolic_type(&SymbolicType::Reference {
                mutability: arche_frontend::Mutability::Mutable,
                lifetime: arche_frontend::SymbolicLifetime::Static,
                pointee: Box::new(SymbolicType::Slice(Box::new(SymbolicType::U8))),
            })
            .unwrap();
        assert_eq!(
            printer.output,
            "X 1\n (reference mutable static (slice u8))"
        );

        let mut printer = Printer::new("X 1");
        printer.string("a\"b\\c\nd\u{1}e\u{e9}");
        assert_eq!(printer.output, "X 1\n \"a\\\"b\\\\c\\nd\\u0001e\u{e9}\"");

        let mut printer = Printer::new("X 1");
        printer
            .field("empty", |printer| {
                printer.hex(&[]);
                Ok(())
            })
            .unwrap();
        assert_eq!(printer.output, "X 1\n (empty)");

        let mut printer = Printer::new("X 1");
        printer
            .symbolic_type(&SymbolicType::Tuple(vec![
                SymbolicType::I32,
                SymbolicType::RawPointer {
                    mutability: arche_frontend::Mutability::Shared,
                    pointee: Box::new(SymbolicType::Str),
                },
            ]))
            .unwrap();
        assert_eq!(printer.output, "X 1\n (tuple i32 (raw-pointer shared str))");

        let mut printer = Printer::new("X 1");
        printer
            .lifetime(&arche_frontend::SymbolicLifetime::Bound { depth: 0, index: 2 })
            .unwrap();
        assert_eq!(printer.output, "X 1\n (bound-lifetime 0 2)");

        let mut printer = Printer::new("X 1");
        printer
            .symbolic_type(&SymbolicType::Closure {
                owner: Box::new(SemanticDeclarationPath {
                    registry_origin: String::new(),
                    package_name: String::new(),
                    target: TargetRoot::Library,
                    modules: Vec::new(),
                    kind: DeclarationKind::Function,
                    name: String::new(),
                }),
                expression_ordinal: 0,
                captures: Vec::new(),
                parameters: Vec::new(),
                result: Box::new(SymbolicType::Unit),
                requires: arche_frontend::SymbolicTypeEffectSet::resolved(Vec::new()),
                throws: arche_frontend::SymbolicTypeEffectSet::resolved(Vec::new()),
                arguments: Vec::new(),
            })
            .unwrap();
        assert_eq!(
            printer.output,
            concat!(
                "X 1\n (closure (path \"\" \"\" library function \"\") (ordinal 0) ",
                "(captures) (parameters) (result unit) (requires) (throws) (arguments))"
            )
        );
    }

    #[test]
    fn corpus_goldens_are_deterministic_and_pinned() {
        let expectations = [
            (
                "language-game",
                "543061514438202F9293D5F15875FF77F02779E1F0A51712D7DF3D989625437A",
                "68D1F2BDDBF8357A75338A26D373C55AD99DD4E7C7E7E4183CAB9F9AA2CBB014",
            ),
            (
                "language-environment",
                "2421947B5957DC97235EF564DD546ECB985A857B1A69E61C6575C088555E399D",
                "16276CA4061F43F708B8CC3CB153541C434A40F49D548B7BCD33D5D5BB7E7643",
            ),
        ];
        let mut mismatches = Vec::new();
        for (corpus, type_pin, trait_pin) in expectations {
            let checked = check_workspace_c2(corpus_frontend(corpus))
                .unwrap_or_else(|failure| panic!("{corpus} must check: {failure:?}"));
            let type_text = dump_type_text(&checked).unwrap();
            let trait_text = dump_trait_text(&checked).unwrap();
            assert_eq!(type_text, dump_type_text(&checked).unwrap(), "{corpus}");
            assert_eq!(trait_text, dump_trait_text(&checked).unwrap(), "{corpus}");
            assert_envelope(&type_text, "ARCHE-TYPE-TEXT 1\n");
            assert_envelope(&trait_text, "ARCHE-TRAIT-TEXT 1\n");
            for (label, actual, expected) in [
                ("type", digest_hex(&type_text), type_pin),
                ("trait", digest_hex(&trait_text), trait_pin),
            ] {
                if actual != expected {
                    mismatches.push(format!("{corpus} {label} actual={actual}"));
                }
            }
        }
        assert!(mismatches.is_empty(), "digest pins: {mismatches:#?}");
    }
}
