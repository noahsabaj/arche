use std::fmt::Write;

use crate::layout;
use crate::parser::Program;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComponentInspectError {
    pub message: String,
}

pub fn format_components(program: &Program) -> Result<String, ComponentInspectError> {
    let mut output = String::new();

    for (index, component) in program.components.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }

        let qualified_name = layout::component_qualified_name(&program.world.name, &component.name);
        let component_layout =
            layout::compute_component_layout(component).map_err(|error| ComponentInspectError {
                message: error.message,
            })?;

        let _ = writeln!(output, "component {qualified_name}");
        let _ = writeln!(output, "  size: {}", component_layout.size);
        let _ = writeln!(output, "  align: {}", component_layout.align);
        let _ = writeln!(output, "  fields:");

        for field in component_layout.fields {
            let _ = writeln!(
                output,
                "    {}: {} @ {}",
                field.name, field.type_name, field.offset
            );
        }
    }

    if output.ends_with('\n') {
        output.pop();
    }

    Ok(output)
}
