//! Four nested quantities produce an image size, and the register constants below are one
//! each of them:
//!
//! * **Array** — 1600×1200, what the part physically has.
//! * **Read window** (`0x09`–`0x10`) — a rectangle of the array the part clocks out, plus
//!   the margin it charges to its own line timing.
//! * **Decimation ratio** (`0x99`) — how many read pixels are averaged into one emitted
//!   pixel ([`EXTRACT_RATIO`]), and which of them are kept (`0x9b`–`0xa2`).
//! * **Output window** (`0x95`–`0x98`) — the size that leaves the part.
//!
//! The field of view is the output width over the array width and no register moves it, so
//! the ratio only has to divide the array exactly — which the compile-time assertion on
//! [`EXTRACT_RATIO`] is there to keep true. What a wider *view* needs is a wider *output*,
//! and the panel's width caps that.
//!
//! The table is transcribed from `gc2145_settings.h` (which register, which bits) and
//! `gc2145.c` (which sequence, which value), so it is replayed as written rather than
//! sorted or split per page: one register number means different things on different
//! pages, and the list carries its own page selects for that reason.
//! `tests::power_on_table_pins_the_entries_that_decide_the_picture` pins the entries a
//! transcription error would otherwise hide. Measured behaviour of each register is in
//! `@/records/iot/camera.md`; the comments here state the current setting and why.

use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;

/// The part's SCCB address, in the 7-bit form the bus takes.
pub const GC2145_I2C_ADDR: u8 = 0x3c;

/// Chip identity, read as a high/low pair from the third page.
const CHIP_ID_HIGH: u8 = 0xf0;
const CHIP_ID_LOW: u8 = 0xf1;

const CHIP_ID: u16 = 0x2145;

/// Reset and page select: bit 7 is the part's own reset, bits 2:0 are the page.
const PAGE_SELECT: u8 = 0xfe;
const PAGE_RESET_BIT: u8 = 0x80;
const PAGE_0: u8 = 0x00;
// Only needed by a test that reads the power-on table page-aware: the table selects page 1
// to write the exposure ladder but this driver never selects it itself, so nothing outside
// the table has a reason to name it.
#[cfg(test)]
const PAGE_1: u8 = 0x01;
const PAGE_2: u8 = 0x02;

/// Settling after the reset and after the table lands, as the reference driver waits.
const RESET_SETTLE_MS: u32 = 100;
const TABLE_SETTLE_MS: u32 = 100;

/// How long to wait after [`Gc2145::set_window`] before starting a transfer: the part
/// applies a new window on a frame boundary rather than on the write, so a transfer
/// started immediately begins on a part still streaming the previous one.
const WINDOW_SETTLE_MS: u32 = 100;

/// Output format. Bits 4:0 choose the data mode; the upper bits are other output-path
/// settings and are merged rather than overwritten.
const REG_OUTPUT_FORMAT: u8 = 0x84;
const OUTPUT_FORMAT_FIELD: u8 = 0x1f;

/// RGB565, one of the values that field accepts (`0x02` YCbCr, `0x06` RGB565, `0x0f`
/// bypass 10-bit) — what `gc2145.c`'s `set_pixformat` writes for `PIXFORMAT_RGB565`.
pub const FORMAT_RGB565: u8 = 0x06;

/// Window origin and size. `REG_WIN_*` is the read window including the blanking the part
/// counts in its own line timing; `REG_OUT_WIN_*` is the rectangle that actually comes out.
const REG_ROW_START_HIGH: u8 = 0x09;
const REG_ROW_START_LOW: u8 = 0x0a;
const REG_COL_START_HIGH: u8 = 0x0b;
const REG_COL_START_LOW: u8 = 0x0c;
const REG_WIN_HEIGHT_HIGH: u8 = 0x0d;
const REG_WIN_HEIGHT_LOW: u8 = 0x0e;
const REG_WIN_WIDTH_HIGH: u8 = 0x0f;
const REG_WIN_WIDTH_LOW: u8 = 0x10;
const REG_OUT_WIN_HEIGHT_HIGH: u8 = 0x95;
const REG_OUT_WIN_HEIGHT_LOW: u8 = 0x96;
const REG_OUT_WIN_WIDTH_HIGH: u8 = 0x97;
const REG_OUT_WIN_WIDTH_LOW: u8 = 0x98;
const REG_OUT_WIN_ROW_START_HIGH: u8 = 0x91;
const REG_OUT_WIN_ROW_START_LOW: u8 = 0x92;
const REG_OUT_WIN_COL_START_HIGH: u8 = 0x93;
const REG_OUT_WIN_COL_START_LOW: u8 = 0x94;

/// Which rows and columns inside each decimation bin are kept, one register per axis
/// position, so the eight are a contiguous range from `REG_SUB_ROW_N1`.
const REG_SUB_ROW_N1: u8 = 0x9b;
const REG_SUB_COL_N4: u8 = 0xa2;

const REG_CROP_ENABLE: u8 = 0x90;

/// Datasheet P0:0x99 `subsample`: `[7:4]` row ratio, `[3:0]` column ratio. A read window
/// that is not a multiple of the output by this ratio comes out stretched.
const REG_SUBSAMPLE: u8 = 0x99;

/// Datasheet P0:0xfd `Scalar mode`: `[1]` column, `[0]` row. The power-on table clears it
/// and the driver never sets it, only reads it back so a caller can confirm: on this part
/// it doubles the frame rate and narrows the view. `@/records/iot/camera.md`.
const REG_SCALAR_MODE: u8 = 0xfd;

/// Datasheet P0:0x9a: `[5]`/`[4]` use-or-cut row/column, `[3]` smooth Y, `[2]` smooth
/// chroma, `[1]` average neighbours.
const REG_SUBSAMPLE_MODE: u8 = 0x9a;
const SUBSAMPLE_MODE_PLAIN: u8 = 0x06;

/// Zero keeps the whole bin in the picture. The `0x01`/`0x23` the Linux mainline and ST
/// 640x480 modes write name one offset per bin instead, which measured crops the view to
/// about a fifth of the array and doubles the frame rate. `@/records/iot/camera.md`.
const SUBSAMPLE_BINS: [u8; 8] = [0x00; 8];

/// Read-only; AEC drives them.
const REG_EXPOSURE_HIGH: u8 = 0x03;
const REG_EXPOSURE_LOW: u8 = 0x04;

/// Datasheet P0:0x05`–`0x08`, the blanking. `P0:0x07`/`0x08` carry the vertical blanking
/// lines and the datasheet gives the frame time as `Ft = VB + Vt + 8`, with `VB` counting
/// this pair — which is what makes them the documented frame-rate knob.
const REG_HBLANK_HIGH: u8 = 0x05;
const REG_HBLANK_LOW: u8 = 0x06;
const REG_VBLANK_HIGH: u8 = 0x07;
const REG_VBLANK_LOW: u8 = 0x08;
/// Bits 4:0 only — the rest of the high byte is not part of the exposure.
const EXPOSURE_HIGH_FIELD: u8 = 0x1f;

/// Mirror is bit 0, vertical flip bit 1.
const REG_ANALOG_MODE: u8 = 0x17;
const MIRROR_BIT: u8 = 0x01;

/// What the part reads at full resolution. A requested window cannot exceed it.
const ARRAY_WIDTH: u16 = 1600;
const ARRAY_HEIGHT: u16 = 1200;

/// Sensor pixels read per pixel emitted. `5` reads the whole array, so the picture is the
/// part's full view with no crop, and it is the only ratio measured to both deliver a
/// frame and carry full detail. See `@/records/iot/camera.md`.
pub const EXTRACT_RATIO: u16 = 5;

