//! WaOS port: the sound output of a WaOS program, WaOS's audio graph.
//!
//! A stream is a ring of interleaved `f32` samples the playback thread fills, as cpal's is, and a
//! writer thread of its own that drains it into a `stream-input` node of an audio context, which
//! plays what is written ahead of the graph and silence while nothing waits. The node's `write`
//! waits while a latency's worth is ahead of what plays, so the writer paces itself, and the ring
//! fills up behind it, which the playback thread sees as a full device. While the stream is
//! paused its context is suspended, so a paused program makes no sound and WaOS may freeze it.
//!
//! The invariant: only the writer thread touches the context and its node, from its start to
//! its end; the playback thread talks to it through `Shared` and the ring.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use rtrb::{Consumer, Producer, RingBuffer};
use tracing::{error, warn};
use wasdk::audio::{self, AudioContext, AudioContextOptions};

use crate::{
    devices::{
        errors::{
            CloseError, FindError, InfoError, InitializationError, ListError, OpenError,
            ResetError, StateError, SubmissionError,
        },
        format::{BufferSize, ChannelSpec, FormatInfo, SampleFormat, SupportedFormat},
        traits::{Device, DeviceProvider, OutputStream},
    },
    media::pipeline::ChannelConsumers,
};

const PROVIDER: &str = "waos";
const CHANNELS: u16 = 2;
/// How many seconds of sound the ring holds ahead of the writer.
const RING_SECONDS: f64 = 0.25;
/// The most frames the writer hands the node at once.
const WRITE_FRAMES: usize = 1024;
/// How long the writer waits for the ring to fill when it found it empty, or while paused.
const IDLE_WAIT: Duration = Duration::from_millis(5);

/// WaOS's one output: the system's default, or the one the user picked for the program.
#[derive(Default)]
pub struct WaosProvider;

impl DeviceProvider for WaosProvider {
    fn initialize(&mut self) -> Result<(), InitializationError> {
        match audio::get_audio() {
            Some(_) => Ok(()),
            None => Err(InitializationError::Unknown(
                "WaOS gives this program no sound".to_owned(),
            )),
        }
    }

    fn get_devices(&mut self) -> Result<Vec<Box<dyn Device>>, ListError> {
        Ok(vec![Box::new(
            WaosDevice::new().map_err(ListError::Unknown)?,
        )])
    }

    fn get_default_device(&mut self) -> Result<Box<dyn Device>, FindError> {
        Ok(Box::new(WaosDevice::new().map_err(FindError::Unknown)?))
    }

    fn get_device_by_uid(&mut self, id: &str) -> Result<Box<dyn Device>, FindError> {
        if id == PROVIDER {
            self.get_default_device()
        } else {
            Err(FindError::DeviceDoesNotExist)
        }
    }
}

/// The output at the rate WaOS plays at, learnt from a context made for it.
struct WaosDevice {
    sample_rate: u32,
}

impl WaosDevice {
    fn new() -> Result<Self, String> {
        let context = new_context()?;
        let sample_rate = context.base().sample_rate() as u32;
        wasdk::block_on(context.close());
        Ok(WaosDevice { sample_rate })
    }

    fn format(&self) -> FormatInfo {
        FormatInfo {
            originating_provider: PROVIDER,
            sample_type: SampleFormat::Float32,
            sample_rate: self.sample_rate,
            buffer_size: BufferSize::Unknown,
            channels: ChannelSpec::Count(CHANNELS),
        }
    }
}

impl Device for WaosDevice {
    fn open_device(&mut self, format: FormatInfo) -> Result<Box<dyn OutputStream>, OpenError> {
        if format.originating_provider != PROVIDER {
            return Err(OpenError::InvalidConfigProvider);
        }
        if format.sample_type != SampleFormat::Float32 {
            return Err(OpenError::InvalidSampleFormat);
        }
        Ok(Box::new(WaosStream::open(self.sample_rate)?))
    }

    fn get_supported_formats(&self) -> Result<Vec<SupportedFormat>, InfoError> {
        Ok(vec![SupportedFormat {
            originating_provider: PROVIDER,
            sample_type: SampleFormat::Float32,
            sample_rates: (self.sample_rate, self.sample_rate),
            buffer_size: BufferSize::Unknown,
            channels: ChannelSpec::Count(CHANNELS),
        }])
    }

