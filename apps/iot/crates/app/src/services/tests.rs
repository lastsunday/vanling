//! The page golden table: every page, with the debug badge on and off, is one fingerprint.
//!
//! The dispatch under test is [`paint`](crate::services::paint), so a page whose view moved still
//! has to draw exactly the picture the table recorded. Regenerate only for an intended pixel
//! change.

use alloc::vec;
use alloc::vec::Vec;

use crate::services::{Services, descriptor, paint};
use crate::shared::i18n::{Lang, Msg};
use crate::shared::intent::{
    BusinessIntent, MenuIntent, NavigationIntent, SettingsIntent, TextIntent, WidgetsIntent,
};
use crate::shared::model::DeviceState;
use crate::shared::route::PageId;
use crate::shared::snapshot::FrameSnapshot;
use crate::shared::surface::Panel;
use crate::state::test_support::{BREATH, cycled, op, state_on, translate};
use iot_core::drivers::board::Backed;
use iot_core::drivers::input::{ButtonEvent, InputEvent};
use iot_core::drivers::motion::{MotionCapabilities, MotionSample};
use iot_core::light::{Rgb, rgb565};
use iot_core::ui::panel::{PANEL_COLUMNS, PANEL_ROWS};
use iot_core::ui::{Canvas, Screen, ThemeId};

const WIDTH: usize = PANEL_COLUMNS;
const HEIGHT: usize = PANEL_ROWS;

fn render(page: PageId, debug: bool) -> Vec<u8> {
    render_snapshot(&page_snapshot(page), debug)
}

/// The snapshot the golden table and the cache test both draw: the menu pinned with every
/// capability wired, so its golden is the product's grid, not the bare board's greyed one.
/// The bare board is covered by the page's own tests.
fn page_snapshot(page: PageId) -> FrameSnapshot {
    let backed = if page == PageId::Menu {
        Backed {
            motion: Some(MotionCapabilities::EMPTY),
            audio: true,
            playback: true,
            camera: true,
            network: true,
        }
    } else {
        Backed::default()
    };
    FrameSnapshot {
        page,
        backed,
        pages: Services::default().snapshot(),
        ..FrameSnapshot::default()
    }
}

fn render_snapshot(snapshot: &FrameSnapshot, debug: bool) -> Vec<u8> {
    let page = snapshot.page;
    let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
    // The device fills the ground before painting; a light page is paper, not a black frame.
    let ground = descriptor(page).ground().unwrap_or_else(|| {
        crate::shared::ui::theme::Theme::of(snapshot.theme)
            .ground
            .rgb888()
    });
    let packed = rgb565(ground).to_be_bytes();
    for pixel in frame.chunks_exact_mut(2) {
        pixel.copy_from_slice(&packed);
    }
    {
        let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, Some(ground));
        let mut panel = Panel::new(
            canvas,
            Screen {
                instance: 0,
                fps: 60,
                debug,
                spl_offset_db: 100,
                probe: None,
                theme: snapshot.theme,
            },
        );
        paint(page, snapshot, &mut panel);
    }
    frame
}

/// A full-frame FNV-1a, used only by the golden table: the sampling fingerprint the camera
/// page compares frames with is deliberately sparse and would miss small changes.
fn fingerprint(frame: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for &byte in frame {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
    }
    hash
}