/// The ratio has to divide the array, or the output window either crops the decimated
/// image or exceeds it. `1600/5` and `1200/5` are whole, so `320x240` *is* the decimated
/// read window. A wider output buys no view for the same reason, so there is none here.
const _: () = {
    assert!((ARRAY_WIDTH as usize).is_multiple_of(EXTRACT_RATIO as usize));
    assert!((ARRAY_HEIGHT as usize).is_multiple_of(EXTRACT_RATIO as usize));
    assert!(ARRAY_WIDTH / EXTRACT_RATIO == POWER_ON_OUT_WIDTH);
    assert!(ARRAY_HEIGHT / EXTRACT_RATIO == POWER_ON_OUT_HEIGHT);
    assert!(FORMAT_RGB565 <= OUTPUT_FORMAT_FIELD);
    assert!((REG_SUB_COL_N4 - REG_SUB_ROW_N1) as usize == SUBSAMPLE_BINS.len() - 1);
};

/// The part counts the margin in its own line timing, so `+8` vertically and `+16`
/// horizontally — asymmetric, and that is what `esp32-camera` writes for QVGA.
const WIN_HEIGHT_MARGIN: u16 = 8;
const WIN_WIDTH_MARGIN: u16 = 16;

const POWER_ON_OUT_WIDTH: u16 = 320;
const POWER_ON_OUT_HEIGHT: u16 = 240;

