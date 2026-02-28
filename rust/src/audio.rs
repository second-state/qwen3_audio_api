use crate::config::ResponseFormat;
use crate::error::ApiError;
use hound::{SampleFormat, WavSpec, WavWriter};
use std::io::Cursor;

/// Encode f32 audio samples to the requested format.
pub fn encode_audio(
    samples: &[f32],
    sample_rate: u32,
    format: ResponseFormat,
) -> Result<Vec<u8>, ApiError> {
    match format {
        ResponseFormat::Wav => encode_wav(samples, sample_rate),
        ResponseFormat::Pcm => Ok(encode_pcm(samples)),
        ResponseFormat::Flac | ResponseFormat::Mp3 | ResponseFormat::Opus | ResponseFormat::Aac => {
            encode_with_ffmpeg_lib(samples, sample_rate, format)
        }
    }
}

/// Encode as 16-bit PCM WAV (mono).
fn encode_wav(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, ApiError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut writer = WavWriter::new(&mut buffer, spec)
            .map_err(|e| ApiError::internal(format!("WAV encode error: {e}")))?;
        for &s in samples {
            let clamped = s.clamp(-1.0, 1.0);
            let i16_val = (clamped * 32767.0) as i16;
            writer
                .write_sample(i16_val)
                .map_err(|e| ApiError::internal(format!("WAV write error: {e}")))?;
        }
        writer
            .finalize()
            .map_err(|e| ApiError::internal(format!("WAV finalize error: {e}")))?;
    }
    Ok(buffer.into_inner())
}

/// Encode as raw 16-bit signed little-endian PCM.
fn encode_pcm(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let i16_val = (clamped * 32767.0) as i16;
        bytes.extend_from_slice(&i16_val.to_le_bytes());
    }
    bytes
}

