//! An offline kira backend: the mixer runs only when [`OfflineRenderer::render`] is called, and
//! the output is returned as interleaved stereo `f32`. Tests and the WAV example use it to
//! check the mix without speakers.

use kira::backend::{Backend, Renderer};
use std::sync::{Arc, Mutex};

/// Where the backend parks kira's renderer; shared with [`OfflineRenderer`].
pub type RendererSlot = Arc<Mutex<Option<Renderer>>>;

#[derive(Clone)]
pub struct OfflineSettings {
    pub sample_rate: u32,
    pub slot: RendererSlot,
}

impl Default for OfflineSettings {
    fn default() -> Self {
        OfflineSettings { sample_rate: 44100, slot: Arc::default() }
    }
}

pub struct OfflineBackend {
    slot: RendererSlot,
}

impl Backend for OfflineBackend {
    type Settings = OfflineSettings;
    type Error = std::convert::Infallible;

    fn setup(settings: Self::Settings, _internal_buffer_size: usize) -> Result<(Self, u32), Self::Error> {
        Ok((OfflineBackend { slot: settings.slot }, settings.sample_rate))
    }

    fn start(&mut self, renderer: Renderer) -> Result<(), Self::Error> {
        *self.slot.lock().expect("renderer mutex") = Some(renderer);
        Ok(())
    }
}

/// Pulls audio out of an [`OfflineBackend`].
#[derive(Clone)]
pub struct OfflineRenderer {
    pub slot: RendererSlot,
    pub sample_rate: u32,
}

impl OfflineRenderer {
    /// Mix the next `frames` frames (one `on_start_processing` per call, so commands sent before
    /// the call take effect at its start). Returns interleaved L/R.
    pub fn render(&self, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        if let Some(r) = self.slot.lock().expect("renderer mutex").as_mut() {
            r.on_start_processing();
            r.process(&mut out, 2);
        }
        out
    }
}

/// `AudioManagerSettings` for an offline manager plus the renderer handle for it.
pub fn offline_settings(sample_rate: u32) -> (kira::AudioManagerSettings<OfflineBackend>, OfflineRenderer) {
    let slot = RendererSlot::default();
    let settings = kira::AudioManagerSettings {
        capacities: Default::default(),
        main_track_builder: Default::default(),
        internal_buffer_size: 128,
        backend_settings: OfflineSettings { sample_rate, slot: slot.clone() },
    };
    (settings, OfflineRenderer { slot, sample_rate })
}

/// 16-bit PCM stereo WAV of interleaved `f32` samples.
pub fn wav_bytes(sample_rate: u32, samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&sample_rate.to_le_bytes());
    w.extend_from_slice(&(sample_rate * 4).to_le_bytes());
    w.extend_from_slice(&4u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    w
}
