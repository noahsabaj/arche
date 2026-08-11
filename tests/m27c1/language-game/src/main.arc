mod game;
mod shared;

pub use self::game::ArenaWorld;
use package::shared::GameError;

pub fn main(
    app: &mut App<ArenaWorld>,
    caps: Caps<Stdio, Udp>,
) requires { Stdio, Udp } throws { GameError } -> i32 {
    app.run(package::game::Frame, caps);
    0
}