/// Encode audio using the statically-linked ffmpeg library.
/// Directly creates ffmpeg audio frames from f32 samples (no WAV intermediate).
fn encode_with_ffmpeg_lib(
    samples: &[f32],
    sample_rate: u32,
    format: ResponseFormat,
) -> Result<Vec<u8>, ApiError> {
    ffmpeg_next::init()
        .map_err(|e| ApiError::internal(format!("Failed to initialize ffmpeg: {e}")))?;

    let codec_id = match format {
        ResponseFormat::Mp3 => ffmpeg_next::codec::Id::MP3,
        ResponseFormat::Opus => ffmpeg_next::codec::Id::OPUS,
        ResponseFormat::Aac => ffmpeg_next::codec::Id::AAC,
        ResponseFormat::Flac => ffmpeg_next::codec::Id::FLAC,
        _ => unreachable!(),
    };

    let encoder_codec = ffmpeg_next::encoder::find(codec_id)
        .ok_or_else(|| ApiError::internal(format!("Encoder not found for {codec_id:?}")))?;

    // Determine encoder's preferred sample format
    let default_sample_fmt = ffmpeg_next::format::Sample::I16(
        ffmpeg_next::format::sample::Type::Packed,
    );
    let enc_sample_format = encoder_codec
        .audio()
        .ok()
        .and_then(|a| a.formats())
        .and_then(|mut f| f.next())
        .unwrap_or(default_sample_fmt);

    let enc_rate = if codec_id == ffmpeg_next::codec::Id::OPUS {
        48000u32
    } else {
        sample_rate
    };

    // Create output file
    let mut dst_file = tempfile::Builder::new()
        .suffix(format.extension())
        .tempfile()
        .map_err(|e| ApiError::internal(format!("Failed to create output temp file: {e}")))?;
    let dst_path = dst_file.path().to_string_lossy().to_string();

    let mut octx = ffmpeg_next::format::output(&dst_path)
        .map_err(|e| ApiError::internal(format!("Failed to open output: {e}")))?;
    let _output_stream = octx
        .add_stream(encoder_codec)
        .map_err(|e| ApiError::internal(format!("Failed to add output stream: {e}")))?;

    // Configure encoder
    let mut context_encoder = ffmpeg_next::codec::context::Context::new_with_codec(encoder_codec);

    // If the output format requires global header, set the codec flag before opening
    if octx.format().flags().contains(ffmpeg_next::format::flag::Flags::GLOBAL_HEADER) {
        unsafe {
            (*context_encoder.as_mut_ptr()).flags |=
                ffmpeg_next::codec::flag::Flags::GLOBAL_HEADER.bits() as i32;
        }
    }

    let mut encoder = context_encoder
        .encoder()
        .audio()
        .map_err(|e| ApiError::internal(format!("Failed to create encoder: {e}")))?;

    encoder.set_rate(enc_rate as i32);
    encoder.set_channel_layout(ffmpeg_next::ChannelLayout::MONO);
    encoder.set_format(enc_sample_format);

    let mut encoder = encoder
        .open_as(encoder_codec)
        .map_err(|e| ApiError::internal(format!("Failed to open encoder: {e}")))?;

    octx.stream_mut(0)
        .ok_or_else(|| ApiError::internal("No output stream found"))?
        .set_parameters(&encoder);

    octx.write_header()
        .map_err(|e| ApiError::internal(format!("Failed to write output header: {e}")))?;

    let output_stream_time_base = octx.stream(0).unwrap().time_base();

    let src_format = ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Packed);

    // Check if we need a resampler (rate change or complex format conversion)
    // For same-rate S16 encoding, convert samples directly for better compatibility
    let needs_resampler = sample_rate != enc_rate
        || enc_sample_format != ffmpeg_next::format::Sample::I16(ffmpeg_next::format::sample::Type::Packed);

    let mut resampler = if needs_resampler {
        Some(
            ffmpeg_next::software::resampling::Context::get(
                src_format,
                ffmpeg_next::ChannelLayout::MONO,
                sample_rate,
                enc_sample_format,
                ffmpeg_next::ChannelLayout::MONO,
                enc_rate,
            )
            .map_err(|e| ApiError::internal(format!("Failed to create resampler: {e}")))?,
        )
    } else {
        None
    };

    // Process samples in chunks matching encoder frame_size
    let frame_size = if encoder.frame_size() > 0 {
        encoder.frame_size() as usize
    } else {
        1024
    };

    let mut pts: i64 = 0;
    let mut offset = 0;

    while offset < samples.len() {
        let chunk_len = std::cmp::min(frame_size, samples.len() - offset);
        let chunk = &samples[offset..offset + chunk_len];

        if let Some(ref mut resampler) = resampler {
            // Use resampler for rate conversion or complex format changes
            let mut frame = ffmpeg_next::frame::Audio::new(src_format, chunk_len, ffmpeg_next::ChannelLayout::MONO);
            frame.set_rate(sample_rate);
            frame.set_pts(Some(pts));

            let data = frame.data_mut(0);
            let byte_slice = unsafe {
                std::slice::from_raw_parts(chunk.as_ptr() as *const u8, chunk.len() * 4)
            };
            data[..byte_slice.len()].copy_from_slice(byte_slice);

            let mut resampled = ffmpeg_next::frame::Audio::empty();
            resampler
                .run(&frame, &mut resampled)
                .map_err(|e| ApiError::internal(format!("Resampler error: {e}")))?;

            if resampled.samples() > 0 {
                encoder
                    .send_frame(&resampled)
                    .map_err(|e| ApiError::internal(format!("Encoder send_frame error: {e}")))?;
                receive_and_write_packets(&mut encoder, &mut octx, output_stream_time_base)?;
            }
        } else {
            // Direct S16 frame creation (no resampler, better compatibility with FLAC etc.)
            let mut frame = ffmpeg_next::frame::Audio::new(
                enc_sample_format,
                chunk_len,
                ffmpeg_next::ChannelLayout::MONO,
            );
            frame.set_rate(enc_rate);
            frame.set_pts(Some(pts));

            // Convert f32 to i16
            let data = frame.data_mut(0);
            for (i, &s) in chunk.iter().enumerate() {
                let clamped = s.clamp(-1.0, 1.0);
                let i16_val = (clamped * 32767.0) as i16;
                let bytes = i16_val.to_le_bytes();
                data[i * 2] = bytes[0];
                data[i * 2 + 1] = bytes[1];
            }

            encoder
                .send_frame(&frame)
                .map_err(|e| ApiError::internal(format!("Encoder send_frame error: {e}")))?;
            receive_and_write_packets(&mut encoder, &mut octx, output_stream_time_base)?;
        }

        pts += chunk_len as i64;
        offset += chunk_len;
    }

    // Flush encoder
    encoder
        .send_eof()
        .map_err(|e| ApiError::internal(format!("Encoder send_eof error: {e}")))?;
    receive_and_write_packets(&mut encoder, &mut octx, output_stream_time_base)?;

    octx.write_trailer()
        .map_err(|e| ApiError::internal(format!("Failed to write output trailer: {e}")))?;

    use std::io::Read;
    let mut output = Vec::new();
    dst_file
        .read_to_end(&mut output)
        .map_err(|e| ApiError::internal(format!("Failed to read encoded output: {e}")))?;

    Ok(output)
}

fn receive_and_write_packets(
    encoder: &mut ffmpeg_next::encoder::Audio,
    octx: &mut ffmpeg_next::format::context::Output,
    time_base: ffmpeg_next::Rational,
) -> Result<(), ApiError> {
    let mut encoded_packet = ffmpeg_next::Packet::empty();
    while encoder.receive_packet(&mut encoded_packet).is_ok() {
        // Skip empty packets (e.g., from encoder flush)
        if encoded_packet.size() == 0 {
            continue;
        }
        encoded_packet.set_stream(0);
        encoded_packet.rescale_ts(encoder.time_base(), time_base);
        encoded_packet
            .write_interleaved(octx)
            .map_err(|e| ApiError::internal(format!("Failed to write packet: {e}")))?;
    }
    Ok(())
}

