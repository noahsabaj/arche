world Arena

component Vitality {
    current: f32
    reserve: f32
}

component Regeneration {
    current_rate: f32
    reserve_rate: f32
    cap: f32
}

component Faction {
    id: i32
}

resource Tick {
    delta: f32
}

system Recover(
    tick: read Tick,
    units: query[mut Vitality, Regeneration]
) {
    for (vitality, regeneration) in units {
        vitality.current += regeneration.current_rate * tick.delta
        vitality.reserve += regeneration.reserve_rate * tick.delta
    }
}

schedule Step {
    run Recover
}

startup {
    resource Tick { delta: 0.5 }

    spawn {
        Vitality { current: 10.0, reserve: 100.0 }
        Regeneration { current_rate: 2.0, reserve_rate: 4.0, cap: 120.0 }
        Faction { id: 1 }
    }

    spawn {
        Vitality { current: 20.0, reserve: 200.0 }
        Regeneration { current_rate: 4.0, reserve_rate: 6.0, cap: 230.0 }
        Faction { id: 2 }
    }

    spawn {
        Vitality { current: 30.0, reserve: 300.0 }
        Regeneration { current_rate: 6.0, reserve_rate: 8.0, cap: 340.0 }
        Faction { id: 3 }
    }

    spawn {
        Vitality { current: 40.0, reserve: 400.0 }
        Faction { id: 4 }
    }

    spawn {
        Vitality { current: 50.0, reserve: 500.0 }
        Faction { id: 5 }
    }

    run Step
    exit 0
}
