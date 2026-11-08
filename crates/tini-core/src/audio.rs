//! Audio files: decoding (wav, flac, mp3, ogg, m4a...), mono float32, 16-bit PCM WAV writing (with a RIFF-INFO
//! "synthetic" tag), MP3 export, line joining.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::Error;

pub const SAMPLE_RATE: u32 = 48_000;

/// Mono audio: samples in [-1, 1] and sample rate.
#[derive(Debug, Clone)]
pub struct Audio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Audio {
    pub fn seconds(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }
}

/// Fast WAV reader (hound); multichannel input is averaged.
pub fn read_wav(path: &Path) -> Result<Audio, Error> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| Error::Audio(format!("{}: {e}", path.display())))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()
        }
    }
    .map_err(|e| Error::Audio(format!("{}: {e}", path.display())))?;
    Ok(Audio {
        samples: downmix(&raw, channels),
        sample_rate: spec.sample_rate,
    })
}

fn downmix(raw: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return raw.to_vec();
    }
    raw.chunks(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Decodes any audio file (symphonia): wav, flac, mp3, ogg/vorbis, m4a/aac, aiff...
pub fn decode(path: &Path) -> Result<Audio, Error> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let fail = |e: String| Error::Audio(format!("ses dosyası okunamadı ({}): {e}", path.display()));
    let file = fs::File::open(path).map_err(|e| fail(e.to_string()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| fail(e.to_string()))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| fail("ses izi yok".into()))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| fail(e.to_string()))?;
    let mut rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(_)) => break,
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(fail(e.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(fail(e.to_string())),
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        samples.extend(downmix(buffer.samples(), spec.channels.count().max(1)));
    }
    if rate == 0 || samples.is_empty() {
        return Err(fail("boş ses".into()));
    }
    Ok(Audio {
        samples,
        sample_rate: rate,
    })
}

/// Loads audio: hound for WAV, otherwise symphonia.
pub fn load(path: &Path) -> Result<Audio, Error> {
    let is_wav = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("wav"))
        .unwrap_or(false);
    if is_wav {
        if let Ok(s) = read_wav(path) {
            return Ok(s);
        }
    }
    decode(path)
}

/// RIFF-INFO tags (ASCII).
#[derive(Debug, Clone, Default)]
pub struct Tag {
    pub title: String,
    pub comment: String,
}

impl Tag {
    pub fn synthetic(title: &str) -> Tag {
        Tag {
            title: title
                .chars()
                .filter(|c| c.is_ascii() && !c.is_ascii_control())
                .collect(),
            comment: "AI-generated synthetic speech (VoxCPM2) / Yapay zeka ile uretilmis sestir"
                .into(),
        }
    }
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

fn info_chunk(tag: &Tag) -> Vec<u8> {
    let mut body = b"INFO".to_vec();
    for (id, text) in [
        (b"INAM", &tag.title),
        (b"ICMT", &tag.comment),
        (b"ISFT", &"Tini".to_string()),
    ] {
        if text.is_empty() {
            continue;
        }
        let mut bytes: Vec<u8> = text.bytes().filter(|b| b.is_ascii()).collect();
        bytes.push(0);
        body.extend_from_slice(id);
        body.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        body.extend_from_slice(&bytes);
        if bytes.len() % 2 == 1 {
            body.push(0);
        }
    }
    let mut chunk = b"LIST".to_vec();
    chunk.extend_from_slice(&(body.len() as u32).to_le_bytes());
    chunk.extend(body);
    chunk
}

/// Writes a 16-bit PCM mono WAV (temp file + rename). With a tag, a LIST/INFO chunk is added.
pub fn write_wav(path: &Path, audio: &Audio, tag: Option<&Tag>) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data_len = (audio.samples.len() * 2) as u32;
    let info = tag.map(info_chunk).unwrap_or_default();
    let mut out = Vec::with_capacity(44 + info.len() + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(4 + 24 + info.len() as u32 + 8 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&audio.sample_rate.to_le_bytes());
    out.extend_from_slice(&(audio.sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend(info);
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in &audio.samples {
        out.extend_from_slice(&to_i16(*s).to_le_bytes());
    }
    let tmp = path.with_extension("tmp.wav");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&out)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// MP3 (LAME, 192 kbps CBR, with an ID3 tag).
pub fn write_mp3(path: &Path, audio: &Audio, title: &str) -> Result<(), Error> {
    use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, Id3Tag, MonoPcm, Quality};
    let err = |e: String| Error::Audio(format!("mp3 yazılamadı: {e}"));
    let mut builder = Builder::new().ok_or_else(|| err("LAME başlatılamadı".into()))?;
    builder
        .set_num_channels(1)
        .map_err(|e| err(format!("{e:?}")))?;
    builder
        .set_sample_rate(audio.sample_rate)
        .map_err(|e| err(format!("{e:?}")))?;
    builder
        .set_brate(Bitrate::Kbps192)
        .map_err(|e| err(format!("{e:?}")))?;
    builder
        .set_quality(Quality::Best)
        .map_err(|e| err(format!("{e:?}")))?;
    let title: Vec<u8> = title.bytes().filter(|b| b.is_ascii()).collect();
    let _ = builder.set_id3_tag(Id3Tag {
        title: &title,
        artist: b"Tini (AI-generated)",
        album: &[],
        year: &[],
        comment: b"AI-generated synthetic speech (VoxCPM2)",
        album_art: &[],
    });
    let mut encoder = builder.build().map_err(|e| err(format!("{e:?}")))?;
    let pcm: Vec<i16> = audio.samples.iter().map(|s| to_i16(*s)).collect();
    let mut buffer = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(pcm.len()));
    let written = encoder
        .encode(MonoPcm(&pcm), buffer.spare_capacity_mut())
        .map_err(|e| err(format!("{e:?}")))?;
    unsafe { buffer.set_len(written) };
    let flushed = encoder
        .flush::<FlushNoGap>(buffer.spare_capacity_mut())
        .map_err(|e| err(format!("{e:?}")))?;
    unsafe { buffer.set_len(buffer.len() + flushed) };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, buffer)?;
    Ok(())
}