/// Apply speed adjustment by resampling via linear interpolation.
/// Speed > 1.0 = faster (shorter audio), speed < 1.0 = slower (longer audio).
pub fn apply_speed(samples: &[f32], speed: f32) -> Vec<f32> {
    if (speed - 1.0).abs() < f32::EPSILON || samples.is_empty() {
        return samples.to_vec();
    }

    let new_length = (samples.len() as f64 / speed as f64) as usize;
    if new_length == 0 {
        return samples.to_vec();
    }

    let mut output = Vec::with_capacity(new_length);
    for i in 0..new_length {
        let src_pos = i as f64 * speed as f64;
        let idx = src_pos as usize;
        let frac = (src_pos - idx as f64) as f32;
        if idx + 1 < samples.len() {
            output.push(samples[idx] * (1.0 - frac) + samples[idx + 1] * frac);
        } else if idx < samples.len() {
            output.push(samples[idx]);
        }
    }
    output
}

/// Convert audio bytes to WAV (mono, target_rate Hz, s16) using the ffmpeg library.
/// Returns the WAV file bytes.
pub fn convert_audio_to_wav_bytes(
    audio_bytes: &[u8],
    suffix: &str,
    target_rate: u32,
) -> Result<Vec<u8>, ApiError> {
    // Write source audio to a temp file
    let mut src_file = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .map_err(|e| ApiError::internal(format!("Failed to create temp file: {e}")))?;
    {
        use std::io::Write;
        src_file
            .write_all(audio_bytes)
            .map_err(|e| ApiError::internal(format!("Failed to write temp file: {e}")))?;
    }
    let src_path = src_file.path().to_string_lossy().to_string();

    let mut dst_file = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile()
        .map_err(|e| ApiError::internal(format!("Failed to create WAV temp file: {e}")))?;
    let dst_path = dst_file.path().to_string_lossy().to_string();

    transcode_to_wav(&src_path, &dst_path, target_rate)?;

    use std::io::Read;
    let mut output = Vec::new();
    dst_file
        .read_to_end(&mut output)
        .map_err(|e| ApiError::internal(format!("Failed to read WAV output: {e}")))?;

    Ok(output)
}

