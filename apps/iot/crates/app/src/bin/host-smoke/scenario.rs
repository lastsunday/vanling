//! The walk a finger and a radio would take, run against the fake board.

use std::sync::atomic::Ordering;

use embassy_time::{Duration, Timer};
use iot_app::runtime::workers::NETWORK;
use iot_app::shared::intent::{ActionIntent, Confirm, Intent, NetworkPageIntent};
use iot_app::shared::keyboard::Key;
use iot_app::shared::light::PALETTE;
use iot_app::state::{NetworkCommand, Page};
use iot_core::drivers::audio::dbfs;
use iot_core::drivers::network::{
    AccessPoint, ApList, Credentials, JoinRequest, SavedNetworks, Ssid,
};

use super::board::*;

/// Takes everything the coordinator handed the network worker. Nothing consumes these
/// commands on the host, so this is where a command is read rather than acted on.
fn take_commands() -> Vec<NetworkCommand> {
    let mut taken = Vec::new();
    while let Some(command) = NETWORK.commands.try_receive() {
        taken.push(command);
    }
    taken
}

pub(super) async fn scenario() {
    Timer::after(Duration::from_millis(200)).await;
    assert!(
        !PAINTED.lock().unwrap().is_empty(),
        "boot breath did not paint any frame"
    );

    Timer::after(Duration::from_millis(3_200)).await;

    PRESSED[0].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(600)).await;
    PRESSED[0].store(false, Ordering::SeqCst);

    let mut attempts = 0;
    while !painted(0, PALETTE[0]) && attempts < 500 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        painted(0, PALETTE[0]),
        "long press did not paint palette[0]"
    );

    let rest0 = colors_on(0);
    let count1 = paints_on(1);
    PRESSED[1].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(120)).await;
    PRESSED[1].store(false, Ordering::SeqCst);

    let mut attempts = 0;
    while paints_on(1) == count1 && attempts < 500 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        paints_on(1) > count1,
        "button 1's click did not paint its own surface"
    );
    assert_eq!(
        colors_on(0),
        rest0,
        "button 1 must not touch surface 0's solid color"
    );

    triple_click().await;

    let mut attempts = 0;
    while !page_seen(Page::Attitude) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        page_seen(Page::Attitude),
        "triple click did not reach the motion-enabled page"
    );

    triple_click().await;

    attempts = 0;
    while !page_seen(Page::Audio) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        page_seen(Page::Audio),
        "the capture-enabled page was never reached"
    );
    assert!(
        AUDIO_POLLS.load(Ordering::SeqCst) > 0,
        "the capture source was never polled"
    );
    // The page can be on screen before the state layer commits a column: poll and commit are
    // separate passes, so wait for the column this assertion is about.
    attempts = 0;
    while AUDIO_COLUMNS.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        AUDIO_COLUMNS.load(Ordering::SeqCst) > 0,
        "no capture poll ever reached the state layer"
    );
    assert_eq!(
        AUDIO_RESTARTS.load(Ordering::SeqCst),
        usize::from(HOST_RESTARTS),
        "the capture's repair count did not reach the panel's diagnostics"
    );
    assert_eq!(
        AUDIO_PEAK_DB.load(Ordering::SeqCst),
        isize::from(dbfs(HOST_LOUD.unsigned_abs())),
        "the capture's level did not reach the panel as a decibel reading"
    );
    assert!(
        AUDIO_DBA_LSB.load(Ordering::SeqCst) > 0,
        "the capture's A-weighted level did not reach the panel's diagnostics"
    );

    triple_click().await;

    attempts = 0;
    while !page_seen(Page::Speaker) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        page_seen(Page::Speaker),
        "the speaker-enabled page was never reached"
    );

    click(1).await;

    attempts = 0;
    while !phase_playing() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        phase_playing(),
        "a tap on the speaker page never started a sound"
    );
    attempts = 0;
    while PLAYED.lock().unwrap().is_empty() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    let played = PLAYED.lock().unwrap().clone();
    assert_eq!(
        played.len(),
        1,
        "a tap did not reach the driver exactly once"
    );
    assert!(
        played[0] > 0,
        "the driver was handed a source with no sound in it"
    );
    attempts = 0;
    while PLAYBACK_PLAYS.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        PLAYBACK_PLAYS.load(Ordering::SeqCst),
        1,
        "the panel's play count did not follow the worker's confirmation"
    );

    click(1).await;

    attempts = 0;
    while PLAYBACK_DROPPED.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        PLAYBACK_DROPPED.load(Ordering::SeqCst) > 0,
        "a tap over a sounding one was neither dropped nor played"
    );
    assert_eq!(
        PLAYED.lock().unwrap().len(),
        1,
        "a dropped tap still reached the driver"
    );

    attempts = 0;
    while !phase_idle_after_playing() && attempts < 500 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        phase_idle_after_playing(),
        "the page never left Playing after the driver reported the sound done"
    );

    PRESSED[0].store(true, Ordering::SeqCst);
    Timer::after(Duration::from_millis(700)).await;
    PRESSED[0].store(false, Ordering::SeqCst);

    attempts = 0;
    while !PLAYBACK_EVER_MUTED.load(Ordering::SeqCst) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        PLAYBACK_EVER_MUTED.load(Ordering::SeqCst),
        "a long press did not latch the speaker mute"
    );
    attempts = 0;
    while MUTED.lock().unwrap().is_empty() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        MUTED.lock().unwrap().as_slice(),
        &[true],
        "the mute did not reach the driver as a single latch"
    );

    triple_click().await;

    attempts = 0;
    while !page_seen(Page::Camera) && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        page_seen(Page::Camera),
        "the camera-enabled page was never reached"
    );

    attempts = 0;
    while CAMERA_PAINTED.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        CAMERA_PAINTED.load(Ordering::SeqCst) > 0,
        "the camera page was reached but no frame ever reached the surface"
    );

    let painted_before = PAINTED.lock().unwrap().len();
    let released_before = RELEASED.load(Ordering::SeqCst);
    let pages_before = PAGES.lock().unwrap().len();
    triple_click().await;
    attempts = 0;
    while !PAGES.lock().unwrap()[pages_before..]
        .iter()
        .any(|page| *page != Page::Camera)
        && attempts < 100
    {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        PAGES.lock().unwrap()[pages_before..]
            .iter()
            .any(|page| *page != Page::Camera),
        "the page cycle did not come back round from the camera page"
    );

    assert!(
        RELEASED.load(Ordering::SeqCst) > released_before,
        "the camera page was left without the buffer being handed back"
    );

    // Both halves of the deadlock: the release above makes a fill reachable again, and a refused
    // fill here is what the panel showed — the last camera frame left on screen for good.
    attempts = 0;
    while PAINTED.lock().unwrap().len() == painted_before && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        PAINTED.lock().unwrap().len() > painted_before,
        "no fill reached the surface after leaving the camera page"
    );

    assert!(
        page_seen(Page::Network),
        "the link's page was never reached by the cycle"
    );

    let list = ApList::from_ranked(&[AccessPoint {
        ssid: Ssid::from_octets(b"neighbour"),
        bssid: [0x02, 0x00, 0x00, 0x00, 0x00, 0x01],
        channel: 11,
        rssi: -55,
        secured: true,
    }]);
    NETWORK.scan.publish(&list);

    attempts = 0;
    while NETWORK_APS.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        NETWORK_APS.load(Ordering::SeqCst),
        1,
        "a scan published through the link never reached the panel's snapshot"
    );

    let send = |intent: ActionIntent| {
        let _ = iot_app::runtime::feature::input::INTENT_BUS.try_send(Intent::Action(intent));
    };

    send(ActionIntent::NetworkDraft(NetworkPageIntent::Choose(
        list.get(0).expect("a row to pick"),
    )));
    Timer::after(Duration::from_millis(100)).await;

    let mut at_ms = 100;
    for key in [
        Key::Char(b'4'),
        Key::Char(b'4'),
        Key::Char(b'2'),
        Key::Shift,
        Key::Char(b'8'),
    ] {
        send(ActionIntent::NetworkDraft(NetworkPageIntent::Key {
            key,
            at_ms,
        }));
        at_ms += 100;
    }
    attempts = 0;
    while NETWORK_TYPED.load(Ordering::SeqCst) < 3 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        NETWORK_TYPED.load(Ordering::SeqCst),
        3,
        "the keyboard's draft never reached the panel's snapshot"
    );

    send(ActionIntent::NetworkDraft(NetworkPageIntent::Key {
        key: Key::Connect,
        at_ms,
    }));
    attempts = 0;
    let mut handed = None;
    while handed.is_none() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        for command in take_commands() {
            if let NetworkCommand::Join(asked) = command {
                handed = Some(asked);
            }
        }
        attempts += 1;
    }
    let handed = handed.expect("connect never reached the network worker");
    let JoinRequest::Typed(handed) = handed else {
        panic!("connect handed over a row's name rather than the passphrase it was given");
    };
    assert_eq!(
        handed.ssid.as_bytes(),
        b"neighbour",
        "the credential did not carry the row that was picked"
    );
    assert_eq!(
        handed.password.as_bytes(),
        b"haT",
        "the credential did not carry what was typed"
    );

    send(ActionIntent::NetworkDraft(NetworkPageIntent::Key {
        key: Key::Cancel,
        at_ms,
    }));
    Timer::after(Duration::from_millis(100)).await;

    let mut store = SavedNetworks::empty();
    store.remember(
        &Credentials::from_text("neighbour", "haT").expect("a credential the radio takes"),
    );
    let mut marked = list;
    let marks = store.mask(&marked);
    marked.mark_saved(marks);
    NETWORK.scan.publish(&marked);
    attempts = 0;
    while NETWORK_SAVED.load(Ordering::SeqCst) == 0 && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert_eq!(
        NETWORK_SAVED.load(Ordering::SeqCst),
        1,
        "a marked list published through the link never reached the panel's snapshot"
    );

    send(ActionIntent::NetworkDraft(NetworkPageIntent::ScanNow));
    attempts = 0;
    let mut scanned = false;
    while !scanned && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        for command in take_commands() {
            if matches!(command, NetworkCommand::ScanNow) {
                scanned = true;
            }
        }
        attempts += 1;
    }
    assert!(
        scanned,
        "the strip's request to look around never reached the network worker"
    );

    send(ActionIntent::NetworkDraft(NetworkPageIntent::AskForget(
        list.get(0).expect("a row to ask about"),
    )));
    Timer::after(Duration::from_millis(100)).await;
    send(ActionIntent::NetworkDraft(NetworkPageIntent::Answer(
        Confirm::Forget(list.get(0).expect("the row the question named")),
    )));
    attempts = 0;
    let mut forgotten = None;
    while forgotten.is_none() && attempts < 100 {
        Timer::after(Duration::from_millis(10)).await;
        for command in take_commands() {
            if let NetworkCommand::Forget(ssid) = command {
                forgotten = Some(ssid);
            }
        }
        attempts += 1;
    }
    match forgotten {
        Some(ssid) => assert_eq!(
            ssid.as_bytes(),
            b"neighbour",
            "the forget named a network other than the row that was held"
        ),
        None => panic!("the hold's answer never reached the network worker"),
    }

    std::process::exit(0);
}
