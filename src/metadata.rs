use anyhow::{Context, Result};
use regex::Regex;
use std::io::Read;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct TrackMetadata {
    pub artist: Option<String>,
    pub title: Option<String>,
}

impl TrackMetadata {
    #[allow(dead_code)]
    pub fn format(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("🎵 {} - {}", artist, title),
            (None, Some(title)) => format!("🎵 {}", title),
            _ => "🎵 [No track information]".to_string(),
        }
    }
}

pub struct MetadataExtractor {
    current_metadata: Arc<Mutex<TrackMetadata>>,
    running: Arc<Mutex<bool>>,
}

impl MetadataExtractor {
    pub fn new() -> Self {
        MetadataExtractor {
            current_metadata: Arc::new(Mutex::new(TrackMetadata {
                artist: None,
                title: None,
            })),
            running: Arc::new(Mutex::new(false)),
        }
    }

    pub async fn start(&self, url: &str) -> Result<()> {
        self.stop().await;

        let mut running = self.running.lock().unwrap();
        *running = true;
        drop(running);

        let url = url.to_string();
        let metadata = Arc::clone(&self.current_metadata);
        let running = Arc::clone(&self.running);

        tokio::task::spawn_blocking(move || {
            if let Err(_e) = Self::monitor_stream_blocking(&url, metadata, running) {
                // Silently handle errors - not all streams support metadata
            }
        });

        Ok(())
    }

    pub async fn stop(&self) {
        let mut running = self.running.lock().unwrap();
        *running = false;
    }

    #[allow(dead_code)]
    pub fn get_metadata(&self) -> TrackMetadata {
        self.current_metadata.lock().unwrap().clone()
    }

    #[allow(dead_code)]
    fn monitor_stream_blocking(
        url: &str,
        metadata: Arc<Mutex<TrackMetadata>>,
        running: Arc<Mutex<bool>>,
    ) -> Result<()> {
        let client = reqwest::blocking::Client::new();

        let response = client
            .get(url)
            .header("Icy-MetaData", "1")
            .header("User-Agent", "RadioPlayer/1.0")
            .timeout(Duration::from_secs(30))
            .send()
            .context("Failed to connect to stream")?;

        let meta_interval = response
            .headers()
            .get("icy-metaint")
            .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);

        if meta_interval == 0 {
            // No ICY metadata supported
            return Ok(());
        }

        let mut buffer: Vec<u8> = Vec::new();
        let mut bytes_read: usize = 0;
        let mut stream = response;

        loop {
            // Check if should stop
            if !*running.lock().unwrap() {
                break;
            }

            // Read next chunk
            let mut chunk = vec![0u8; 8192];
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    buffer.extend_from_slice(&chunk[..n]);
                    bytes_read += n;
                }
                Err(_) => break,
            }

            // Process metadata at intervals
            while bytes_read >= meta_interval + 1 && buffer.len() >= meta_interval + 1 {
                // Skip audio data
                let audio_data = buffer.split_off(meta_interval);
                buffer = audio_data;
                bytes_read -= meta_interval;

                if buffer.is_empty() {
                    break;
                }

                // Read metadata length
                let meta_len_byte = buffer[0] as usize;
                let meta_len = meta_len_byte * 16;
                buffer.drain(0..1);

                if meta_len > 0 && buffer.len() >= meta_len {
                    let meta_data: Vec<u8> = buffer.drain(0..meta_len).collect();
                    if let Ok(text) = String::from_utf8(meta_data) {
                        let parsed = Self::parse_metadata(&text);
                        if parsed.artist.is_some() || parsed.title.is_some() {
                            let mut current = metadata.lock().unwrap();
                            *current = parsed;
                        }
                    }
                }
            }

            // Prevent buffer from growing too large
            if buffer.len() > meta_interval * 2 {
                buffer.clear();
                bytes_read = 0;
            }
        }

        Ok(())
    }

    fn parse_metadata(data: &str) -> TrackMetadata {
        // Try to extract StreamTitle
        let re = Regex::new(r"StreamTitle='([^']*)'").unwrap();

        if let Some(caps) = re.captures(data) {
            let full_title = caps.get(1).map(|m| m.as_str()).unwrap_or("");

            // Try to split by common separators
            let separators = [" - ", " – ", " — ", " ~ "];

            for sep in &separators {
                if let Some(pos) = full_title.find(sep) {
                    let artist = full_title[..pos].trim().to_string();
                    let title = full_title[pos + sep.len()..].trim().to_string();
                    return TrackMetadata {
                        artist: if artist.is_empty() { None } else { Some(artist) },
                        title: if title.is_empty() { None } else { Some(title) },
                    };
                }
            }

            // No separator found, treat as title only
            return TrackMetadata {
                artist: None,
                title: Some(full_title.to_string()),
            };
        }

        TrackMetadata {
            artist: None,
            title: None,
        }
    }
}
