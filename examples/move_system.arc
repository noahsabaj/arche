world Demo

component Position {
    x: f32
    y: f32
}

component Velocity {
    x: f32
    y: f32
}

resource Time {
    delta: f32
}

system Move(
    time: read Time,
    movers: query[mut Position, Velocity]
) {
    time.delta
    Position.x
    Position.y
    Velocity.x
    Velocity.y
}

startup {
    exit 0
}