/// One line per page and debug state. Regenerate with `PAGE_GOLDEN_UPDATE=1`.
const GOLDEN: [(&str, bool, u32); 34] = [
    ("menu", false, 0xa7812a81),
    ("menu", true, 0xe3a13e31),
    ("input", false, 0xc18e7dc5),
    ("input", true, 0xb334d787),
    ("attitude", false, 0x5d452faf),
    ("attitude", true, 0xf08f755f),
    ("audio", false, 0xb871c695),
    ("audio", true, 0x3c2ac445),
    ("speaker", false, 0x166de255),
    ("speaker", true, 0xfb733805),
    ("camera", false, 0x33d824fb),
    ("camera", true, 0x133e02ab),
    ("network", false, 0x216eb4b9),
    ("network", true, 0x2b62f669),
    ("splash", false, 0xaa0aa601),
    ("splash", true, 0x3f0b3db1),
    ("env", false, 0x1f8598cd),
    ("env", true, 0xa877747d),
    ("text", false, 0x1f4c862b),
    ("text", true, 0xbe98e7db),
    ("widgets", false, 0x59bbd7f7),
    ("widgets", true, 0xc1c211a7),
    ("advanced", false, 0x7acb3fae),
    ("advanced", true, 0xd9abb15e),
    ("feedback", false, 0x47bacd74),
    ("feedback", true, 0x48872b24),
    ("extended", false, 0x1b5c4a5d),
    ("extended", true, 0xa10b2e0d),
    ("data", false, 0xa75b191e),
    ("data", true, 0x5404eace),
    ("list", false, 0x52deda55),
    ("list", true, 0x37e43005),
    ("lighting", false, 0x2c929a49),
    ("lighting", true, 0x3375f881),
];

#[test]
fn every_page_draws_the_picture_the_golden_table_expects() {
    let update = std::env::var_os("PAGE_GOLDEN_UPDATE").is_some();
    for page in PageId::ALL {
        for debug in [false, true] {
            let actual = fingerprint(&render(page, debug));
            if update {
                std::println!("    (\"{}\", {debug}, 0x{actual:08x}),", page.path());
                continue;
            }
            let expected = GOLDEN
                .iter()
                .find(|(path, state, _)| *path == page.path() && *state == debug)
                .map(|(_, _, hash)| *hash)
                .expect("every page has a golden row; run with PAGE_GOLDEN_UPDATE=1");
            assert_eq!(
                actual,
                expected,
                "{} (debug={debug}) changed; regenerate only for an intended pixel change",
                page.path()
            );
        }
    }
}

#[test]
fn a_theme_switch_reaches_the_pixels() {
    for page in PageId::ALL {
        // Input prints nothing on a release build; Camera and Network keep their own black
        // ground and text by inverting it, so the palette does not reach them.
        if matches!(page, PageId::Input | PageId::Camera | PageId::Network) {
            continue;
        }
        let dark = page_snapshot(page);
        let mut light = dark;
        light.theme = ThemeId::Light;
        assert_ne!(
            render_snapshot(&light, false),
            render_snapshot(&dark, false),
            "{page:?} ignores the theme"
        );
    }
}

#[test]
fn a_language_switch_reaches_the_pixels() {
    // Input and Text print readout keys and the face's own specimen, which do not move.
    let speaking = [
        PageId::Splash,
        PageId::Menu,
        PageId::Widgets,
        PageId::Env,
        PageId::Attitude,
        PageId::Audio,
        PageId::Speaker,
        PageId::Camera,
        PageId::Network,
        PageId::Advanced,
        PageId::Feedback,
        PageId::Extended,
        PageId::Data,
        PageId::List,
        PageId::Lighting,
    ];
    for page in speaking {
        let render = |lang| {
            render_snapshot(
                &FrameSnapshot {
                    page,
                    lang,
                    ..FrameSnapshot::default()
                },
                false,
            )
        };
        assert_ne!(
            render(Lang::En),
            render(Lang::ZhHans),
            "{page:?} reads its words from the snapshot"
        );
    }
}

#[test]
fn a_language_switch_writes_the_state() {
    let mut services = Services::default();
    let mut state = DeviceState::default();
    services.apply(
        &BusinessIntent::Settings(SettingsIntent::SetLang { lang: Lang::ZhHans }),
        &mut state,
    );
    assert_eq!(state.lang, Lang::ZhHans);
}

#[test]
fn a_menu_choice_opens_its_page_without_geometry() {
    let mut services = Services::default();
    let mut state = DeviceState::default();
    services.apply(
        &BusinessIntent::Menu(MenuIntent::Choose {
            page: PageId::Widgets.code(),
        }),
        &mut state,
    );
    assert_eq!(state.page(), PageId::Widgets);
}

