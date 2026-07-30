use std::io::Cursor;

use ogg::writing::{PacketWriteEndInfo, PacketWriter};

fn build_opus_head(channels: u8, sample_rate: u32) -> Vec<u8> {
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1); // version
    head.push(channels);
    head.extend_from_slice(&0u16.to_le_bytes()); // pre-skip = 0
    head.extend_from_slice(&sample_rate.to_le_bytes());
    head.extend_from_slice(&0i16.to_le_bytes()); // output gain = 0
    head.push(0); // channel mapping family = 0 (mono/stereo)
    head
}

fn build_opus_tags() -> Vec<u8> {
    let vendor = "vanling";
    let mut tags = Vec::new();
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor.as_bytes());
    tags.extend_from_slice(&0u32.to_le_bytes());
    tags
}

pub fn mux_opus_to_ogg(
    frames: &[Vec<u8>],
    frame_duration_ms: u64,
    channels: u8,
    sample_rate: u32,
) -> Result<Vec<u8>, std::io::Error> {
    let mut buf = Vec::new();
    {
        let mut cursor = Cursor::new(&mut buf);
        let mut writer = PacketWriter::new(&mut cursor);

        let head = build_opus_head(channels, sample_rate);
        writer.write_packet(head.as_slice(), 1, PacketWriteEndInfo::EndPage, 0)?;

        let tags = build_opus_tags();
        writer.write_packet(tags.as_slice(), 1, PacketWriteEndInfo::EndPage, 0)?;

        let granule_increment = (frame_duration_ms * 48_000) / 1000;
        for (i, frame) in frames.iter().enumerate() {
            let granule = ((i + 1) as u64) * granule_increment;
            let end_info = if i == frames.len() - 1 {
                PacketWriteEndInfo::EndStream
            } else {
                PacketWriteEndInfo::NormalPacket
            };
            writer.write_packet(frame.as_slice(), 1, end_info, granule)?;
        }
    }
    Ok(buf)
}
