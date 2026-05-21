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
    for (pos, vel) in movers {
        pos.x += vel.x * time.delta
        pos.y += vel.y * time.delta
    }
}

schedule Main {
    run Move
}

startup {
    resource Time { delta: 1.0 }

    spawn {
        Position { x: 1.0, y: 2.0 }
        Velocity { x: 3.0, y: 4.0 }
    }

    spawn {
        Position { x: 10.0, y: 20.0 }
        Velocity { x: 1.0, y: 2.0 }
    }

    run Main
    exit 0
}
