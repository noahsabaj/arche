use package::shared::Position;
use package::shared::Disabled;

pub world ArenaWorld {
    init {
        resource Clock = Clock { tick: 0u64 };
        spawn { Position { x: 0.0f32, y: -0.0f32 }, Player };
    }
}

pub resource Clock {
    pub tick: u64,
}

pub tag Player;

pub fn recurse(value: i32) -> i32 {
    if value == 0 {
        0
    } else {
        recurse(value - 1)
    }
}

pub system Advance(
    clock: mut Clock,
    positions: query [mut Position, !Disabled],
    cmd: commands,
    udp: &Udp,
) requires { Udp } throws {} {
    clock.tick += 1u64;
    for position in positions {
        position.x += 1.0f32;
    }
    cmd.spawn { Player };
}

pub schedule Frame {
    run package::game::Advance;
}
