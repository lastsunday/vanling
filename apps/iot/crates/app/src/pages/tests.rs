//! The page golden table: every page, with the debug badge on and off, is one fingerprint.
//!
//! The dispatch under test is [`paint`](crate::pages::paint), so a page whose view moved still
//! has to draw exactly the picture the table recorded. Regenerate only for an intended pixel
//! change.

use alloc::vec;
use alloc::vec::Vec;

use crate::pages::{descriptor, paint};
use crate::shared::kit::painter::Painter;
use crate::shared::route::PageId;
use crate::shared::snapshot::FrameSnapshot;
use iot_core::light::Rgb;
use iot_core::ui::Canvas;

const WIDTH: usize = iot_core::overlay::PANEL_COLUMNS;
const HEIGHT: usize = iot_core::overlay::PANEL_ROWS;

fn render(page: PageId, debug: bool) -> Vec<u8> {
    let mut frame = vec![0u8; WIDTH * HEIGHT * 2];
    let snapshot = FrameSnapshot {
        page,
        ..FrameSnapshot::default()
    };
    {
        let canvas = Canvas::new(&mut frame, WIDTH, HEIGHT, descriptor(page).ground());
        let mut painter = Painter::new(canvas, 0, 60, debug, 100);
        paint(page, &snapshot, &mut painter);
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
const GOLDEN: [(&str, bool, u32); 16] = [
    ("ambient", false, 0xc18e_7dc5),
    ("ambient", true, 0xc104_7691),
    ("attitude", false, 0x3d8d_fc2f),
    ("attitude", true, 0xc9e3_2c21),
    ("audio", false, 0xe52b_405d),
    ("audio", true, 0x5dca_544f),
    ("speaker", false, 0x9370_7f03),
    ("speaker", true, 0x0302_faf5),
    ("camera", false, 0x311b_aeb1),
    ("camera", true, 0x9903_7aa3),
    ("network", false, 0x53df_7d41),
    ("network", true, 0x5c55_fd33),
    ("splash", false, 0x328a_4c63),
    ("splash", true, 0xe51b_5e55),
    ("env", false, 0xc8c9_1e4b),
    ("env", true, 0x80cd_763d),
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
fn the_camera_page_puts_the_fps_badge_where_every_page_does() {
    let mut expected = render(PageId::Camera, false);
    {
        let canvas = Canvas::new(&mut expected, WIDTH, HEIGHT, None);
        let mut painter = Painter::new(canvas, 0, 60, false, 100);
        painter.draw_fps();
    }
    assert_eq!(render(PageId::Camera, true), expected);
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