/// The power-on table, transcribed from esp32-camera's `gc2145_default_init_regs`.
const POWER_ON_REGS: &[(u8, u8)] = &[
    // Soft reset three times over — bit 7 of the page-select register is the part's
    // own reset, so these are resets, not page changes.
    (0xfe, 0xf0),
    (0xfe, 0xf0),
    (0xfe, 0xf0),
    // Clocks and power: analog_pwc, pad IO, PLL_mode1/2, clk_div_mode, cm_mode. The
    // `0xfd` here is the scalar mode explicitly cleared; the driver never writes it.
    (0xfc, 0x06),
    (0xf6, 0x00),
    (0xf7, 0x1d),
    (0xf8, 0x83),
    (0xfa, 0x00),
    (0xf9, 0xfe),
    (0xfd, 0x00),
    (0xc2, 0x00),
    (0xf2, 0x0f),
    (0xfe, 0x00),
    // Exposure, which AEC owns once it is running — the table only seeds a start value.
    (0x03, 0x04),
    (0x04, 0x62),
    (0x05, 0x01),
    (0x06, 0x3b),
    // Read window: origin at 0,0 and 1618x1216, the array plus the margins.
    (0x09, 0x00),
    (0x0a, 0x00),
    (0x0b, 0x00),
    (0x0c, 0x00),
    (0x0d, 0x04),
    (0x0e, 0xc0),
    (0x0f, 0x06),
    (0x10, 0x52),
    // SH delay, and the mirror / vertical-flip register.
    (0x12, 0x2e),
    (0x17, 0x14),
    (0x18, 0x22),
    (0x19, 0x0f),
    (0x1a, 0x01),
    (0x1b, 0x4b),
    (0x1c, 0x07),
    (0x1d, 0x10),
    (0x1e, 0x88),
    (0x1f, 0x78),
    (0x20, 0x03),
    (0x21, 0x40),
    (0x22, 0xa0),
    (0x24, 0x1e),
    (0x25, 0x01),
    (0x26, 0x10),
    (0x2d, 0x60),
    (0x30, 0x01),
    (0x31, 0x90),
    (0x33, 0x06),
    (0x34, 0x01),
    // ISP: block enables, special effect, output format, sync and bypass modes, frame
    // start, global gain, and the AWB gains.
    (0x80, 0xff),
    (0x81, 0x24),
    (0x82, 0xfa),
    (0x83, 0x00),
    (0x84, 0x02),
    (0x86, 0x03),
    (0x88, 0x03),
    (0x89, 0x03),
    (0x85, 0x30),
    (0x8a, 0x00),
    (0x8b, 0x00),
    (0xb0, 0x55),
    (0xc3, 0x00),
    (0xc4, 0x80),
    (0xc5, 0x90),
    (0xc6, 0x38),
    (0xc7, 0x40),
    (0xec, 0x06),
    (0xed, 0x04),
    (0xee, 0x60),
    (0xef, 0x90),
    (0xb6, 0x01),
    // Crop window: the rectangle that leaves the part, which is the array itself.
    (0x90, 0x01),
    (0x91, 0x00),
    (0x92, 0x00),
    (0x93, 0x00),
    (0x94, 0x00),
    (0x95, 0x04),
    (0x96, 0xb0),
    (0x97, 0x06),
    (0x98, 0x40),
    // The analog mode register again, now the ISP block is in place.
    (0x18, 0x02),
    // BLK: black level.
    (0x40, 0x42),
    (0x41, 0x00),
    (0x43, 0x54),
    (0x5e, 0x00),
    (0x5f, 0x00),
    (0x60, 0x00),
    (0x61, 0x00),
    (0x62, 0x00),
    (0x63, 0x00),
    (0x64, 0x00),
    (0x65, 0x00),
    (0x66, 0x20),
    (0x67, 0x20),
    (0x68, 0x20),
    (0x69, 0x20),
    (0x76, 0x00),
    (0x6a, 0x00),
    (0x6b, 0x00),
    (0x6c, 0x3e),
    (0x6d, 0x3e),
    (0x6e, 0x3f),
    (0x6f, 0x3f),
    (0x70, 0x00),
    (0x71, 0x00),
    (0x76, 0x00),
    (0x72, 0xf0),
    (0x7e, 0x3c),
    (0x7f, 0x00),
    (0xfe, 0x02),
    // Page 2, three registers between BLK and AEC that the datasheet's register list
    // does not give a purpose for.
    (0x48, 0x15),
    (0x49, 0x00),
    (0x4b, 0x0b),
    (0xfe, 0x00),
    (0xfe, 0x01),
    // AEC: the exposure algorithm's own registers.
    (0x01, 0x04),
    (0x02, 0xc0),
    (0x03, 0x04),
    (0x04, 0x90),
    (0x05, 0x30),
    (0x06, 0x90),
    (0x07, 0x20),
    (0x08, 0x70),
    (0x09, 0x00),
    (0x0a, 0xc2),
    (0x0b, 0x11),
    (0x0c, 0x10),
    (0x13, 0x40),
    (0x17, 0x00),
    (0x1c, 0x11),
    (0x1e, 0x61),
    (0x1f, 0x30),
    (0x20, 0x40),
    (0x22, 0x80),
    (0x23, 0x20),
    (0xfe, 0x02),
    // INTPEE, split across both pages: one write on page 2, the ladder on page 1, and
    // the block the datasheet names on page 2.
    (0x0f, 0x04),
    (0xfe, 0x01),
    (0x12, 0x35),
    (0x15, 0x50),
    (0x10, 0x31),
    (0x3e, 0x28),
    (0x3f, 0xe0),
    (0x40, 0x20),
    (0x41, 0x0f),
    (0xfe, 0x02),
    (0x0f, 0x05),
    (0xfe, 0x02),
    (0x90, 0x6c),
    (0x91, 0x03),
    (0x92, 0xc8),
    (0x94, 0x66),
    (0x95, 0xb5),
    (0x97, 0x64),
    (0xa2, 0x11),
    (0xfe, 0x00),
    (0xfe, 0x02),
    // DNDD.
    (0x80, 0xc1),
    (0x81, 0x08),
    (0x82, 0x08),
    (0x83, 0x08),
    (0x84, 0x0a),
    (0x86, 0xf0),
    (0x87, 0x50),
    (0x88, 0x15),
    (0x89, 0x50),
    (0x8a, 0x30),
    (0x8b, 0x10),
    (0xfe, 0x01),
    // ASDE: one register on page 1, the rest on page 2.
    (0x21, 0x14),
    (0xfe, 0x02),
    (0xa3, 0x40),
    (0xa4, 0x20),
    (0xa5, 0x40),
    (0xa6, 0x80),
    (0xab, 0x40),
    (0xae, 0x0c),
    (0xb3, 0x34),
    (0xb4, 0x44),
    (0xb6, 0x38),
    (0xb7, 0x02),
    (0xb9, 0x30),
    (0x3c, 0x08),
    (0x3d, 0x30),
    (0x4b, 0x0d),
    (0x4c, 0x20),
    (0xfe, 0x00),
    (0xfe, 0x02),
    // Gamma: twenty-two points from shadow to highlight.
    (0x10, 0x10),
    (0x11, 0x15),
    (0x12, 0x1a),
    (0x13, 0x1f),
    (0x14, 0x2c),
    (0x15, 0x39),
    (0x16, 0x45),
    (0x17, 0x54),
    (0x18, 0x69),
    (0x19, 0x7d),
    (0x1a, 0x8f),
    (0x1b, 0x9d),
    (0x1c, 0xa9),
    (0x1d, 0xbd),
    (0x1e, 0xcd),
    (0x1f, 0xd9),
    (0x20, 0xe3),
    (0x21, 0xea),
    (0x22, 0xef),
    (0x23, 0xf5),
    (0x24, 0xf9),
    (0x25, 0xff),
    (0xfe, 0x02),
    // Gamma 2, the second leg of the same ramp.
    (0x26, 0x0f),
    (0x27, 0x14),
    (0x28, 0x19),
    (0x29, 0x1e),
    (0x2a, 0x27),
    (0x2b, 0x33),
    (0x2c, 0x3b),
    (0x2d, 0x45),
    (0x2e, 0x59),
    (0x2f, 0x69),
    (0x30, 0x7c),
    (0x31, 0x89),
    (0x32, 0x98),
    (0x33, 0xae),
    (0x34, 0xc0),
    (0x35, 0xcf),
    (0x36, 0xda),
    (0x37, 0xe2),
    (0x38, 0xe9),
    (0x39, 0xf3),
    (0x3a, 0xf9),
    (0x3b, 0xff),
    (0xfe, 0x02),
    // YCP: chroma.
    (0xd1, 0x30),
    (0xd2, 0x30),
    (0xd3, 0x45),
    (0xdd, 0x14),
    (0xde, 0x86),
    (0xed, 0x01),
    (0xee, 0x28),
    (0xef, 0x30),
    (0xd8, 0xd8),
    (0xfe, 0x01),
    // CC: the colour matrix's coefficients.
    (0xa1, 0x80),
    (0xa2, 0x80),
    (0xa4, 0x00),
    (0xa5, 0x00),
    (0xa6, 0x70),
    (0xa7, 0x00),
    (0xa8, 0x77),
    (0xa9, 0x77),
    (0xaa, 0x1f),
    (0xab, 0x0d),
    (0xac, 0x19),
    (0xad, 0x24),
    (0xae, 0x0e),
    (0xaf, 0x1d),
    (0xb0, 0x12),
    (0xb1, 0x0c),
    (0xb2, 0x06),
    (0xb3, 0x13),
    (0xb4, 0x10),
    (0xb5, 0x0c),
    (0xb6, 0x6a),
    (0xb7, 0x46),
    (0xb8, 0x40),
    (0xb9, 0x0b),
    (0xba, 0x04),
    (0xbb, 0x00),
    (0xbc, 0x53),
    (0xbd, 0x37),
    (0xbe, 0x2d),
    (0xbf, 0x0a),
    (0xc0, 0x0a),
    (0xc1, 0x14),
    (0xc2, 0x34),
    (0xc3, 0x22),
    (0xc4, 0x18),
    (0xc5, 0x23),
    (0xc6, 0x0f),
    (0xc7, 0x3c),
    (0xc8, 0x20),
    (0xc9, 0x1f),
    (0xca, 0x17),
    (0xcb, 0x2d),
    (0xcc, 0x12),
    (0xcd, 0x20),
    (0xd0, 0x61),
    (0xd1, 0x2f),
    (0xd2, 0x39),
    (0xd3, 0x45),
    (0xd4, 0x2c),
    (0xd5, 0x21),
    (0xd6, 0x64),
    (0xd7, 0x2d),
    (0xd8, 0x30),
    (0xd9, 0x42),
    (0xda, 0x27),
    (0xdb, 0x13),
    (0xfe, 0x00),
    (0xfe, 0x01),
    // LSC: lens shading, and the largest block here — a lookup table rather than a set
    // of settings. `0x4c` selects a coefficient, `0x4d`/`0x4e` carry its value, `0x4f`
    // ends the block, and the entries after it are the second half of the same table.
    (0x4f, 0x00),
    (0x4f, 0x00),
    (0x4b, 0x01),
    (0x4f, 0x00),
    (0x4c, 0x01),
    (0x4d, 0x6f),
    (0x4e, 0x02),
    (0x4c, 0x01),
    (0x4d, 0x70),
    (0x4e, 0x02),
    (0x4c, 0x01),
    (0x4d, 0x8f),
    (0x4e, 0x02),
    (0x4c, 0x01),
    (0x4d, 0x90),
    (0x4e, 0x02),
    (0x4c, 0x01),
    (0x4d, 0xed),
    (0x4e, 0x33),
    (0x4c, 0x01),
    (0x4d, 0xcd),
    (0x4e, 0x33),
    (0x4c, 0x01),
    (0x4d, 0xec),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0x6c),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0x6d),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0x6e),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0x8c),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0x8d),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0x8e),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xab),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xac),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xad),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xae),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xcb),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xcc),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xce),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xeb),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xec),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xee),
    (0x4e, 0x03),
    (0x4c, 0x02),
    (0x4d, 0x0c),
    (0x4e, 0x03),
    (0x4c, 0x02),
    (0x4d, 0x0d),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xea),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xaf),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xcf),
    (0x4e, 0x03),
    (0x4c, 0x01),
    (0x4d, 0xca),
    (0x4e, 0x04),
    (0x4c, 0x02),
    (0x4d, 0x0b),
    (0x4e, 0x05),
    (0x4c, 0x02),
    (0x4d, 0xc8),
    (0x4e, 0x06),
    (0x4c, 0x02),
    (0x4d, 0xa8),
    (0x4e, 0x06),
    (0x4c, 0x02),
    (0x4d, 0xa9),
    (0x4e, 0x06),
    (0x4c, 0x02),
    (0x4d, 0x89),
    (0x4e, 0x06),
    (0x4c, 0x02),
    (0x4d, 0x69),
    (0x4e, 0x06),
    (0x4c, 0x02),
    (0x4d, 0x6a),
    (0x4e, 0x06),
    (0x4c, 0x02),
    (0x4d, 0xc7),
    (0x4e, 0x07),
    (0x4c, 0x02),
    (0x4d, 0xe7),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x07),
    (0x4e, 0x07),
    (0x4c, 0x02),
    (0x4d, 0xe8),
    (0x4e, 0x07),
    (0x4c, 0x02),
    (0x4d, 0xe9),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x08),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x09),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x27),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x28),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x29),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x47),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x48),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x49),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x67),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x68),
    (0x4e, 0x07),
    (0x4c, 0x03),
    (0x4d, 0x69),
    (0x4e, 0x07),
    (0x4f, 0x01),
    (0xfe, 0x01),
    (0x50, 0x80),
    (0x51, 0xa8),
    (0x52, 0x57),
    (0x53, 0x38),
    (0x54, 0xc7),
    (0x56, 0x0e),
    (0x58, 0x08),
    (0x5b, 0x00),
    (0x5c, 0x74),
    (0x5d, 0x8b),
    (0x61, 0xd3),
    (0x62, 0xb5),
    (0x63, 0x00),
    (0x65, 0x04),
    (0x67, 0xb2),
    (0x68, 0xac),
    (0x69, 0x00),
    (0x6a, 0xb2),
    (0x6b, 0xac),
    (0x6c, 0xb2),
    (0x6d, 0xac),
    (0x6e, 0x40),
    (0x6f, 0x18),
    (0x73, 0x00),
    (0x70, 0x10),
    (0x71, 0xe8),
    (0x72, 0xc0),
    (0x74, 0x01),
    (0x75, 0x01),
    (0x7f, 0x08),
    (0x76, 0x70),
    (0x77, 0x58),
    (0x78, 0xa0),
    (0xfe, 0x00),
    (0xfe, 0x02),
    // CC: the matrix the coefficients above feed.
    (0xc0, 0x01),
    (0xc1, 0x50),
    (0xc2, 0xf9),
    (0xc3, 0x00),
    (0xc4, 0xe8),
    (0xc5, 0x48),
    (0xc6, 0xf0),
    (0xc7, 0x50),
    (0xc8, 0xf2),
    (0xc9, 0x00),
    (0xca, 0xe0),
    (0xcb, 0x45),
    (0xcc, 0xec),
    (0xcd, 0x45),
    (0xce, 0xf0),
    (0xcf, 0x00),
    (0xe3, 0xf0),
    (0xe4, 0x45),
    (0xe5, 0xe8),
    (0xfe, 0x00),
    // Frame rate: a clock divider and the blanking on page 0, paired with an exposure
    // ladder on page 1. The table writes this twice, the second time at a lower divider.
    (0xf2, 0x0f),
    (0xfe, 0x00),
    (0xf7, 0x1d),
    (0xf8, 0x84),
    (0xfa, 0x00),
    (0x05, 0x01),
    (0x06, 0x3b),
    (0x07, 0x01),
    (0x08, 0x0b),
    (0xfe, 0x01),
    (0x25, 0x01),
    (0x26, 0x32),
    (0x27, 0x03),
    (0x28, 0x96),
    (0x29, 0x03),
    (0x2a, 0x96),
    (0x2b, 0x03),
    (0x2c, 0x96),
    (0x2d, 0x04),
    (0x2e, 0x62),
    (0x3c, 0x00),
    (0xfe, 0x00),
    (0xfe, 0x00),
    (0x18, 0x22),
    (0xfe, 0x02),
    (0x40, 0xbf),
    (0x46, 0xcf),
    (0xfe, 0x00),
    (0xfe, 0x00),
    (0xf7, 0x1d),
    (0xf8, 0x84),
    (0xfa, 0x10),
    (0x05, 0x01),
    (0x06, 0x18),
    (0x07, 0x00),
    (0x08, 0x2e),
    (0xfe, 0x01),
    (0x25, 0x00),
    (0x26, 0xa2),
    (0x27, 0x01),
    (0x28, 0xe6),
    (0x29, 0x01),
    (0x2a, 0xe6),
    (0x2b, 0x01),
    (0x2c, 0xe6),
    (0x2d, 0x04),
    (0x2e, 0x62),
    (0x3c, 0x00),
    (0xfe, 0x00),
    // The window the table lands on: a 320x240 sub-window of the array rather than the
    // whole of it, which is what `POWER_ON_OUT_WIDTH`/`HEIGHT` name and what
    // `set_window` replaces with the panel's own geometry.
    (0x09, 0x01),
    (0x0a, 0xd0),
    (0x0b, 0x02),
    (0x0c, 0x70),
    (0x0d, 0x01),
    (0x0e, 0x00),
    (0x0f, 0x01),
    (0x10, 0x50),
    (0x90, 0x01),
    (0x91, 0x00),
    (0x92, 0x00),
    (0x93, 0x00),
    (0x94, 0x00),
    (0x95, 0x00),
    (0x96, 0xf0),
    (0x97, 0x01),
    (0x98, 0x40),
];