pub fn silence(seconds: f64, sample_rate: u32) -> Vec<f32> {
    vec![0.0; (seconds.max(0.0) * sample_rate as f64) as usize]
}

/// Joins lines with pauses between them (audio itself is untouched).
pub fn join_lines(parts: &[Vec<f32>], pauses: &[f64], sample_rate: u32) -> Vec<f32> {
    let mut out = Vec::new();
    for (index, audio) in parts.iter().enumerate() {
        if index > 0 {
            out.extend(silence(
                pauses.get(index - 1).copied().unwrap_or(0.35),
                sample_rate,
            ));
        }
        out.extend_from_slice(audio);
    }
    out
}

/// Below this margin a reference has audible background noise (hiss or hum) that VoxCPM copies into every reading.
/// Measured: clean references sit at 50-57 dB, noisy design variants at 25-37 dB.
pub const NOISY_MARGIN_DB: f64 = 40.0;

/// Background-noise margin in dB: level of the speech (90th percentile of 2048-sample frame RMS) minus the level of
/// the quietest 10 % of frames. Digital silence (exact zeros) is ignored. `None` for clips shorter than ~0.1 s.
pub fn noise_margin_db(samples: &[f32]) -> Option<f64> {
    const FRAME: usize = 2048;
    const HOP: usize = 512;
    if samples.len() < FRAME * 4 {
        return None;
    }
    let mut rms: Vec<f64> = samples
        .windows(FRAME)
        .step_by(HOP)
        .map(|w| (w.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / FRAME as f64).sqrt())
        .filter(|r| *r > 1e-6)
        .collect();
    if rms.len() < 10 {
        return None;
    }
    rms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let speech = rms[(rms.len() as f64 * 0.9) as usize];
    let quiet = &rms[..(rms.len() / 10).max(1)];
    let floor = (quiet.iter().map(|r| r * r).sum::<f64>() / quiet.len() as f64).sqrt();
    Some((20.0 * (speech / floor).log10() * 10.0).round() / 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_margin() {
        let sr = 48_000usize;
        // one second of "speech" (0.3 amplitude tone) between two quiet stretches with a noise floor
        let mut clean = Vec::new();
        let mut noisy = Vec::new();
        let mut seed = 1u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
        };
        for i in 0..sr * 3 {
            let tone = if (sr..2 * sr).contains(&i) {
                0.3 * (i as f32 * 0.05).sin()
            } else {
                0.0
            };
            let n = noise();
            clean.push(tone + n * 0.0002);
            noisy.push(tone + n * 0.02);
        }
        let c = noise_margin_db(&clean).unwrap();
        let n = noise_margin_db(&noisy).unwrap();
        assert!(c > NOISY_MARGIN_DB, "clean {c}");
        assert!(n < NOISY_MARGIN_DB, "noisy {n}");
        assert!(noise_margin_db(&[0.0; 100]).is_none());
    }

    #[test]
    fn wav_round_trip_and_tag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        let audio = Audio {
            samples: (0..4800).map(|i| (i as f32 / 100.0).sin() * 0.5).collect(),
            sample_rate: 48000,
        };
        write_wav(&path, &audio, Some(&Tag::synthetic("Deneme Şükrü"))).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.windows(4).any(|w| w == b"INFO"));
        let back = read_wav(&path).unwrap();
        assert_eq!(back.sample_rate, 48000);
        assert_eq!(back.samples.len(), 4800);
        assert!((back.samples[100] - audio.samples[100]).abs() < 1e-3);
        let decoded = decode(&path).unwrap();
        assert_eq!(decoded.samples.len(), 4800);
        write_mp3(&dir.path().join("a.mp3"), &audio, "deneme").unwrap();
        assert!(fs::metadata(dir.path().join("a.mp3")).unwrap().len() > 100);
    }

    #[test]
    fn join() {
        let joined = join_lines(&[vec![1.0; 10], vec![1.0; 10]], &[0.5], 100);
        assert_eq!(joined.len(), 70);
    }
}
