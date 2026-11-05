use voice_abi::Model;
use voice_protocol::StreamId;

/// Where the all-apps microphone is in its life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AllAppsState {
    #[default]
    Off,
    Opening,
    Running(StreamId),
    /// Opening failed; tried again once the settings change.
    Failed,
}

/// What brings the all-apps microphone in line with the settings.
#[derive(Debug, PartialEq, Eq)]
pub enum AllAppsStep {
    Open(Model),
    Close(StreamId),
    Switch(StreamId, Model),
    Wait,
}

/// The step from `state`, running `running`, towards `wanted` (`None`: no all-apps microphone).
pub fn next_step(
    state: AllAppsState,
    running: Option<Model>,
    wanted: Option<Model>,
) -> AllAppsStep {
    match (state, wanted) {
        (AllAppsState::Running(stream), None) => AllAppsStep::Close(stream),
        (AllAppsState::Running(stream), Some(model)) if running != Some(model) => {
            AllAppsStep::Switch(stream, model)
        }
        (AllAppsState::Off, Some(model)) => AllAppsStep::Open(model),
        _ => AllAppsStep::Wait,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: StreamId = StreamId(7);

    #[test]
    fn opens_when_turned_on() {
        assert_eq!(
            next_step(AllAppsState::Off, None, Some(Model::Standard)),
            AllAppsStep::Open(Model::Standard)
        );
        assert_eq!(next_step(AllAppsState::Off, None, None), AllAppsStep::Wait);
    }

    #[test]
    fn closes_when_turned_off() {
        assert_eq!(
            next_step(AllAppsState::Running(STREAM), Some(Model::Standard), None),
            AllAppsStep::Close(STREAM)
        );
    }

    #[test]
    fn switches_when_the_enhancement_changes() {
        assert_eq!(
            next_step(
                AllAppsState::Running(STREAM),
                Some(Model::Standard),
                Some(Model::Light)
            ),
            AllAppsStep::Switch(STREAM, Model::Light)
        );
        assert_eq!(
            next_step(
                AllAppsState::Running(STREAM),
                Some(Model::Light),
                Some(Model::Light)
            ),
            AllAppsStep::Wait
        );
    }

    #[test]
    fn waits_for_an_open_in_flight_and_after_a_failure() {
        for state in [AllAppsState::Opening, AllAppsState::Failed] {
            assert_eq!(
                next_step(state, None, Some(Model::Standard)),
                AllAppsStep::Wait
            );
            assert_eq!(next_step(state, None, None), AllAppsStep::Wait);
        }
    }
}