/// What the sensor can fail at, over whatever the bus reports.
#[derive(Debug)]
pub enum Gc2145Error<E> {
    /// The bus itself failed.
    Bus(E),
    /// Something answered at the part's address but is not a GC2145.
    WrongChip { found: u16 },
    /// The part did not answer at all.
    Absent,
    /// An empty window, or one whose read window — the output times the ratio — does not
    /// fit the array.
    WindowOutOfRange,
}

/// What the part reports it is streaming, read back in one pass.
///
/// The read window and the ratio are here because a picture can only be judged against
/// them: an output size that reads back correctly while the ratio does not is a picture
/// of the right shape and the wrong content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowGeometry {
    pub out_width: u16,
    pub out_height: u16,
    /// Top-left of the read window in the array.
    pub row_start: u16,
    pub col_start: u16,
    /// What the part charges its line timing for, so larger than `out_*` by the margins.
    pub win_width: u16,
    pub win_height: u16,
    /// Decimation factor, high nibble rows and low nibble columns.
    pub subsample: u8,
    /// `0x9a` — which of "use" or "cut" the part does with the rows and columns the
    /// decimation removes, and whether it averages them.
    pub subsample_mode: u8,
    /// `0x9b`–`0xa2`, the per-bin row and column numbers the part decodes by.
    ///
    /// Read rather than assumed: the datasheet's reset value is `0x02`/`0x04`, this crate's
    /// power-on table does not mention these registers at all, and `write_decimation`
    /// writes zero. So there are three candidate values and only the part knows which.
    pub sub_bins: [u8; 8],
    /// `0xfd`, read rather than written: it is left at the power-on table's `0x00` on
    /// purpose, and this is how a caller checks that it took.
    pub scalar: u8,
    pub crop_enabled: bool,
}

impl<E> From<E> for Gc2145Error<E> {
    fn from(error: E) -> Self {
        Self::Bus(error)
    }
}

/// The part, reached over a shared bus.
///
/// `D` is the bus device type, and deliberately not assumed to be exclusive: on this
/// board the touch controller, the codecs and the motion sensor share it, and the
/// sensor's SCCB *is* that bus.
pub struct Gc2145<D: I2c> {
    i2c: D,
    addr: u8,
    /// Set once the part is streaming, so a second [`Self::init`] is a no-op rather
    /// than a reset under a live transfer.
    running: bool,
}

impl<D: I2c> Gc2145<D> {
    pub fn new(i2c: D, addr: u8) -> Self {
        Self {
            i2c,
            addr,
            running: false,
        }
    }

    /// Whether a GC2145 answers at `addr`.
    ///
    /// A NACK on the first register read is an answer rather than a failure: it means
    /// the address reached nothing, which is what an absent part looks like on a bus
    /// other devices share.
    pub fn probe(&mut self) -> Result<(), Gc2145Error<D::Error>> {
        match self.read_id() {
            Ok(CHIP_ID) => Ok(()),
            Ok(found) => Err(Gc2145Error::WrongChip { found }),
            Err(_) => Err(Gc2145Error::Absent),
        }
    }

