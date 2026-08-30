use package::shared::Agent;
use package::shared::Episode;
use package::shared::Done;

pub world TrainingWorld {
    init {
        resource Episode = Episode { index: 0u64, score: 0i64 };
        spawn { Agent { id: 0i64 }, Active };
    }
}

pub tag Active;

pub system ResetSystem(
    episode: mut Episode,
    agents: query [mut Agent],
    cmd: commands,
) requires {} throws {} {
    episode.index = 0u64;
    episode.score = 0i64;
    for agent in agents {
        agent.id = 0i64;
    }
    cmd.spawn { Active };
}

pub system StepSystem(
    episode: mut Episode,
    agents: query [Agent, !Done],
) requires {} throws { package::shared::EnvironmentError } {
    for agent in agents {
        episode.score += agent.id;
    }
}

pub system SelfPlaySystem(
    episode: read Episode,
    agents: query [Agent],
) requires {} throws {} {
    for agent in agents {
        let _ = (episode.index, agent.id);
    }
}

pub schedule Reset {
    run package::training::ResetSystem;
}

pub schedule Step {
    run package::training::StepSystem;
}

pub schedule SelfPlay {
    run package::training::SelfPlaySystem;
}

pub schedule Evaluation {
    run package::training::ResetSystem;
    run package::training::StepSystem;
    run package::training::SelfPlaySystem;
}
