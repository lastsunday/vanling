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

fn scanned(list: ApList, backed: Backed) -> Option<ApList> {
    let mut manager = DeviceManager::boot(Boot {
        backed,
        ..Default::default()
    });
    manager.apply_scan(list);
    manager.state().network.aps
}

#[test]
fn a_scan_list_is_kept_only_when_a_link_is_wired() {
    let list = ApList::from_ranked(&[AccessPoint {
        ssid: Ssid::from_octets(b"neighbour"),
        bssid: [1, 2, 3, 4, 5, 6],
        channel: 11,
        rssi: -55,
        secured: true,
    }]);
    assert_eq!(
        scanned(list, Backed::NONE),
        None,
        "a build with no link has no page to draw a list on"
    );
    assert_eq!(
        scanned(
            list,
            Backed {
                network: true,
                ..Backed::NONE
            }
        ),
        Some(list)
    );
}

#[test]
fn an_empty_scan_is_a_reading_not_an_absence() {
    let mut manager = DeviceManager::boot(Boot {
        backed: Backed {
            network: true,
            ..Backed::NONE
        },
        ..Default::default()
    });
    assert_eq!(manager.state().network.aps, None, "nothing has scanned yet");
    manager.apply_scan(ApList::empty());
    assert_eq!(
        manager.state().network.aps,
        Some(ApList::empty()),
        "a scan that found nothing is kept as an answer"
    );
}