    /// Brings the part up: reset, the power-on table, then RGB565.
    ///
    /// Idempotent, so a board that reaches the sensor from two bring-up paths does
    /// not reset a running part under a live transfer. The window is *not* set here:
    /// [`Self::set_window`] is separate because the caller has to know the geometry it
    /// asked for, and a sensor left at the power-on window would otherwise stream a
    /// shape nobody downstream is sized for.
    pub fn init(&mut self, delay: &mut impl DelayNs) -> Result<(), Gc2145Error<D::Error>> {
        if self.running {
            return Ok(());
        }
        self.probe()?;
        self.write_reg(PAGE_SELECT, PAGE_RESET_BIT | PAGE_0)?;
        delay.delay_ms(RESET_SETTLE_MS);
        for (reg, value) in POWER_ON_REGS {
            self.write_reg(*reg, *value)?;
        }
        delay.delay_ms(TABLE_SETTLE_MS);
        self.set_format()?;
        self.running = true;
        Ok(())
    }

    /// Switches the output to RGB565, leaving the rest of the format register alone.
    pub fn set_format(&mut self) -> Result<(), Gc2145Error<D::Error>> {
        self.select_page(PAGE_0)?;
        let current = self.read_reg(REG_OUTPUT_FORMAT)?;
        self.write_reg(
            REG_OUTPUT_FORMAT,
            (current & !OUTPUT_FORMAT_FIELD) | FORMAT_RGB565,
        )?;
        Ok(())
    }

    /// Mirrors the picture horizontally, read-modify-written so it does not disturb
    /// the vertical flip in the same register.
    pub fn set_hmirror(&mut self, enable: bool) -> Result<(), Gc2145Error<D::Error>> {
        self.select_page(PAGE_0)?;
        let current = self.read_reg(REG_ANALOG_MODE)?;
        self.write_reg(
            REG_ANALOG_MODE,
            if enable {
                current | MIRROR_BIT
            } else {
                current & !MIRROR_BIT
            },
        )?;
        Ok(())
    }

    /// Emits `width` x `height` by decimating a `ratio`-times-larger read window, centred
    /// in the array.
    ///
    /// The read window (`0x09`–`0x10`) is what the sensor charges its line timing for and
    /// is `ratio` times the output in each direction; the output window (`0x91`–`0x98`) is
    /// what leaves the part. `0x99` is what connects them, and a part reading N pixels to
    /// emit one has to be told N or the picture comes out stretched. `esp32-camera` is the
    /// arithmetic:
    ///
    /// ```c
    /// win_w = w * c_ratio;  win_h = h * r_ratio;
    /// win_x = ((UXGA_HSIZE - win_w) / 2);  win_y = ((UXGA_VSIZE - win_h) / 2);
    /// video_write_cci_reg(&cfg->i2c, GC2145_REG_WIN_ROW_START, win_y);
    /// video_write_cci_reg(&cfg->i2c, GC2145_REG_WIN_COL_START, win_x);
    /// video_write_cci_reg(&cfg->i2c, GC2145_REG_WIN_HEIGHT, win_h + 8);
    /// video_write_cci_reg(&cfg->i2c, GC2145_REG_WIN_WIDTH, win_w + 16);
    /// video_write_cci_reg(&cfg->i2c, GC2145_REG_SUBSAMPLE, (r_ratio << 4) | c_ratio);
    /// ```
    pub fn set_window(
        &mut self,
        width: u16,
        height: u16,
        ratio: u16,
        delay: &mut impl DelayNs,
    ) -> Result<(), Gc2145Error<D::Error>> {
        if width == 0 || height == 0 || ratio == 0 {
            return Err(Gc2145Error::WindowOutOfRange);
        }
        let read_width = width
            .checked_mul(ratio)
            .filter(|w| *w <= ARRAY_WIDTH)
            .ok_or(Gc2145Error::WindowOutOfRange)?;
        let read_height = height
            .checked_mul(ratio)
            .filter(|h| *h <= ARRAY_HEIGHT)
            .ok_or(Gc2145Error::WindowOutOfRange)?;
        let row_start = (ARRAY_HEIGHT - read_height) / 2;
        let col_start = (ARRAY_WIDTH - read_width) / 2;
        let win_h = read_height + WIN_HEIGHT_MARGIN;
        let win_w = read_width + WIN_WIDTH_MARGIN;

        // `0x95`-`0x98` names the *output* size, which every driver with a working picture
        // writes. Whether the part honours that reading or treats it as a rectangle of the
        // read window is undocumented, and measured as indistinguishable — see
        // `@/records/iot/camera.md`.
        self.select_page(PAGE_0)?;
        // The output window's origin is the read window's: the ratio decides how many read
        // pixels each output pixel covers, so there is nothing to centre.
        self.write_reg(REG_OUT_WIN_ROW_START_HIGH, 0)?;
        self.write_reg(REG_OUT_WIN_ROW_START_LOW, 0)?;
        self.write_reg(REG_OUT_WIN_COL_START_HIGH, 0)?;
        self.write_reg(REG_OUT_WIN_COL_START_LOW, 0)?;
        self.write_reg(REG_OUT_WIN_HEIGHT_HIGH, (height >> 8) as u8)?;
        self.write_reg(REG_OUT_WIN_HEIGHT_LOW, height as u8)?;
        self.write_reg(REG_OUT_WIN_WIDTH_HIGH, (width >> 8) as u8)?;
        self.write_reg(REG_OUT_WIN_WIDTH_LOW, width as u8)?;
        self.write_reg(REG_ROW_START_HIGH, (row_start >> 8) as u8)?;
        self.write_reg(REG_ROW_START_LOW, row_start as u8)?;
        self.write_reg(REG_COL_START_HIGH, (col_start >> 8) as u8)?;
        self.write_reg(REG_COL_START_LOW, col_start as u8)?;
        self.write_reg(REG_WIN_HEIGHT_HIGH, (win_h >> 8) as u8)?;
        self.write_reg(REG_WIN_HEIGHT_LOW, win_h as u8)?;
        self.write_reg(REG_WIN_WIDTH_HIGH, (win_w >> 8) as u8)?;
        self.write_reg(REG_WIN_WIDTH_LOW, win_w as u8)?;
        self.write_decimation(ratio as u8, delay)?;
        self.write_reg(REG_CROP_ENABLE, 0x01)?;
        Ok(())
    }

    /// Sets the decimation factor — `5` is `0x99 = 0x55` — and how it is produced.
    ///
    /// The mode byte and bins go with it because the power-on table does not mention them
    /// at all, so without this the part runs on the datasheet's reset values (`0x06` and
    /// `0x02`/`0x04`). `esp32-camera` uses `0x0e` for the mode byte, which is this plus
    /// bit 3; measured, the two differ in nothing. `@/records/iot/camera.md`.
    pub fn write_decimation(
        &mut self,
        ratio: u8,
        delay: &mut impl DelayNs,
    ) -> Result<(), Gc2145Error<D::Error>> {
        self.select_page(PAGE_0)?;
        self.write_reg(REG_SUBSAMPLE, (ratio << 4) | ratio)?;
        self.write_reg(REG_SUBSAMPLE_MODE, SUBSAMPLE_MODE_PLAIN)?;
        for (index, value) in SUBSAMPLE_BINS.iter().enumerate() {
            self.write_reg(REG_SUB_ROW_N1 + index as u8, *value)?;
        }
        // The part takes the window on a frame boundary rather than on the write, and
        // Zephyr notes the datasheet does not say why the wait is needed — only that
        // the sensor DSP needs it to act on the transaction before the next one.
        delay.delay_ms(WINDOW_SETTLE_MS);
        Ok(())
    }

