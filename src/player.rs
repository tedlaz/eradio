use anyhow::{Context, Result};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct RadioPlayer {
    _stream: Option<OutputStream>,
    stream_handle: Option<OutputStreamHandle>,
    sink: Arc<Mutex<Option<Sink>>>,
    playing: Arc<AtomicBool>,
}

impl RadioPlayer {
    pub fn new() -> Self {
        let (stream, stream_handle) = match OutputStream::try_default() {
            Ok((s, sh)) => (Some(s), Some(sh)),
            Err(e) => {
                eprintln!("Warning: Could not create audio output stream: {}", e);
                (None, None)
            }
        };

        RadioPlayer {
            _stream: stream,
            stream_handle,
            sink: Arc::new(Mutex::new(None)),
            playing: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_available(&self) -> bool {
        self.stream_handle.is_some()
    }

    pub fn get_player_name(&self) -> String {
        if self.is_available() {
            "embedded (rodio)".to_string()
        } else {
            "None".to_string()
        }
    }

    pub fn play(&self, url: &str) -> Result<bool> {
        self.stop()?;

        if self.stream_handle.is_none() {
            println!("Error: No audio output available!");
            return Ok(false);
        }

        let stream_handle = self.stream_handle.as_ref().unwrap().clone();
        let sink = Sink::try_new(&stream_handle)?;

        {
            let mut current_sink = self.sink.lock().unwrap();
            *current_sink = Some(sink);
        }

        let url = url.to_string();
        let sink_arc = Arc::clone(&self.sink);
        let playing = Arc::clone(&self.playing);

        thread::spawn(move || {
            playing.store(true, Ordering::SeqCst);
            if let Err(e) = Self::stream_audio_blocking(&url, &sink_arc, &playing) {
                eprintln!("Stream error: {}", e);
            }
            playing.store(false, Ordering::SeqCst);
        });

        Ok(true)
    }

    fn stream_audio_blocking(
        url: &str,
        sink_arc: &Arc<Mutex<Option<Sink>>>,
        playing: &Arc<AtomicBool>,
    ) -> Result<()> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        let response = client
            .get(url)
            .header("User-Agent", "RadioPlayer/1.0")
            .header("Accept", "*/*")
            .header("Icy-MetaData", "0")
            .send()
            .context("Failed to connect to stream")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", response.status()));
        }

        // Create streaming reader
        let stream_reader = StreamReader::new(response);

        // Create decoder
        let decoder = Decoder::new(stream_reader).context("Failed to create audio decoder")?;

        // Get sink and append
        {
            let sink_guard = sink_arc.lock().unwrap();
            if let Some(ref sink) = *sink_guard {
                sink.append(decoder);
                sink.play();
            } else {
                return Err(anyhow::anyhow!("Sink not available"));
            }
        }

        // Keep thread alive while playing
        while playing.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));

            let sink_guard = sink_arc.lock().unwrap();
            let is_empty = if let Some(ref sink) = *sink_guard {
                sink.len() == 0 && sink.is_paused()
            } else {
                true
            };

            if is_empty {
                break;
            }
        }

        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        self.playing.store(false, Ordering::SeqCst);

        {
            let mut sink = self.sink.lock().unwrap();
            if let Some(sink) = sink.take() {
                sink.stop();
            }
        }

        Ok(())
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::SeqCst)
    }
}

// Fixed streaming reader that properly handles continuous streaming
struct StreamReader {
    response: reqwest::blocking::Response,
    buffer: Vec<u8>,
    position: usize,
}

impl StreamReader {
    fn new(response: reqwest::blocking::Response) -> Self {
        StreamReader {
            response,
            buffer: Vec::with_capacity(32768),
            position: 0,
        }
    }
}

impl Read for StreamReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // If we have data in buffer, return it first
        if self.position < self.buffer.len() {
            let available = self.buffer.len() - self.position;
            let to_read = std::cmp::min(buf.len(), available);
            buf[..to_read].copy_from_slice(&self.buffer[self.position..self.position + to_read]);
            self.position += to_read;

            // If we've read all buffered data, clear it to save memory
            if self.position >= self.buffer.len() {
                self.buffer.clear();
                self.position = 0;
            }

            return Ok(to_read);
        }

        // Buffer is empty, read directly from response
        let mut chunk = vec![0u8; buf.len()];
        match self.response.read(&mut chunk) {
            Ok(0) => Ok(0), // EOF
            Ok(n) => {
                buf[..n].copy_from_slice(&chunk[..n]);
                Ok(n)
            }
            Err(e) => Err(e),
        }
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, _pos: SeekFrom) -> std::io::Result<u64> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Cannot seek in streaming audio",
        ))
    }
}

pub fn print_install_instructions() {
    println!(
        r#"
No audio output available. Please check your audio system.

On Windows:
  - Make sure you have audio drivers installed
  - Check that your default audio device is working

On Linux:
  - Install ALSA or PulseAudio
  - Ubuntu/Debian: sudo apt-get install alsa-utils
  - Fedora: sudo dnf install alsa-utils

On macOS:
  - CoreAudio should be available by default
"#
    );
}