    fn get_default_format(&self) -> Result<FormatInfo, InfoError> {
        Ok(self.format())
    }

    fn get_name(&self) -> Result<String, InfoError> {
        Ok("WaOS".to_owned())
    }

    fn get_uid(&self) -> Result<String, InfoError> {
        Ok(PROVIDER.to_owned())
    }

    fn requires_matching_format(&self) -> bool {
        true
    }
}

/// What the playback thread tells the writer, and the writer tells it back.
#[derive(Default)]
struct Shared {
    /// The volume times ReplayGain, as `f64` bits.
    gain: AtomicU64,
    paused: AtomicBool,
    /// Set by the playback thread to end the writer, which then closes its context.
    closing: AtomicBool,
    /// Set by the writer when the context refused a write.
    failed: AtomicBool,
}

impl Shared {
    fn gain(&self) -> f32 {
        f64::from_bits(self.gain.load(Ordering::Relaxed)) as f32
    }
}

struct WaosStream {
    ring: Producer<f32>,
    ring_samples: usize,
    sample_rate: u32,
    shared: Arc<Shared>,
    writer: Option<JoinHandle<()>>,
    volume: f64,
    replaygain: f64,
    interleaved: Vec<f32>,
}

impl WaosStream {
    fn open(sample_rate: u32) -> Result<Self, OpenError> {
        let ring_samples = (RING_SECONDS * f64::from(sample_rate)) as usize * usize::from(CHANNELS);
        let (ring, drained) = RingBuffer::new(ring_samples);
        let shared = Arc::new(Shared {
            gain: AtomicU64::new(1.0f64.to_bits()),
            paused: AtomicBool::new(true),
            ..Shared::default()
        });
        let writer = start_writer(drained, Arc::clone(&shared))?;
        Ok(WaosStream {
            ring,
            ring_samples,
            sample_rate,
            shared,
            writer: Some(writer),
            volume: 1.0,
            replaygain: 1.0,
            interleaved: Vec::with_capacity(WRITE_FRAMES * usize::from(CHANNELS)),
        })
    }

    fn set_gain(&self) {
        let gain = self.volume * self.replaygain;
        self.shared.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    fn end_writer(&mut self) {
        self.shared.closing.store(true, Ordering::Relaxed);
        if let Some(writer) = self.writer.take()
            && writer.join().is_err()
        {
            error!("the WaOS sound writer panicked");
        }
    }
}

impl Drop for WaosStream {
    fn drop(&mut self) {
        self.end_writer();
    }
}

impl OutputStream for WaosStream {
    fn close_stream(&mut self) -> Result<(), CloseError> {
        self.end_writer();
        Ok(())
    }

    fn needs_input(&self) -> bool {
        self.ring.slots() >= usize::from(CHANNELS)
    }

    fn queued_frames(&self) -> usize {
        (self.ring_samples - self.ring.slots()) / usize::from(CHANNELS)
    }

    fn idle_pause_delay(&self) -> Duration {
        Duration::from_secs_f64(2.0 * RING_SECONDS)
    }

