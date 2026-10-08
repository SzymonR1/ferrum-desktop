use ferrum_core::ContainerState;

#[test]
fn engine_state_names_are_classified_without_case_sensitivity() {
    assert_eq!(ContainerState::parse("RUNNING"), ContainerState::Running);
    assert_eq!(ContainerState::parse("exited"), ContainerState::Exited);
}

#[test]
fn unknown_engine_states_are_preserved_instead_of_mislabeled() {
    let state = ContainerState::parse("checkpointing");

    assert_eq!(state, ContainerState::Unknown("checkpointing".to_owned()));
    assert_eq!(state.label(), "checkpointing");
}
