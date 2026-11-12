use crate::shared::intent::LightAction;

use super::*;

#[test]
fn a_light_intent_moves_the_surface_it_names() {
    let mut manager = DeviceManager::new();
    manager.apply_to(0, LightState::Off);
    let lit = manager.apply_business(BusinessIntent::Light {
        instance: 0,
        action: LightAction::Cycle,
    });
    assert!(lit.is_none(), "the cycle reports no command");
    assert!(
        matches!(manager.light_state(), LightState::Breath(_)),
        "off cycles into breathing"
    );
    manager.apply_business(BusinessIntent::Light {
        instance: 0,
        action: LightAction::Step,
    });
    assert_ne!(
        manager.light_state(),
        LightState::default(),
        "a step walks the mode it lands in"
    );
    manager.apply_business(BusinessIntent::Light {
        instance: 7,
        action: LightAction::Cycle,
    });
    assert!(
        matches!(manager.light_state_at(1), LightState::Solid { .. }),
        "an out-of-range instance clamps to the last surface"
    );
}

#[test]
fn a_light_write_replaces_state_and_snapshot() {
    let mut manager = DeviceManager::new();
    manager.apply_to(0, LightState::Off);
    assert_eq!(manager.light_state(), LightState::Off);
    let solid = LightState::Solid {
        color: Rgb(3, 4, 5),
        brightness: LightState::SOLID_DEFAULT_BRIGHTNESS,
    };
    manager.apply_to(0, solid);
    assert_eq!(manager.light_state(), solid);
    assert_eq!(
        manager.state().lights[0],
        solid,
        "the snapshot mirrors the applied target"
    );
    manager.apply_to(7, LightState::Off);
    assert_eq!(manager.light_state_at(1), LightState::Off);
    manager.apply_business(BusinessIntent::Invalid);
    assert_eq!(manager.light_state(), solid, "an invalid intent is a no-op");
}

#[test]
fn a_link_report_lands_whole_and_an_unwired_board_reads_off() {
    let mut manager = DeviceManager::new();
    assert_eq!(manager.state().network.link.phase, LinkPhase::Off);

    let status = LinkStatus {
        phase: LinkPhase::Connected,
        ap: Some(AccessPoint {
            ssid: Ssid::from_octets(b"neighbour"),
            bssid: [0xaa; 6],
            channel: 11,
            rssi: -45,
            secured: true,
        }),
        address: Some(Ipv4::from_octets([192, 168, 1, 136])),
        ..LinkStatus::default()
    };
    manager.apply_business(BusinessIntent::Link(LinkIntent::Status(status)));
    assert_eq!(manager.state().network.link, status);
}