    /// Every register the picture depends on, read in one pass, so a write that did not
    /// land is visible here rather than showing up later as a picture of the wrong shape.
    ///
    /// Reading only `0x95`–`0x98` would report the size that was asked for while saying
    /// nothing about the read window or the ratio, which is how a stretched picture passes
    /// as correctly sized.
    pub fn read_window_geometry(&mut self) -> Result<WindowGeometry, Gc2145Error<D::Error>> {
        self.select_page(PAGE_0)?;
        let mut sub_bins = [0u8; SUBSAMPLE_BINS.len()];
        for (index, bin) in sub_bins.iter_mut().enumerate() {
            *bin = self.read_reg(REG_SUB_ROW_N1 + index as u8)?;
        }
        let mut read = |high: u8, low: u8| -> Result<u16, D::Error> {
            Ok(u16::from(self.read_reg(high)?) << 8 | u16::from(self.read_reg(low)?))
        };
        Ok(WindowGeometry {
            out_width: read(REG_OUT_WIN_WIDTH_HIGH, REG_OUT_WIN_WIDTH_LOW)?,
            out_height: read(REG_OUT_WIN_HEIGHT_HIGH, REG_OUT_WIN_HEIGHT_LOW)?,
            row_start: read(REG_ROW_START_HIGH, REG_ROW_START_LOW)?,
            col_start: read(REG_COL_START_HIGH, REG_COL_START_LOW)?,
            win_width: read(REG_WIN_WIDTH_HIGH, REG_WIN_WIDTH_LOW)?,
            win_height: read(REG_WIN_HEIGHT_HIGH, REG_WIN_HEIGHT_LOW)?,
            subsample: self.read_reg(REG_SUBSAMPLE)?,
            subsample_mode: self.read_reg(REG_SUBSAMPLE_MODE)?,
            scalar: self.read_reg(REG_SCALAR_MODE)?,
            sub_bins,
            crop_enabled: self.read_reg(REG_CROP_ENABLE)? & 0x01 == 0x01,
        })
    }

    /// The exposure the part is running, in lines.
    ///
    /// Read-only per the datasheet and driven by AEC, which is what makes it worth
    /// reading: once the exposure exceeds the window the frame time is a function of this
    /// and of nothing else, so a picture that arrives slowly is explained by this number
    /// rather than by the DMA, the ring or the clock.
    pub fn read_exposure(&mut self) -> Result<u16, Gc2145Error<D::Error>> {
        self.select_page(PAGE_0)?;
        let high = u16::from(self.read_reg(REG_EXPOSURE_HIGH)? & EXPOSURE_HIGH_FIELD);
        Ok(high << 8 | u16::from(self.read_reg(REG_EXPOSURE_LOW)?))
    }

    /// The blanking the part is actually running, horizontal and vertical.
    ///
    /// Read rather than assumed from the power-on table, because the datasheet makes the
    /// vertical pair the frame-rate control — `Ft = VB + Vt + 8` — and the table writes that
    /// group twice. Naming the pair is the difference between a measured frame rate and an
    /// arithmetic one; see `@/records/iot/camera.md`.
    pub fn read_blanking(&mut self) -> Result<(u16, u16), Gc2145Error<D::Error>> {
        self.select_page(PAGE_0)?;
        let mut pair = |high: u8, low: u8| -> Result<u16, D::Error> {
            Ok(u16::from(self.read_reg(high)?) << 8 | u16::from(self.read_reg(low)?))
        };
        Ok((
            pair(REG_HBLANK_HIGH, REG_HBLANK_LOW)?,
            pair(REG_VBLANK_HIGH, REG_VBLANK_LOW)?,
        ))
    }

    /// The identity pair as the part reports it, so a caller can log what answered.
    pub fn read_id(&mut self) -> Result<u16, Gc2145Error<D::Error>> {
        // The identity pair is not on page 0, and the page select is what the
        // reference driver reads it after.
        self.select_page(PAGE_2)?;
        let high = self.read_reg(CHIP_ID_HIGH)?;
        let low = self.read_reg(CHIP_ID_LOW)?;
        Ok(u16::from(high) << 8 | u16::from(low))
    }

    /// The format register's data-mode field, so a write that did not land is visible here
    /// rather than later as a picture in the wrong format.
    pub fn read_format(&mut self) -> Result<u8, Gc2145Error<D::Error>> {
        self.select_page(PAGE_0)?;
        Ok(self.read_reg(REG_OUTPUT_FORMAT)? & OUTPUT_FORMAT_FIELD)
    }

    fn select_page(&mut self, page: u8) -> Result<(), Gc2145Error<D::Error>> {
        self.write_reg(PAGE_SELECT, page)?;
        Ok(())
    }

    fn read_reg(&mut self, reg: u8) -> Result<u8, D::Error> {
        let mut byte = [0u8; 1];
        self.i2c.write_read(self.addr, &[reg], &mut byte)?;
        Ok(byte[0])
    }

    fn write_reg(&mut self, reg: u8, value: u8) -> Result<(), D::Error> {
        self.i2c.write(self.addr, &[reg, value])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::vec;
    use alloc::vec::Vec;
    use embedded_hal::i2c::{ErrorKind, ErrorType, NoAcknowledgeSource, Operation};

    /// A part with no clock behind it. Registers live in a shadow file keyed by page,
    /// because the page is part of what a register number means, and every write is kept
    /// in order so a call that short-circuits leaves no trace.
    struct MockPart {
        page: u8,
        pointer: u8,
        registers: BTreeMap<(u8, u8), u8>,
        history: Vec<(u8, u8, u8)>,
        silent: bool,
    }

    impl MockPart {
        fn new() -> Self {
            let mut part = Self {
                page: PAGE_0,
                pointer: 0,
                registers: BTreeMap::new(),
                history: Vec::new(),
                silent: false,
            };
            part.seed_identity();
            part
        }

        /// What the part reads at power-on. `init` resets the page before the table runs,
        /// so the identity pair is the only thing that has to be here already.
        fn seed_identity(&mut self) {
            self.registers
                .insert((PAGE_2, CHIP_ID_HIGH), (CHIP_ID >> 8) as u8);
            self.registers.insert((PAGE_2, CHIP_ID_LOW), CHIP_ID as u8);
        }

        fn get(&self, reg: u8) -> u8 {
            self.registers
                .get(&(self.page, reg))
                .copied()
                .unwrap_or_default()
        }

        /// A 16-bit register pair as one number, which is how the driver reads them.
        fn pair(&self, high: u8, low: u8) -> u16 {
            u16::from(self.get(high)) << 8 | u16::from(self.get(low))
        }

        fn set(&mut self, reg: u8, value: u8) {
            self.history.push((self.page, reg, value));
            if reg == PAGE_SELECT {
                self.page = value & 0x07;
            }
            self.registers.insert((self.page, reg), value);
        }
    }

    struct MockI2c {
        part: MockPart,
    }

    impl ErrorType for MockI2c {
        type Error = ErrorKind;
    }

    impl I2c for MockI2c {
        fn read(&mut self, _address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
            if self.part.silent {
                return Err(ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address));
            }
            let value = self.part.get(self.part.pointer);
            read.fill(value);
            Ok(())
        }

        fn write(&mut self, _address: u8, write: &[u8]) -> Result<(), Self::Error> {
            match write {
                [reg] => self.part.pointer = *reg,
                [reg, value] => {
                    let (reg, value) = (*reg, *value);
                    self.part.set(reg, value);
                }
                _ => {}
            }
            Ok(())
        }

        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [Operation<'_>],
        ) -> Result<(), Self::Error> {
            for operation in operations {
                match operation {
                    Operation::Read(read) => self.read(address, read)?,
                    Operation::Write(write) => self.write(address, write)?,
                }
            }
            Ok(())
        }
    }

