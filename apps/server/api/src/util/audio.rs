use anyhow::Context;
use symphonia::core::audio::conv::FromSample;
use symphonia::core::audio::{Audio, GenericAudioBufferRef};
use symphonia::core::codecs::audio::{AudioDecoderOptions, CODEC_ID_NULL_AUDIO};

pub fn pcm_decode<P: AsRef<std::path::Path>>(path: P) -> Result<(Vec<f32>, u32), anyhow::Error> {
    fn conv<T>(samples: &mut Vec<f32>, data: &symphonia::core::audio::AudioBuffer<T>)
    where
        T: symphonia::core::audio::sample::Sample,
        f32: symphonia::core::audio::conv::FromSample<T>,
    {
        samples.extend(data.plane(0).unwrap().iter().map(|v| f32::from_sample(*v)))
    }

    // Open the media source.
    let src = std::fs::File::open(path).context("wrap")?;

    // Create the media source stream.
    let mss = symphonia::core::io::MediaSourceStream::new(Box::new(src), Default::default());

    // Create a probe hint using the file's extension. [Optional]
    let hint = symphonia::core::formats::probe::Hint::new();

    // Use the default options for metadata and format readers.
    let meta_opts: symphonia::core::meta::MetadataOptions = Default::default();
    let fmt_opts: symphonia::core::formats::FormatOptions = Default::default();

    // Probe the media source.
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, fmt_opts, meta_opts)
        .context("wrap")?;

    // Find the first audio track with a known (decodeable) codec.
    let track = format
        .tracks()
        .iter()
        .find(|t| {
            t.codec_params
                .as_ref()
                .and_then(|p| p.audio())
                .is_some_and(|a| a.codec != CODEC_ID_NULL_AUDIO)
        })
        .context("no supported audio tracks".to_string())?;

    // Use the default options for the decoder.
    let dec_opts: AudioDecoderOptions = Default::default();

    // Create a decoder for the track.
    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .context("no audio codec parameters")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(audio_params, &dec_opts)
        .context("unsupported codec")?;
    let track_id = track.id;
    let sample_rate = audio_params.sample_rate.unwrap_or(0);
    let mut pcm_data = Vec::new();
    // The decode loop.
    while let Ok(Some(packet)) = format.next_packet() {
        // Consume any new metadata that has been read since the last packet.
        while !format.metadata().is_latest() {
            format.metadata().pop();
        }

        // If the packet does not belong to the selected track, skip over it.
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet).context("wrap")? {
            GenericAudioBufferRef::F32(buf) => pcm_data.extend(buf.plane(0).unwrap()),
            GenericAudioBufferRef::U8(data) => conv(&mut pcm_data, data),
            GenericAudioBufferRef::U16(data) => conv(&mut pcm_data, data),
            GenericAudioBufferRef::U24(data) => conv(&mut pcm_data, data),
            GenericAudioBufferRef::U32(data) => conv(&mut pcm_data, data),
            GenericAudioBufferRef::S8(data) => conv(&mut pcm_data, data),
            GenericAudioBufferRef::S16(data) => conv(&mut pcm_data, data),
            GenericAudioBufferRef::S24(data) => conv(&mut pcm_data, data),
            GenericAudioBufferRef::S32(data) => conv(&mut pcm_data, data),
            GenericAudioBufferRef::F64(data) => conv(&mut pcm_data, data),
        }
    }
    Ok((pcm_data, sample_rate))
}
