world Demo

component Position {
    x: f32
    y: f32
}

system Bad(
    a: query[mut Position],
    b: query[Position]
) {
}

startup {
    exit 0
}
