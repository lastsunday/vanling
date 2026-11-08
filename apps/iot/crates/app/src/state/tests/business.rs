use super::*;

#[test]
fn set_light_replaces_state_and_snapshot() {
    let mut manager = DeviceManager::new();
    manager.apply_action(set_light(0, LightState::Off));
    assert_eq!(manager.light_state(), LightState::Off);
    let solid = LightState::Solid {
        color: Rgb(3, 4, 5),
        brightness: LightState::SOLID_DEFAULT_BRIGHTNESS,
    };
    manager.apply_action(set_light(0, solid));
    assert_eq!(manager.light_state(), solid);
    assert_eq!(
        manager.state().lights[0],
        solid,
        "the snapshot mirrors the applied target"
    );
    manager.apply_action(set_light(7, LightState::Off));
    assert_eq!(manager.light_state_at(1), LightState::Off);
    manager.apply_action(ActionIntent::Invalid);
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
    manager.apply_action(ActionIntent::Link(LinkIntent::LinkReport(status)));
    assert_eq!(manager.state().network.link, status);
}
