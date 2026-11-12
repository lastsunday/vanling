use super::*;

fn landing() -> DeviceManager {
    DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    })
}

#[test]
fn one_input_intent_is_the_two_steps_it_replaces() {
    let mut direct = landed(landing());
    let mut manual = direct.clone();

    let operation = tap();
    let command = direct.apply_intent(Intent::Input(operation));

    manual.apply_input(operation);
    let action = translate(&operation, manual.state_ref());
    let expected = manual.apply_business(action);

    assert_eq!(
        command, expected,
        "the command is the one the recipe produced"
    );
    assert_eq!(
        direct.state(),
        manual.state(),
        "writing before translating leaves the same record"
    );
}

#[test]
fn one_business_intent_answers_like_apply_business() {
    let mut direct = landing();
    let mut manual = direct.clone();

    let intent = Intent::Business(BusinessIntent::Navigation(NavigationIntent::SplashOver));
    assert_eq!(
        direct.apply_intent(intent),
        manual.apply_business(BusinessIntent::Navigation(NavigationIntent::SplashOver)),
        "a business intent is apply_business and nothing else"
    );
    assert_eq!(direct.state(), manual.state());
    assert_ne!(direct.state().page(), Page::Splash, "the splash landed");
}
