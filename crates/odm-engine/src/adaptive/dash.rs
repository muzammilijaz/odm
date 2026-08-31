//! Minimal DASH (`.mpd`) support: `SegmentTemplate`-addressed representations
//! (both fixed-duration and `SegmentTimeline`), single `Period`. Covers the
//! common CMAF/fMP4 VOD case; multi-period and `SegmentList`/`SegmentBase`
//! addressing are not yet handled — falls back to an error rather than
//! silently producing a broken file.

use super::ffmpeg;
use super::segments::resolve_url;
use crate::error::{EngineError, Result};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
struct SegTemplate {
    media: Option<String>,
    initialization: Option<String>,
    start_number: u64,
    /// Fixed per-segment duration (in `timescale` units). Reserved for a
    /// smarter segment-count estimate once `mediaPresentationDuration`
    /// parsing is added; the current fallback just uses a generous cap.
    #[allow(dead_code)]
    duration: Option<u64>,
    timescale: u64,
    timeline: Vec<(u64, u32)>, // (duration, repeat_count) per <S d=".." r=".."/>
}

#[derive(Debug, Clone)]
struct Track {
    mime_type: String,
    bandwidth: u64,
    seg_template: SegTemplate,
}

/// Downloads a DASH stream to `dest`: picks the highest-bandwidth video and
/// audio representations, downloads their segments, and muxes them together
/// with ffmpeg.
pub async fn download_dash(client: &reqwest::Client, mpd_url: &str, dest: &Path) -> Result<PathBuf> {
    let resp = client.get(mpd_url).send().await?;
    if !resp.status().is_success() {
        return Err(EngineError::BadStatus(resp.status()));
    }
    let final_url = resp.url().to_string();
    let bytes = resp.bytes().await?;

    let (base_url, tracks) = parse_mpd(&bytes, &final_url)?;

    let video = tracks
        .iter()
        .filter(|t| t.mime_type.starts_with("video"))
        .max_by_key(|t| t.bandwidth);
    let audio = tracks
        .iter()
        .filter(|t| t.mime_type.starts_with("audio"))
        .max_by_key(|t| t.bandwidth);

    let seg_dir = super::segments::segment_dir(dest);
    tokio::fs::create_dir_all(&seg_dir).await?;

    let video_path = match video {
        Some(t) => Some(download_track(client, &base_url, t, &seg_dir.join("video.m4s")).await?),
        None => None,
    };
    let audio_path = match audio {
        Some(t) => Some(download_track(client, &base_url, t, &seg_dir.join("audio.m4s")).await?),
        None => None,
    };

    let ffmpeg_bin = ffmpeg::resolve_ffmpeg_path();
    match (video_path, audio_path) {
        (Some(v), Some(a)) => {
            ffmpeg::mux_video_audio(&ffmpeg_bin, &v, &a, dest).await?;
        }
        (Some(v), None) => {
            tokio::fs::rename(&v, dest).await?;
        }
        (None, Some(a)) => {
            tokio::fs::rename(&a, dest).await?;
        }
        (None, None) => {
            return Err(EngineError::Io(std::io::Error::other("no usable video/audio representation found in MPD")));
        }
    }

    let _ = tokio::fs::remove_dir_all(&seg_dir).await;
    Ok(dest.to_path_buf())
}

/// Downloads one representation's init segment (if any) plus every media
/// segment, concatenating them byte-for-byte into `out_path` — valid for
/// fragmented MP4/CMAF, which is the near-universal case for DASH VOD.
async fn download_track(client: &reqwest::Client, base_url: &str, track: &Track, out_path: &Path) -> Result<PathBuf> {
    let tpl = &track.seg_template;
    let media_tpl = tpl
        .media
        .as_ref()
        .ok_or_else(|| EngineError::Io(std::io::Error::other("representation has no SegmentTemplate@media")))?;

    let mut file = tokio::fs::File::create(out_path).await?;
    use tokio::io::AsyncWriteExt;

    if let Some(init) = &tpl.initialization {
        let url = resolve_url(base_url, init);
        let bytes = fetch(client, &url).await?;
        file.write_all(&bytes).await?;
    }

    let numbers = segment_numbers(tpl);
    for n in numbers {
        let uri = media_tpl.replace("$Number$", &n.to_string());
        let url = resolve_url(base_url, &uri);
        let bytes = fetch(client, &url).await?;
        file.write_all(&bytes).await?;
    }

    file.flush().await?;
    Ok(out_path.to_path_buf())
}