#[test]
fn every_word_the_panel_prints_has_a_bitmap_glyph() {
    let face = iot_core::ui::font::Face::new();
    for msg in Msg::ALL {
        for lang in Lang::ALL {
            let run = msg.bytes(*lang);
            let text = core::str::from_utf8(run).expect("every word is UTF-8");
            for ch in text.chars() {
                assert!(
                    face.has(ch),
                    "{msg:?} in {lang:?} has no glyph for {ch:?}; rerun `iot-tool font`"
                );
            }
        }
    }
}

#[test]
fn a_text_scroll_moves_one_region_and_its_thumb() {
    let mut services = Services::default();
    let mut state = DeviceState::default();
    let before = services.snapshot();
    assert_eq!(before.text.areas[0].offset, 0);
    assert_eq!(before.text.areas[1].offset, 0);
    services.apply(
        &BusinessIntent::Text(TextIntent::Scroll {
            area: 1,
            lines: 3,
            speed: 0,
        }),
        &mut state,
    );
    let after = services.snapshot();
    assert_eq!(after.text.areas[0], before.text.areas[0]);
    assert!(after.text.areas[1].offset > 0, "the swiped region moved");

    let frame = |pages| {
        render_snapshot(
            &FrameSnapshot {
                page: PageId::Text,
                pages,
                ..FrameSnapshot::default()
            },
            false,
        )
    };
    let (before_frame, after_frame) = (frame(before), frame(after));
    assert_ne!(before_frame, after_frame, "the offset reaches the picture");

    let thumb_top = |frame: &[u8]| {
        (before.text.areas[1].rect.y as usize..HEIGHT)
            .find(|&y| {
                let at = (y * WIDTH + 318) * 2;
                frame[at] != 0 || frame[at + 1] != 0
            })
            .expect("a thumb beside the swiped region")
    };
    assert!(
        thumb_top(&after_frame) > thumb_top(&before_frame),
        "the thumb follows the offset"
    );
}

/// Writes every page as a 24-bit BMP under `PAGE_DUMP_DIR`: the golden table proves a picture
/// is unchanged, not that it is right.
#[test]
fn a_widgets_scroll_moves_the_gallery_and_only_that() {
    let mut services = Services::default();
    let mut state = DeviceState::default();
    let before = services.snapshot();
    services.apply(
        &BusinessIntent::Widgets(WidgetsIntent::Scroll {
            area: 0,
            lines: 3,
            speed: 0,
        }),
        &mut state,
    );
    let after = services.snapshot();
    assert!(after.widgets.area.offset > 0, "the gallery moved");

    let frame = |pages| {
        render_snapshot(
            &FrameSnapshot {
                page: PageId::Widgets,
                pages,
                ..FrameSnapshot::default()
            },
            false,
        )
    };
    assert_ne!(
        frame(before),
        frame(after),
        "the offset reaches the picture"
    );

    let shift = descriptor(PageId::Widgets)
        .scroll_shift(
            &FrameSnapshot {
                page: PageId::Widgets,
                pages: before,
                ..FrameSnapshot::default()
            },
            &FrameSnapshot {
                page: PageId::Widgets,
                pages: after,
                ..FrameSnapshot::default()
            },
        )
        .expect("a pure scroll");
    assert_eq!(shift.shifts()[0], after.widgets.area.offset);
}

#[test]
fn dump_every_page_when_asked() {
    let Some(dir) = std::env::var_os("PAGE_DUMP_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    for &theme in ThemeId::ALL {
        let suffix = if theme == ThemeId::Light {
            "-light"
        } else {
            ""
        };
        for page in PageId::ALL {
            let mut snapshot = page_snapshot(page);
            snapshot.theme = theme;
            std::fs::write(
                dir.join(format!("{}{suffix}.bmp", page.path())),
                bmp(&render_snapshot(&snapshot, false)),
            )
            .unwrap();
        }
    }
    std::fs::write(
        dir.join("attitude-motion.bmp"),
        bmp(&render_snapshot(&motion(), false)),
    )
    .unwrap();
    let mut light_motion = motion();
    light_motion.theme = ThemeId::Light;
    std::fs::write(
        dir.join("attitude-motion-light.bmp"),
        bmp(&render_snapshot(&light_motion, false)),
    )
    .unwrap();
    let mut services = Services::default();
    let mut state = DeviceState::default();
    services.apply(
        &BusinessIntent::Widgets(WidgetsIntent::Scroll {
            area: 0,
            lines: 40,
            speed: 0,
        }),
        &mut state,
    );
    let scrolled = services.snapshot();
    std::fs::write(
        dir.join("widgets-scrolled.bmp"),
        bmp(&render_snapshot(
            &FrameSnapshot {
                page: PageId::Widgets,
                pages: scrolled,
                ..FrameSnapshot::default()
            },
            false,
        )),
    )
    .unwrap();
}

