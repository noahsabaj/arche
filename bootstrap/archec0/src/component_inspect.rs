use std::fmt;
use std::io::{self, Write};

use crate::layout;
use crate::parser::Program;

#[derive(Debug)]
pub enum ComponentInspectError {
    Layout(layout::LayoutError),
    Io(io::Error),
}

impl fmt::Display for ComponentInspectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Layout(error) => formatter.write_str(&error.message),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ComponentInspectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Layout(_) => None,
            Self::Io(error) => Some(error),
        }
    }
}

impl From<io::Error> for ComponentInspectError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn write_components(
    output: &mut impl Write,
    program: &Program,
) -> Result<(), ComponentInspectError> {
    for (index, component) in program.components.iter().enumerate() {
        if index > 0 {
            writeln!(output)?;
        }

        let component_layout =
            layout::compute_component_layout(component).map_err(ComponentInspectError::Layout)?;
        writeln!(
            output,
            "component {}.{}",
            program.world.name, component.name
        )?;
        writeln!(output, "  size: {}", component_layout.size)?;
        writeln!(output, "  align: {}", component_layout.align)?;
        writeln!(output, "  fields:")?;

        for field in component_layout.fields {
            writeln!(
                output,
                "    {}: {} @ {}",
                field.name, field.type_name, field.offset
            )?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_checked_u64_component_layouts() {
        let source = "world Demo component Empty {} component Value { live: bool count: i32 } startup { exit 0 }";
        let tokens = crate::lexer::lex(source).expect("fixture lexes");
        let program = crate::parser::parse_program(&tokens).expect("fixture parses");
        let mut output = Vec::new();
        write_components(&mut output, &program).expect("inspection writes");
        assert_eq!(
            String::from_utf8(output).expect("inspection is UTF-8"),
            "component Demo.Empty\n  size: 0\n  align: 1\n  fields:\n\ncomponent Demo.Value\n  size: 8\n  align: 4\n  fields:\n    live: bool @ 0\n    count: i32 @ 4\n"
        );
    }
}
