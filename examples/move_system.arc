world Demo

resource Time {
    delta: f32
}

system Move(time: read Time) {
}

startup {
    exit 0
}