/// A page with numbers in every reading: the state the golden table's empty snapshot cannot
/// show, and the one the dial is drawn from.
fn motion() -> FrameSnapshot {
    FrameSnapshot {
        page: PageId::Attitude,
        motion: Some(MotionSample {
            raw_accel: [-1700, 400, -900],
            raw_gyro: [150, -80, 40],
            accel_mg: [-355, 120, -940],
            gyro_dps_x10: [-1799, 870, 25],
            tilt_deg_x10: [-1799, 640, 0],
            status: 3,
            tap_residual_mg: -32768,
            gravity_deviation_mg: 12345,
            shake_residual_mg: -7,
            valid: true,
        }),
        ..FrameSnapshot::default()
    }
}

#[test]
fn the_attitude_page_with_numbers_draws_the_recorded_picture() {
    let actual = fingerprint(&render_snapshot(&motion(), false));
    if std::env::var_os("PAGE_GOLDEN_UPDATE").is_some() {
        std::println!("attitude-motion 0x{actual:08x}");
        return;
    }
    assert_eq!(
        actual, 0xf94b_504b,
        "regenerate only for an intended pixel change"
    );
}

fn bmp(frame: &[u8]) -> Vec<u8> {
    const HEADER: usize = 54;
    let row = WIDTH * 3;
    let size = HEADER + row * HEIGHT;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(HEADER as u32).to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(WIDTH as i32).to_le_bytes());
    out.extend_from_slice(&(HEIGHT as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..HEIGHT).rev() {
        for x in 0..WIDTH {
            let offset = (y * WIDTH + x) * 2;
            let packed = u16::from_be_bytes([frame[offset], frame[offset + 1]]);
            let (r, g, b) = (
                u32::from(packed >> 11 & 0x1f) * 255 / 31,
                u32::from(packed >> 5 & 0x3f) * 255 / 63,
                u32::from(packed & 0x1f) * 255 / 31,
            );
            out.extend_from_slice(&[b as u8, g as u8, r as u8]);
        }
    }
    out
}

#[test]
fn every_page_paints_something_and_does_it_the_same_way_twice() {
    for page in PageId::ALL {
        let first = render(page, true);
        let second = render(page, true);
        assert!(
            first.iter().any(|byte| *byte != 0),
            "{page:?} painted nothing"
        );
        assert_eq!(first, second, "{page:?} is not deterministic");
    }
}

#[test]
fn every_page_answers_every_render_question() {
    for page in PageId::ALL {
        assert_eq!(
            descriptor(page).ground(),
            matches!(page, PageId::Camera | PageId::Network).then_some(Rgb(0, 0, 0)),
            "{page:?} grounds"
        );
        assert_eq!(
            descriptor(page).camera_frames(),
            page == PageId::Camera,
            "{page:?} frames"
        );
    }
}

#[test]
fn a_button_hold_goes_home_from_every_page_that_keeps_it() {
    let services = Services::default();
    for page in cycled() {
        assert_eq!(
            translate(
                &op(InputEvent::Button(ButtonEvent::LongPress)),
                &state_on(page, BREATH),
                &services
            ),
            Some(BusinessIntent::Navigation(NavigationIntent::Go(
                PageId::Menu.code()
            ))),
            "{page:?} kept the hold"
        );
    }
}