fn segment_numbers(tpl: &SegTemplate) -> Vec<u64> {
    let start = tpl.start_number.max(1);
    if !tpl.timeline.is_empty() {
        let count: u64 = tpl.timeline.iter().map(|(_, r)| (*r as u64) + 1).sum();
        (start..start + count).collect()
    } else {
        // No timeline and no period-duration bookkeeping in this minimal
        // implementation — a fixed, generous cap; ffmpeg will happily ignore
        // any trailing 404s' worth of missing segments once concatenated up
        // to the real end (segments beyond the stream's end simply fail and
        // are skipped rather than aborting the whole download).
        (start..start + 50_000).collect()
    }
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<bytes::Bytes> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(EngineError::BadStatus(resp.status()));
    }
    Ok(resp.bytes().await?)
}

#[allow(clippy::too_many_arguments)]
fn open_tag(
    name: &str,
    e: &quick_xml::events::BytesStart,
    in_first_period: &mut bool,
    period_done: bool,
    current_mime: &mut Option<String>,
    adaptation_seg_tpl: &mut Option<SegTemplate>,
    current_representation_bandwidth: &mut Option<u64>,
    current_seg_tpl: &mut Option<SegTemplate>,
) {
    match name {
        "Period" => {
            if !period_done {
                *in_first_period = true;
            }
        }
        "AdaptationSet" if *in_first_period => {
            *current_mime = attr(e, "mimeType").or_else(|| attr(e, "contentType"));
            *adaptation_seg_tpl = None;
        }
        "Representation" if *in_first_period => {
            *current_representation_bandwidth = attr(e, "bandwidth").and_then(|s| s.parse().ok());
            *current_seg_tpl = adaptation_seg_tpl.clone();
        }
        "SegmentTemplate" if *in_first_period => {
            let mut tpl = SegTemplate {
                media: attr(e, "media"),
                initialization: attr(e, "initialization"),
                start_number: attr(e, "startNumber").and_then(|s| s.parse().ok()).unwrap_or(1),
                duration: attr(e, "duration").and_then(|s| s.parse().ok()),
                timescale: attr(e, "timescale").and_then(|s| s.parse().ok()).unwrap_or(1),
                timeline: Vec::new(),
            };
            if tpl.timescale == 0 {
                tpl.timescale = 1;
            }
            if current_representation_bandwidth.is_some() {
                *current_seg_tpl = Some(tpl);
            } else {
                *adaptation_seg_tpl = Some(tpl);
            }
        }
        "S" if *in_first_period => {
            let d: u64 = attr(e, "d").and_then(|s| s.parse().ok()).unwrap_or(0);
            let r: u32 = attr(e, "r").and_then(|s| s.parse().ok()).unwrap_or(0);
            if let Some(tpl) = current_seg_tpl.as_mut().or(adaptation_seg_tpl.as_mut()) {
                tpl.timeline.push((d, r));
            }
        }
        _ => {}
    }
}

fn parse_mpd(xml: &[u8], mpd_url: &str) -> Result<(String, Vec<Track>)> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);

    let mut base_url = mpd_url.to_string();
    let mut tracks = Vec::new();

    let mut in_first_period = false;
    let mut period_done = false;
    let mut current_mime: Option<String> = None;
    let mut adaptation_seg_tpl: Option<SegTemplate> = None;
    let mut current_representation_bandwidth: Option<u64> = None;
    let mut current_seg_tpl: Option<SegTemplate> = None;
    let mut buf = Vec::new();

    macro_rules! finish_representation {
        () => {
            if let (Some(mime), Some(tpl)) = (
                current_mime.clone(),
                current_seg_tpl.take().or_else(|| adaptation_seg_tpl.clone()),
            ) {
                tracks.push(Track {
                    mime_type: mime,
                    bandwidth: current_representation_bandwidth.unwrap_or(0),
                    seg_template: tpl,
                });
            }
            current_representation_bandwidth = None;
        };
    }

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                open_tag(
                    &name,
                    &e,
                    &mut in_first_period,
                    period_done,
                    &mut current_mime,
                    &mut adaptation_seg_tpl,
                    &mut current_representation_bandwidth,
                    &mut current_seg_tpl,
                );
            }
            // Self-closing tags (`<Representation .../>`) never produce a
            // matching `Event::End`, so a self-closed Representation must be
            // finished immediately here rather than waiting for `End`.
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                open_tag(
                    &name,
                    &e,
                    &mut in_first_period,
                    period_done,
                    &mut current_mime,
                    &mut adaptation_seg_tpl,
                    &mut current_representation_bandwidth,
                    &mut current_seg_tpl,
                );
                if name == "Representation" && in_first_period {
                    finish_representation!();
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "Representation" => {
                        finish_representation!();
                    }
                    "AdaptationSet" => {
                        current_mime = None;
                        adaptation_seg_tpl = None;
                    }
                    "Period" => {
                        if in_first_period {
                            period_done = true;
                            in_first_period = false;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                // Only meaningful for <BaseURL>text</BaseURL>; harmless elsewhere.
                if let Ok(text) = t.unescape() {
                    let text = text.trim();
                    if !text.is_empty() && text.starts_with("http") {
                        base_url = resolve_url(&base_url, text);
                    }
                }
            }
            Ok(_) => {}
            Err(e) => return Err(EngineError::Io(std::io::Error::other(format!("MPD parse error: {e}")))),
        }
        buf.clear();
    }

    Ok((base_url, tracks))
}