    fn play(&mut self) -> Result<(), StateError> {
        self.shared.paused.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn pause(&mut self) -> Result<(), StateError> {
        self.shared.paused.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> Result<(), ResetError> {
        let paused = self.shared.paused.load(Ordering::Relaxed);
        self.end_writer();
        let mut fresh = WaosStream::open(self.sample_rate)
            .map_err(|error| ResetError::Unknown(error.to_string()))?;
        fresh.volume = self.volume;
        fresh.replaygain = self.replaygain;
        fresh.set_gain();
        fresh.shared.paused.store(paused, Ordering::Relaxed);
        *self = fresh;
        Ok(())
    }

    fn set_volume(&mut self, volume: f64) -> Result<(), StateError> {
        self.volume = volume;
        self.set_gain();
        Ok(())
    }

    fn set_replaygain(&mut self, gain: f64) -> Result<(), StateError> {
        self.replaygain = gain;
        self.set_gain();
        Ok(())
    }

    fn consume_from(
        &mut self,
        input: &mut ChannelConsumers<f64>,
    ) -> Result<usize, SubmissionError> {
        if self.shared.failed.load(Ordering::Relaxed) {
            return Err(SubmissionError::DeviceError);
        }
        let channels = usize::from(CHANNELS);
        let writable_frames = self.ring.slots() / channels;
        let available = input
            .potentially_available()
            .min(WRITE_FRAMES)
            .min(writable_frames);
        if available == 0 {
            return Ok(0);
        }
        let read = input.try_read_to_staging(available);
        if read == 0 {
            return Ok(0);
        }
        interleave_stereo(input.staging(), read, &mut self.interleaved);
        let chunk = self
            .ring
            .write_chunk_uninit(self.interleaved.len())
            .expect("checked the ring's room before reading the input");
        chunk.fill_from_iter(self.interleaved.iter().copied());
        Ok(read)
    }
}

/// `read` frames of `staging`'s channels as interleaved stereo: a mono source on both sides, and
/// only the first two of more channels.
fn interleave_stereo(staging: &[Vec<f64>], read: usize, interleaved: &mut Vec<f32>) {
    interleaved.clear();
    let left = &staging[0];
    let right = staging.get(1).unwrap_or(left);
    for frame in 0..read {
        interleaved.push(left[frame] as f32);
        interleaved.push(right[frame] as f32);
    }
}

fn new_context() -> Result<AudioContext, String> {
    let audio = audio::get_audio().ok_or("WaOS gives this program no sound")?;
    audio
        .new_context(AudioContextOptions::new())
        .map_err(|error| format!("WaOS refused a sound context: {error:?}"))
}

fn start_writer(drained: Consumer<f32>, shared: Arc<Shared>) -> Result<JoinHandle<()>, OpenError> {
    thread::Builder::new()
        .name("waos-sound-writer".to_owned())
        .spawn(move || write_until_closed(drained, &shared))
        .map_err(|error| OpenError::Unknown(format!("no thread for the sound writer: {error}")))
}

/// The writer thread: makes the context and its stream input, then moves what the ring holds
/// into it, block by block, until the stream closes.
fn write_until_closed(mut drained: Consumer<f32>, shared: &Shared) {
    let context = match new_context() {
        Ok(context) => context,
        Err(why) => {
            error!("{why}");
            shared.failed.store(true, Ordering::Relaxed);
            return;
        }
    };
    let input = context.create_stream_input(u32::from(CHANNELS));
    let base = context.base();
    input.node().connect(&base.destination().node());
    let mut block = Vec::with_capacity(WRITE_FRAMES * usize::from(CHANNELS));
    // A context plays from when it is made; the stream opens paused.
    let mut suspended = false;
    while !shared.closing.load(Ordering::Relaxed) {
        let paused = shared.paused.load(Ordering::Relaxed);
        if paused != suspended {
            follow_pause(&context, paused);
            suspended = paused;
        }
        let samples =
            drained.slots().min(block.capacity()) / usize::from(CHANNELS) * usize::from(CHANNELS);
        if paused || samples == 0 {
            thread::sleep(IDLE_WAIT);
            continue;
        }
        let gain = shared.gain();
        let chunk = drained
            .read_chunk(samples)
            .expect("read no more than the ring holds");
        block.clear();
        block.extend(chunk.into_iter().map(|sample| sample * gain));
        if let Err(error) = wasdk::block_on(input.write(block.clone())) {
            warn!("WaOS refused the sound written: {error:?}");
            shared.failed.store(true, Ordering::Relaxed);
            break;
        }
    }
    wasdk::block_on(context.close());
}

/// Suspends the context while the stream is paused, so its output stops and WaOS sees the
/// program make no sound (and may freeze it out of sight), and resumes it to play again.
fn follow_pause(context: &AudioContext, paused: bool) {
    let changed = if paused {
        wasdk::block_on(context.suspend())
    } else {
        wasdk::block_on(context.resume())
    };
    if let Err(error) = changed {
        warn!(
            "WaOS's sound did not {}: {error:?}",
            if paused { "suspend" } else { "resume" }
        );
    }
}
