world M26Closure

component Position {
    x: i32
    weight: f32
    active: bool
}

component Velocity {
    dx: i32
    factor: f32
}

component Empty {}

tag Enemy
tag Frozen

resource Config {
    step: i32
    scale: f32
    enabled: bool
}

resource Counter {
    value: i32
}

resource EmptyReady {}

resource EmptyPending {}

resource Scratch {
    value: i32
}

system Advance(
    config: read Config,
    counter: mut Counter,
    moving: query[mut Position, Velocity, Enemy, !Frozen]
) {
    let mut pass: i32 = 0
    let mut gate: bool = config.enabled

    while pass < config.step {
        if gate && pass < 2 {
            for (position, velocity, _) in moving {
                position.x = position.x + (velocity.dx << (pass & 31))
                position.weight = position.weight + velocity.factor * config.scale
                position.active = position.active == true
                counter.value = counter.value + 1
            }
        } else {
            counter.value = counter.value + 10
        }

        pass = pass + 1
        gate = !gate || pass == config.step
    }

    for (position, velocity, _) in moving {
        if position.weight >= velocity.factor {
            position.x += counter.value & 3
        } else {
            position.x = position.x - 1
        }
    }
}

system Normalize(
    first: read Counter,
    second: read Counter,
    candidates: query[mut Position, !Enemy]
) {
    let mut visits: i32 = 0
    for (position) in candidates {
        {
            position.x = position.x ^ (first.value | second.value)
            position.weight = -position.weight / 2.0
            position.active = position.active && true
            visits = visits + 1
        }
    }
    if visits == 0 {
        visits = ~visits
    } else {
        visits = visits % 4
    }
}

schedule Warmup {
    run Advance
    run Normalize
}

schedule Reverse {
    run Normalize
    run Advance
    run Advance
}

startup {
    let mut base: i32 = 1
    base = (base + 1) << 1

    resource Config {
        enabled: !false,
        scale: 0.25 + 0.75,
        step: base - 2
    }
    resource Counter { value: 0 }
    resource EmptyReady {}

    spawn {}

    spawn {
        Position { active: true, weight: 1.5, x: 4 + 1 }
        Velocity { factor: 0.5, dx: 2 }
        Enemy {}
        Empty {}
    }

    spawn {
        Position { x: 9, weight: 2.0, active: false }
        Velocity { dx: 3, factor: 1.0 }
        Enemy {}
        Frozen {}
    }

    spawn {
        Position { x: 7, weight: -4.0, active: true }
        Empty {}
    }

    run Warmup
    run Reverse

    let mut status: i32 = 40
    status = status + 7
    exit status
}