/// Transcode audio to mono WAV at the given sample rate using ffmpeg for decoding
/// and hound for WAV output. Decodes any audio format, converts to f32 samples,
/// resamples if needed, then writes as 16-bit PCM WAV.
fn transcode_to_wav(
    input_path: &str,
    output_path: &str,
    target_rate: u32,
) -> Result<(), ApiError> {
    ffmpeg_next::init()
        .map_err(|e| ApiError::internal(format!("Failed to initialize ffmpeg: {e}")))?;

    let mut ictx = ffmpeg_next::format::input(input_path)
        .map_err(|e| ApiError::internal(format!("Failed to open input: {e}")))?;

    let input_stream = ictx
        .streams()
        .best(ffmpeg_next::media::Type::Audio)
        .ok_or_else(|| ApiError::internal("No audio stream found in input"))?;
    let input_stream_index = input_stream.index();

    let context_decoder =
        ffmpeg_next::codec::context::Context::from_parameters(input_stream.parameters())
            .map_err(|e| ApiError::internal(format!("Failed to create decoder context: {e}")))?;
    let mut decoder = context_decoder
        .decoder()
        .audio()
        .map_err(|e| ApiError::internal(format!("Failed to open decoder: {e}")))?;

    // Decode all frames and extract f32 samples
    let mut all_samples: Vec<f32> = Vec::new();
    let mut decoded_frame = ffmpeg_next::frame::Audio::empty();
    let mut source_rate: Option<u32> = None;
    let mut source_channels: Option<u16> = None;

    let decode_frame = |decoder: &mut ffmpeg_next::decoder::Audio,
                            all_samples: &mut Vec<f32>,
                            decoded_frame: &mut ffmpeg_next::frame::Audio,
                            source_rate: &mut Option<u32>,
                            source_channels: &mut Option<u16>|
     -> Result<(), ApiError> {
        while decoder.receive_frame(decoded_frame).is_ok() {
            let rate = decoded_frame.rate();
            let channels = decoded_frame.channels() as u16;
            let samples = decoded_frame.samples();
            let format = decoded_frame.format();

            if source_rate.is_none() {
                *source_rate = Some(rate);
                *source_channels = Some(channels);
            }

            // Extract samples as f32 based on the decoded format
            for i in 0..samples {
                // For mono or downmix: average all channels
                let mut sum = 0.0f32;
                for ch in 0..channels as usize {
                    let sample = match format {
                        ffmpeg_next::format::Sample::I16(ffmpeg_next::format::sample::Type::Packed) => {
                            let data = decoded_frame.data(0);
                            let offset = (i * channels as usize + ch) * 2;
                            if offset + 1 < data.len() {
                                let val = i16::from_le_bytes([data[offset], data[offset + 1]]);
                                val as f32 / 32768.0
                            } else {
                                0.0
                            }
                        }
                        ffmpeg_next::format::Sample::I16(ffmpeg_next::format::sample::Type::Planar) => {
                            let data = decoded_frame.data(ch);
                            let offset = i * 2;
                            if offset + 1 < data.len() {
                                let val = i16::from_le_bytes([data[offset], data[offset + 1]]);
                                val as f32 / 32768.0
                            } else {
                                0.0
                            }
                        }
                        ffmpeg_next::format::Sample::I32(ffmpeg_next::format::sample::Type::Packed) => {
                            let data = decoded_frame.data(0);
                            let offset = (i * channels as usize + ch) * 4;
                            if offset + 3 < data.len() {
                                let val = i32::from_le_bytes([
                                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                                ]);
                                val as f32 / 2147483648.0
                            } else {
                                0.0
                            }
                        }
                        ffmpeg_next::format::Sample::I32(ffmpeg_next::format::sample::Type::Planar) => {
                            let data = decoded_frame.data(ch);
                            let offset = i * 4;
                            if offset + 3 < data.len() {
                                let val = i32::from_le_bytes([
                                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                                ]);
                                val as f32 / 2147483648.0
                            } else {
                                0.0
                            }
                        }
                        ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Packed) => {
                            let data = decoded_frame.data(0);
                            let offset = (i * channels as usize + ch) * 4;
                            if offset + 3 < data.len() {
                                f32::from_le_bytes([
                                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                                ])
                            } else {
                                0.0
                            }
                        }
                        ffmpeg_next::format::Sample::F32(ffmpeg_next::format::sample::Type::Planar) => {
                            let data = decoded_frame.data(ch);
                            let offset = i * 4;
                            if offset + 3 < data.len() {
                                f32::from_le_bytes([
                                    data[offset], data[offset + 1], data[offset + 2], data[offset + 3],
                                ])
                            } else {
                                0.0
                            }
                        }
                        _ => 0.0,
                    };
                    sum += sample;
                }
                all_samples.push(sum / channels as f32);
            }
        }
        Ok(())
    };

    for (stream, packet) in ictx.packets() {
        if stream.index() != input_stream_index {
            continue;
        }
        decoder
            .send_packet(&packet)
            .map_err(|e| ApiError::internal(format!("Decoder send_packet error: {e}")))?;
        decode_frame(
            &mut decoder,
            &mut all_samples,
            &mut decoded_frame,
            &mut source_rate,
            &mut source_channels,
        )?;
    }

    // Flush decoder
    decoder
        .send_eof()
        .map_err(|e| ApiError::internal(format!("Decoder send_eof error: {e}")))?;
    decode_frame(
        &mut decoder,
        &mut all_samples,
        &mut decoded_frame,
        &mut source_rate,
        &mut source_channels,
    )?;

    if all_samples.is_empty() {
        return Err(ApiError::internal("No audio samples decoded from input"));
    }

    let src_rate = source_rate.unwrap_or(target_rate);

    // Resample if needed (simple linear interpolation)
    let resampled = if src_rate != target_rate {
        let ratio = target_rate as f64 / src_rate as f64;
        let new_len = (all_samples.len() as f64 * ratio) as usize;
        let mut output = Vec::with_capacity(new_len);
        for i in 0..new_len {
            let src_pos = i as f64 / ratio;
            let idx = src_pos as usize;
            let frac = (src_pos - idx as f64) as f32;
            if idx + 1 < all_samples.len() {
                output.push(all_samples[idx] * (1.0 - frac) + all_samples[idx + 1] * frac);
            } else if idx < all_samples.len() {
                output.push(all_samples[idx]);
            }
        }
        output
    } else {
        all_samples
    };

    // Write as 16-bit PCM WAV using hound
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: target_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(output_path, spec)
        .map_err(|e| ApiError::internal(format!("Failed to create WAV writer: {e}")))?;
    for &s in &resampled {
        let clamped = s.clamp(-1.0, 1.0);
        let i16_val = (clamped * 32767.0) as i16;
        writer
            .write_sample(i16_val)
            .map_err(|e| ApiError::internal(format!("WAV write error: {e}")))?;
    }
    writer
        .finalize()
        .map_err(|e| ApiError::internal(format!("WAV finalize error: {e}")))?;

    Ok(())
}