    /// Records the millisecond waits instead of spending them, so a test can pin the
    /// settle windows.
    struct MockDelay {
        waited_ms: Vec<u32>,
    }

    impl MockDelay {
        fn new() -> Self {
            Self {
                waited_ms: Vec::new(),
            }
        }
    }

    impl DelayNs for MockDelay {
        fn delay_ns(&mut self, ns: u32) {
            self.waited_ms.push(ns / 1_000_000);
        }
    }

    fn sensor() -> Gc2145<MockI2c> {
        Gc2145::new(
            MockI2c {
                part: MockPart::new(),
            },
            GC2145_I2C_ADDR,
        )
    }

    /// The table's entry for `reg` on the page selected at that point in the walk.
    ///
    /// Page-aware because the number alone is ambiguous: `0x84` is the output format
    /// on page 0 and a DNDD threshold on page 2.
    fn table_entry(reg: u8) -> Option<u8> {
        let mut page = PAGE_0;
        let mut found = None;
        for (r, v) in POWER_ON_REGS {
            if *r == PAGE_SELECT && v & !PAGE_RESET_BIT <= 0x03 {
                page = v & 0x03;
                continue;
            }
            if *r == reg && page == PAGE_0 {
                found = Some(*v);
            }
        }
        found
    }

    /// Pins the entries the picture depends on. Not a checksum of the whole table:
    /// the point is that the handful of registers whose values are known to fix or
    /// cause a specific artefact cannot be edited by accident.
    #[test]
    fn power_on_table_pins_the_entries_that_decide_the_picture() {
        // Output data mode. If this drifts the merge still yields RGB565, but the other
        // output-path settings in the upper bits are whatever the table happened to carry.
        assert_eq!(table_entry(REG_OUTPUT_FORMAT), Some(0x02));

        // The window the table lands on, which sizes the ring and the panel.
        assert_eq!(
            (
                u16::from(table_entry(REG_OUT_WIN_WIDTH_HIGH).unwrap()) << 8
                    | u16::from(table_entry(REG_OUT_WIN_WIDTH_LOW).unwrap()),
                u16::from(table_entry(REG_OUT_WIN_HEIGHT_HIGH).unwrap()) << 8
                    | u16::from(table_entry(REG_OUT_WIN_HEIGHT_LOW).unwrap()),
            ),
            (POWER_ON_OUT_WIDTH, POWER_ON_OUT_HEIGHT)
        );

        // The four esp32-camera annotates as artefact fixes.
        // `0x12` "sh_delay 太短 YUV出图异常" — too short an SH delay, bad YUV out.
        assert_eq!(table_entry(0x12), Some(0x2e), "SH delay");
        // `0x1c` "帧率快后，横条纹" — horizontal stripes once the frame rate is up.
        assert_eq!(table_entry(0x1c), Some(0x07), "TX width / space width");
        // `0x1e` "fix 竖线" — fixes vertical lines.
        assert_eq!(table_entry(0x1e), Some(0x88), "analog mode 1");
        // `0x21` "fix 灯管横条纹" — fixes fluorescent-tube horizontal stripes.
        assert_eq!(table_entry(0x21), Some(0x40), "analog mode 3");

        // The mirror/flip register, whose bit 0 this driver sets and whose bit 1 it
        // leaves alone.
        assert_eq!(table_entry(REG_ANALOG_MODE), Some(0x14));
    }

    /// The page-1 frame-rate steps are written twice, and only the second set is in effect.
    ///
    /// Pinned because the first set reads like the real thing — it comes earlier in the table
    /// and carries the larger values — so quoting it is easy and wrong. `camera.md` recorded
    /// these registers as written nowhere at all, and then cited the pair above them as the
    /// frame-rate ceiling, which measurement (486 lines against a ~1200-line readout) rules out.
    ///
    /// The exposure itself is a different register pair on a different page: page 0's
    /// `0x03`/`0x04`, written once. That separation is the reason the earlier claim was wrong,
    /// so it is asserted here too.
    #[test]
    fn the_frame_rate_steps_are_written_twice_and_the_second_set_wins() {
        // Walked with the driver's own page rule rather than through `table_entry`, which
        // returns the last write per page already and so cannot show that there were two.
        let mut page = PAGE_0;
        let mut writes: BTreeMap<(u8, u8), Vec<u8>> = BTreeMap::new();
        for (reg, value) in POWER_ON_REGS {
            if *reg == PAGE_SELECT {
                page = *value & 0x03;
                continue;
            }
            writes.entry((page, *reg)).or_default().push(*value);
        }
        let steps = |reg: u8| writes.get(&(PAGE_1, reg)).cloned().unwrap_or_default();

        // `0x27`–`0x2c` are halved by the second write and `0x2d`/`0x2e` are written twice
        // unchanged, so a claim about these has to name the effective pair.
        for (reg, first, second) in [
            (0x27, 0x03, 0x01),
            (0x28, 0x96, 0xe6),
            (0x29, 0x03, 0x01),
            (0x2a, 0x96, 0xe6),
            (0x2b, 0x03, 0x01),
            (0x2c, 0x96, 0xe6),
            (0x2d, 0x04, 0x04),
            (0x2e, 0x62, 0x62),
        ] {
            assert_eq!(
                steps(reg),
                vec![first, second],
                "page-1 0x{reg:02x} is a frame-rate step, written twice"
            );
        }
        // The pair the driver actually reads as the exposure, on the page it reads it from.
        assert_eq!(
            (
                writes.get(&(PAGE_0, REG_EXPOSURE_HIGH)),
                writes.get(&(PAGE_0, REG_EXPOSURE_LOW)),
            ),
            (Some(&vec![0x04]), Some(&vec![0x62])),
            "exposure is page 0's 0x03/0x04, written once — not the page-1 ladder above"
        );
    }

    /// Every register write in the table is preceded by a page select, because a write
    /// without one lands on whatever page the previous write left selected.
    #[test]
    fn the_table_selects_a_page_before_its_first_write() {
        let mut selected = false;
        for (reg, _) in POWER_ON_REGS {
            if *reg == PAGE_SELECT {
                selected = true;
                continue;
            }
            assert!(selected, "a register write landed with no page selected");
        }
        assert!(!POWER_ON_REGS.is_empty(), "the table is empty");
    }

    /// A bring-up resets the part before anything else reaches it, replays the whole
    /// table, and merges RGB565 into the data-mode field rather than overwriting the
    /// register — the table leaves `0x84` at `0x02`, and an overwrite would throw away
    /// whatever else that register carries. It leaves the window alone, because the caller
    /// has to know the geometry it asked for.
    #[test]
    fn init_resets_replays_the_table_and_merges_the_format() {
        let mut sensor = sensor();
        let mut delay = MockDelay::new();
        sensor.init(&mut delay).expect("init");

        let history = &sensor.i2c.part.history;
        let reset_at = history
            .iter()
            .position(|&(_, reg, value)| reg == PAGE_SELECT && value == PAGE_RESET_BIT | PAGE_0)
            .expect("the part must be reset");
        let first_table_write = history
            .iter()
            .position(|&(_, reg, _)| reg != PAGE_SELECT)
            .expect("the table must be replayed");
        assert!(
            reset_at < first_table_write,
            "the reset has to come before the table"
        );
        assert!(history.len() >= POWER_ON_REGS.len());

        assert_eq!(sensor.read_format().expect("format"), FORMAT_RGB565);
        assert_eq!(POWER_ON_OUT_WIDTH * EXTRACT_RATIO, ARRAY_WIDTH);
        assert_eq!(POWER_ON_OUT_HEIGHT * EXTRACT_RATIO, ARRAY_HEIGHT);
        assert_eq!(delay.waited_ms, vec![RESET_SETTLE_MS, TABLE_SETTLE_MS]);
    }

