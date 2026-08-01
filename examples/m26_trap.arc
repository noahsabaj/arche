world M26Trap

component Counter {
    value: i32
}

resource Denominator {
    value: i32
}

system Divide(
    denominator: read Denominator,
    counters: query[mut Counter]
) {
    for (counter) in counters {
        counter.value = counter.value + 1
        counter.value = counter.value / denominator.value
    }
}

schedule Main {
    run Divide
}

startup {
    resource Denominator { value: 0 }
    spawn { Counter { value: 41 } }
    run Main
    exit 70
}
