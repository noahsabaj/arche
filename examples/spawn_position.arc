world Demo

component Position {
    x: f32
    y: f32
}

startup {
    spawn {
        Position { x: 1.0, y: 2.0 }
    }

    exit 0
}