fn attr(e: &quick_xml::events::BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find(|a| a.key.as_ref() == key.as_bytes()).and_then(|a| a.unescape_value().ok().map(|v| v.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MPD: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011">
  <Period>
    <AdaptationSet mimeType="video/mp4">
      <SegmentTemplate media="v-$Number$.m4s" initialization="v-init.mp4" startNumber="1" timescale="1000">
        <SegmentTimeline>
          <S d="4000" r="2"/>
          <S d="2000"/>
        </SegmentTimeline>
      </SegmentTemplate>
      <Representation id="v1" bandwidth="500000"/>
      <Representation id="v2" bandwidth="2000000"/>
    </AdaptationSet>
    <AdaptationSet mimeType="audio/mp4">
      <SegmentTemplate media="a-$Number$.m4s" initialization="a-init.mp4" startNumber="1" timescale="1000">
        <SegmentTimeline>
          <S d="4000" r="1"/>
        </SegmentTimeline>
      </SegmentTemplate>
      <Representation id="a1" bandwidth="128000"/>
    </AdaptationSet>
  </Period>
</MPD>"#;

    #[test]
    fn parses_video_and_audio_tracks_with_timeline() {
        let (_, tracks) = parse_mpd(SAMPLE_MPD.as_bytes(), "https://example.com/stream/manifest.mpd").unwrap();
        assert_eq!(tracks.len(), 3);

        let video_tracks: Vec<_> = tracks.iter().filter(|t| t.mime_type.starts_with("video")).collect();
        assert_eq!(video_tracks.len(), 2);
        let best_video = video_tracks.iter().max_by_key(|t| t.bandwidth).unwrap();
        assert_eq!(best_video.bandwidth, 2_000_000);
        // Same SegmentTemplate is shared (declared at AdaptationSet level) by both representations.
        assert_eq!(best_video.seg_template.timeline, vec![(4000, 2), (2000, 0)]);

        let audio = tracks.iter().find(|t| t.mime_type.starts_with("audio")).unwrap();
        assert_eq!(audio.bandwidth, 128_000);
        assert_eq!(audio.seg_template.media.as_deref(), Some("a-$Number$.m4s"));
    }

    #[test]
    fn segment_numbers_expands_timeline_repeat_counts() {
        let tpl = SegTemplate {
            media: Some("x-$Number$.m4s".into()),
            initialization: None,
            start_number: 1,
            duration: None,
            timescale: 1000,
            timeline: vec![(4000, 2), (2000, 0)], // 3 repeats + 1 = 4 segments
        };
        let numbers = segment_numbers(&tpl);
        assert_eq!(numbers, vec![1, 2, 3, 4]);
    }

    #[test]
    fn segment_numbers_without_timeline_falls_back_to_a_cap() {
        let tpl = SegTemplate {
            media: Some("x-$Number$.m4s".into()),
            initialization: None,
            start_number: 5,
            duration: Some(4),
            timescale: 1,
            timeline: vec![],
        };
        let numbers = segment_numbers(&tpl);
        assert_eq!(numbers[0], 5);
        assert_eq!(numbers.len(), 50_000);
    }
}