    /// A second `init` on a streaming part would reset it under a live transfer.
    #[test]
    fn init_is_idempotent() {
        let mut sensor = sensor();
        let mut delay = MockDelay::new();
        sensor.init(&mut delay).expect("init");
        let after_first = sensor.i2c.part.history.len();

        sensor.init(&mut delay).expect("second init");
        assert_eq!(sensor.i2c.part.history.len(), after_first);
    }

    /// The window is what a picture is judged against, so its registers are pinned
    /// number by number: the output rectangle, the read window centred in the array with
    /// the margins the part charges to its own line timing, the ratio, and the decimation
    /// group.
    #[test]
    fn set_window_programs_the_registers_the_picture_depends_on() {
        let mut sensor = sensor();
        let mut delay = MockDelay::new();
        sensor
            .set_window(
                POWER_ON_OUT_WIDTH,
                POWER_ON_OUT_HEIGHT,
                EXTRACT_RATIO,
                &mut delay,
            )
            .expect("set_window");

        let part = &sensor.i2c.part;
        assert_eq!(
            part.pair(REG_OUT_WIN_WIDTH_HIGH, REG_OUT_WIN_WIDTH_LOW),
            POWER_ON_OUT_WIDTH
        );
        assert_eq!(
            part.pair(REG_OUT_WIN_HEIGHT_HIGH, REG_OUT_WIN_HEIGHT_LOW),
            POWER_ON_OUT_HEIGHT
        );
        // The margins are asymmetric because that is what the reference driver writes.
        assert_eq!(part.pair(REG_ROW_START_HIGH, REG_ROW_START_LOW), 0);
        assert_eq!(part.pair(REG_COL_START_HIGH, REG_COL_START_LOW), 0);
        assert_eq!(
            part.pair(REG_WIN_HEIGHT_HIGH, REG_WIN_HEIGHT_LOW),
            POWER_ON_OUT_HEIGHT * EXTRACT_RATIO + WIN_HEIGHT_MARGIN
        );
        assert_eq!(
            part.pair(REG_WIN_WIDTH_HIGH, REG_WIN_WIDTH_LOW),
            POWER_ON_OUT_WIDTH * EXTRACT_RATIO + WIN_WIDTH_MARGIN
        );
        // 0x99 is one nibble per axis, so `5` is `0x55`.
        assert_eq!(part.get(REG_SUBSAMPLE), 0x55);
        assert_eq!(part.get(REG_SUBSAMPLE_MODE), SUBSAMPLE_MODE_PLAIN);
        assert_eq!(part.get(REG_CROP_ENABLE), 0x01);
        for reg in REG_SUB_ROW_N1..=REG_SUB_COL_N4 {
            assert_eq!(part.get(reg), 0x00, "bin register 0x{reg:02x}");
        }
        assert_eq!(part.get(REG_SCALAR_MODE), 0x00);
        assert_eq!(delay.waited_ms, vec![WINDOW_SETTLE_MS]);
    }

    /// A ratio that is written and ignored looks exactly like one that works from the
    /// registers alone, so the read-back is compared against what was asked for.
    #[test]
    fn reading_the_geometry_back_returns_what_was_programmed() {
        let mut sensor = sensor();
        let mut delay = MockDelay::new();
        sensor
            .set_window(
                POWER_ON_OUT_WIDTH,
                POWER_ON_OUT_HEIGHT,
                EXTRACT_RATIO,
                &mut delay,
            )
            .expect("set_window");

        let geometry = sensor.read_window_geometry().expect("geometry");
        assert_eq!(
            (geometry.out_width, geometry.out_height),
            (POWER_ON_OUT_WIDTH, POWER_ON_OUT_HEIGHT)
        );
        assert_eq!((geometry.row_start, geometry.col_start), (0, 0));
        assert_eq!(
            (geometry.win_width, geometry.win_height),
            (
                POWER_ON_OUT_WIDTH * EXTRACT_RATIO + WIN_WIDTH_MARGIN,
                POWER_ON_OUT_HEIGHT * EXTRACT_RATIO + WIN_HEIGHT_MARGIN
            )
        );
        assert_eq!(geometry.subsample, 0x55);
        assert_eq!(geometry.subsample_mode, SUBSAMPLE_MODE_PLAIN);
        assert_eq!(geometry.sub_bins, SUBSAMPLE_BINS);
        assert_eq!(geometry.scalar, 0x00);
        assert!(geometry.crop_enabled);
    }

    /// A window the part cannot produce has to be refused rather than programmed: a ratio
    /// needing a negative start would wrap to a huge unsigned one and read off the other
    /// edge of the part.
    #[test]
    fn a_window_the_part_cannot_produce_is_refused() {
        for (width, height, ratio) in [(0, 240, 5), (320, 0, 5), (640, 480, 5), (320, 240, 0)] {
            let mut sensor = sensor();
            let mut delay = MockDelay::new();
            let outcome = sensor.set_window(width, height, ratio, &mut delay);
            assert!(
                matches!(outcome, Err(Gc2145Error::WindowOutOfRange)),
                "{width}x{height} at ratio {ratio} must be refused, got {outcome:?}"
            );
            assert!(
                sensor.i2c.part.history.is_empty(),
                "a refused window must not reach the bus"
            );
        }
    }

    /// A NACK on the first read is what an absent part looks like on a shared bus, and a
    /// part answering with someone else's identity must not be taken for a GC2145.
    #[test]
    fn probe_separates_absent_from_wrong() {
        let mut absent = sensor();
        absent.i2c.part.silent = true;
        assert!(matches!(absent.probe(), Err(Gc2145Error::Absent)));

        let mut wrong = sensor();
        wrong.i2c.part.registers.insert((PAGE_2, CHIP_ID_LOW), 0x46);
        assert!(matches!(
            wrong.probe(),
            Err(Gc2145Error::WrongChip { found: 0x2146 })
        ));

        assert!(sensor().probe().is_ok());
    }

    /// Mirror is read-modify-written so it does not disturb the vertical flip sharing the
    /// register, and the format merge must not spill past its own five bits.
    #[test]
    fn the_control_bits_are_merged_not_overwritten() {
        let mut sensor = sensor();
        // The power-on table's mirror (bit 0) off, vertical flip (bit 1) on.
        sensor
            .i2c
            .part
            .registers
            .insert((PAGE_0, REG_ANALOG_MODE), 0x02);
        sensor.set_hmirror(true).expect("mirror on");
        assert_eq!(sensor.i2c.part.get(REG_ANALOG_MODE), 0x03);
        sensor.set_hmirror(false).expect("mirror off");
        assert_eq!(sensor.i2c.part.get(REG_ANALOG_MODE), 0x02);

        // Above the data-mode field sits output-path configuration the merge must keep.
        sensor
            .i2c
            .part
            .registers
            .insert((PAGE_0, REG_OUTPUT_FORMAT), 0xf2);
        sensor.set_format().expect("format");
        assert_eq!(sensor.i2c.part.get(REG_OUTPUT_FORMAT), 0xe6);
    }
}
