//! The say leaf spoken on board: Piper synthesizes, the host's audio output
//! plays, and the behavior tree's ticks drive both a step at a time.
//!
//! The contract is Vizij's speech skill's `say` (the function and parameter
//! ids of vizij-rs `vizij-arora-host::skills`), under Vizij's Piper module id:
//! `say(text, voice, viseme: &mut String) -> Status`. No network and no
//! credentials: the build provisions libpiper, espeak-ng's data and a voice
//! (see `build.rs`). The voice is chosen at build or run time (`PIPER_VOICE`),
//! so the `voice` argument is ignored.
//!
//! # An iterative leaf
//!
//! A `say` is advanced by the ticks that call it, never by a task of its own:
//!
//! 1. the first tick phonemizes the text (`piper_synthesize_start`);
//! 2. each tick where less than [`LOOKAHEAD`] of audio is queued ahead of the
//!    playhead synthesizes the next sentence (`piper_synthesize_next`, one
//!    model inference) and queues its samples on the utterance's sink;
//! 3. the audio output plays the queue on its own device thread, and each
//!    tick reads the playhead to write the mouth shape there into `viseme`;
//! 4. the tick that finds the queue empty and the text fully synthesized
//!    returns `Success`.
//!
//! So a tick does at most one sentence's inference, and speech starts as soon
//! as the first sentence is ready rather than when the whole text is. A
//! finished `say` keeps returning `Success` while its branch keeps ticking it
//! — an episodic leaf's convention — so it pairs with a policy in a
//! `Parallel`, and speaks again when the branch is entered anew.
//!
//! A halt is silence: a `say` the tree stops ticking for [`IDLE_STOP`] is
//! stopped by the audio thread, and forgotten at the next call. The module
//! ABI hands a call no run id, so utterances are keyed by their text; a new
//! text also forgets the finished ones, so a phase re-entered sooner than
//! the halt bound still speaks.
//!
//! Piper synthesizes one utterance at a time: a new text takes the
//! synthesizer over, and an utterance still synthesizing plays what it has.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::{Duration, Instant};

use arora_behavior::Status;
use rodio::{OutputStreamHandle, Sink};

#[allow(
    dead_code,
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    clippy::all
)]
mod ffi {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

/// The rest shape: the viseme written while nothing is spoken.
pub const SILENCE_VISEME: &str = "sil";

/// How much audio is kept queued ahead of the playhead: below it, the next
/// sentence is synthesized.
pub const LOOKAHEAD: Duration = Duration::from_millis(300);

/// How long a `say` goes unticked before it is halted and its audio stopped.
pub const IDLE_STOP: Duration = Duration::from_millis(250);

#[arora_module::module(
    id = "31ca2243-5719-4862-aa57-c30d27cab62e",
    name = "piper-say",
    version = "0.1.0",
    author = "Semio",
    license = "GPL-3.0-or-later",
    description = "Speech synthesized on board by Piper and played on the host's audio output, a step per tick"
)]
pub mod piper {
    use super::*;

    /// Speak `text`: `Running` while it is synthesized and played, `Success`
    /// when the audio has ended (and for as long as the leaf keeps being
    /// ticked afterwards), `Failure` when the voice, the synthesis or the
    /// audio output fails. `viseme` holds the mouth shape at the playhead,
    /// one of the face standard's (`PP`, `aa`, …, [`SILENCE_VISEME`]). An empty
    /// text says nothing and succeeds. `voice` is ignored: Piper's voice is
    /// the build's (`PIPER_VOICE` at run time).
    #[export(id = "77bf2798-e7ce-47c6-a45c-3c2e9ba1837d")]
    pub fn say(
        #[param(id = "881dc182-d4ba-4ea0-9e81-f4eddab6f669")] text: String,
        #[param(id = "f56ca142-db46-4c58-bc44-7896c4b54d5c")] voice: String,
        #[param(id = "a1fbf58b-bf66-44a6-a503-9d9078ee5755")] viseme: &mut String,
    ) -> Status {
        let _ = voice;
        let (status, shape) = super::tick(&text);
        *viseme = shape.to_string();
        status
    }
}

/// Load the voice and open the audio output now rather than on the first
/// `say`: the model load takes a noticeable fraction of a second, which a
/// tick should not pay.
pub fn warm_up() -> Result<(), String> {
    let mut provider = PROVIDER.lock().unwrap();
    provider.voice()?;
    provider.output()?;
    Ok(())
}

/// Drop the voice (`piper_free`) before the process exits: libpiper's C++
/// statics abort when torn down with a live synthesizer.
pub fn shut_down() {
    PROVIDER.lock().unwrap().voice = None;
}

// ─── The provider ────────────────────────────────────────────────────────────

struct Provider {
    voice: Option<Voice>,
    output: Option<Output>,
    runs: HashMap<String, Run>,
    /// The text whose sentences the synthesizer is producing.
    synthesizing: Option<String>,
}

static PROVIDER: LazyLock<Mutex<Provider>> = LazyLock::new(|| {
    Mutex::new(Provider {
        voice: None,
        output: None,
        runs: HashMap::new(),
        synthesizing: None,
    })
});

impl Provider {
    fn voice(&mut self) -> Result<&mut Voice, String> {
        if self.voice.is_none() {
            self.voice = Some(Voice::load()?);
        }
        Ok(self.voice.as_mut().expect("just loaded"))
    }

    fn output(&mut self) -> Result<&Output, String> {
        if self.output.is_none() {
            self.output = Some(Output::open()?);
        }
        Ok(self.output.as_ref().expect("just opened"))
    }
}

/// One utterance.
struct Run {
    stage: Stage,
    /// When the leaf was last ticked; the audio thread stops a quiet one.
    pulse: Arc<Mutex<Instant>>,
    /// The utterance's queue on the audio output: one source per sentence.
    sink: Option<Arc<Sink>>,
    /// Where each queued sentence starts, in seconds of the utterance.
    starts: Vec<f64>,
    /// Seconds of audio queued so far.
    queued: f64,
    /// The phoneme timeline as mouth shapes, in seconds of the utterance.
    cues: Vec<(f64, &'static str)>,
    next_cue: usize,
    shape: &'static str,
    /// Samples synthesized so far, and how much of libpiper's cumulative
    /// phoneme buffer has been decoded.
    cursor: i64,
    phonemes_seen: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    /// Sentences still to synthesize.
    Synthesizing,
    /// Everything queued; playing it out.
    Playing,
    /// Over: latched until the leaf stops being ticked.
    Done(Status),
}

impl Run {
    fn new() -> Self {
        Self {
            stage: Stage::Synthesizing,
            pulse: Arc::new(Mutex::new(Instant::now())),
            sink: None,
            starts: Vec::new(),
            queued: 0.0,
            cues: Vec::new(),
            next_cue: 0,
            shape: SILENCE_VISEME,
            cursor: 0,
            phonemes_seen: 0,
        }
    }

    fn halted(&self) -> bool {
        self.pulse.lock().unwrap().elapsed() > IDLE_STOP
    }

    /// Seconds into the utterance at the playhead: the start of the sentence
    /// playing plus the position within it.
    fn playhead(&self) -> f64 {
        let Some(sink) = &self.sink else {
            return 0.0;
        };
        if sink.empty() {
            return self.queued;
        }
        let playing = self.starts.len().saturating_sub(sink.len());
        self.starts.get(playing).copied().unwrap_or(self.queued) + sink.get_pos().as_secs_f64()
    }

    fn fail(&mut self, why: String) -> (Status, &'static str) {
        log::error!("say (piper): {why}");
        if let Some(sink) = &self.sink {
            sink.stop();
        }
        self.stage = Stage::Done(Status::Failure);
        (Status::Failure, SILENCE_VISEME)
    }
}

/// One tick of `say(text)`.
fn tick(text: &str) -> (Status, &'static str) {
    let mut guard = PROVIDER.lock().unwrap();
    let provider = &mut *guard;

    // Halted utterances go (the audio thread has silenced them).
    provider.runs.retain(|_, run| !run.halted());
    if let Some(synthesizing) = &provider.synthesizing {
        if !provider.runs.contains_key(synthesizing) {
            provider.synthesizing = None;
        }
    }
    if text.is_empty() {
        return (Status::Success, SILENCE_VISEME);
    }

    if !provider.runs.contains_key(text) {
        // A new text: finished utterances are forgotten, so a phase entered
        // again speaks again; this one takes the synthesizer.
        provider
            .runs
            .retain(|_, run| !matches!(run.stage, Stage::Done(_)));
        let mut run = Run::new();
        let started = provider
            .voice()
            .and_then(|voice| voice.start(text))
            .and_then(|()| {
                let output = provider.output()?;
                let sink = Arc::new(
                    Sink::try_new(&output.handle).map_err(|e| format!("audio sink: {e}"))?,
                );
                output.watch(&sink, &run.pulse);
                Ok(sink)
            });
        match started {
            Ok(sink) => {
                run.sink = Some(sink);
                provider.synthesizing = Some(text.to_string());
            }
            Err(why) => {
                run.fail(why);
            }
        }
        provider.runs.insert(text.to_string(), run);
    }

    let Provider {
        runs,
        voice,
        synthesizing,
        ..
    } = provider;
    let run = runs.get_mut(text).expect("present");
    *run.pulse.lock().unwrap() = Instant::now();

    match run.stage {
        Stage::Done(status) => return (status, SILENCE_VISEME),
        Stage::Synthesizing if synthesizing.as_deref() != Some(text) => {
            // Another text took the synthesizer: play what was made.
            run.stage = Stage::Playing;
        }
        Stage::Synthesizing => {
            let playhead = run.playhead();
            if run.queued - playhead < LOOKAHEAD.as_secs_f64() {
                let voice = voice.as_mut().expect("loaded to start");
                match voice.next_sentence(&mut run.cursor, &mut run.phonemes_seen) {
                    Ok((sentence, done)) => {
                        if !sentence.samples.is_empty() {
                            let rate = f64::from(sentence.sample_rate);
                            let start = run.queued;
                            run.cues.extend(
                                sentence
                                    .cues
                                    .iter()
                                    .map(|(samples, shape)| (*samples as f64 / rate, *shape)),
                            );
                            run.starts.push(start);
                            run.queued += sentence.samples.len() as f64 / rate;
                            let sink = run.sink.as_ref().expect("started with a sink");
                            sink.append(rodio::buffer::SamplesBuffer::new(
                                1,
                                sentence.sample_rate,
                                sentence.samples,
                            ));
                        }
                        if done {
                            run.stage = Stage::Playing;
                            *synthesizing = None;
                        }
                    }
                    Err(why) => {
                        *synthesizing = None;
                        return run.fail(why);
                    }
                }
            }
        }
        Stage::Playing => {}
    }

    if run.stage == Stage::Playing && run.sink.as_ref().is_none_or(|sink| sink.empty()) {
        run.stage = Stage::Done(Status::Success);
        return (Status::Success, SILENCE_VISEME);
    }
    let playhead = run.playhead();
    while run.next_cue < run.cues.len() && run.cues[run.next_cue].0 <= playhead {
        run.shape = run.cues[run.next_cue].1;
        run.next_cue += 1;
    }
    (Status::Running, run.shape)
}

// ─── The audio output ────────────────────────────────────────────────────────

/// The host's audio output, kept open on a thread of its own (a cpal stream
/// is bound to the thread that opened it). The same thread stops the sinks
/// of utterances the tree stopped ticking: nothing else runs between ticks.
struct Output {
    handle: OutputStreamHandle,
    watched: Arc<Mutex<Vec<Watched>>>,
}

type Watched = (Weak<Sink>, Arc<Mutex<Instant>>);

impl Output {
    fn open() -> Result<Self, String> {
        let watched: Arc<Mutex<Vec<Watched>>> = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        let list = watched.clone();
        std::thread::Builder::new()
            .name("say-audio".into())
            .spawn(move || match rodio::OutputStream::try_default() {
                Ok((_stream, handle)) => {
                    let _ = tx.send(Ok(handle));
                    loop {
                        std::thread::sleep(Duration::from_millis(20));
                        list.lock().unwrap().retain(|(sink, pulse)| {
                            let Some(sink) = sink.upgrade() else {
                                return false;
                            };
                            if pulse.lock().unwrap().elapsed() > IDLE_STOP {
                                sink.stop();
                                return false;
                            }
                            true
                        });
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(format!("audio output: {e}")));
                }
            })
            .map_err(|e| format!("audio thread: {e}"))?;
        let handle = rx.recv().map_err(|_| "audio thread ended".to_string())??;
        Ok(Self { handle, watched })
    }

    fn watch(&self, sink: &Arc<Sink>, pulse: &Arc<Mutex<Instant>>) {
        self.watched
            .lock()
            .unwrap()
            .push((Arc::downgrade(sink), pulse.clone()));
    }
}

// ─── Piper ───────────────────────────────────────────────────────────────────

/// A loaded Piper voice.
struct Voice {
    raw: *mut ffi::piper_synthesizer,
}

// SAFETY: libpiper has no thread affinity (onnxruntime and espeak-ng calls);
// it is not re-entrant, which the provider's lock guarantees.
unsafe impl Send for Voice {}

/// One synthesized sentence: mono PCM and its mouth shapes, each at a sample
/// offset from the utterance's start.
struct Sentence {
    samples: Vec<f32>,
    sample_rate: u32,
    cues: Vec<(i64, &'static str)>,
}

impl Voice {
    /// The build-provisioned voice and espeak-ng data, overridable with
    /// `PIPER_VOICE`, `PIPER_VOICE_CONFIG`, `PIPER_ESPEAK_DATA`.
    fn load() -> Result<Self, String> {
        let var = |name: &str, default: &str| std::env::var(name).unwrap_or(default.to_string());
        let model = var("PIPER_VOICE", env!("VIZIJ_PIPER_DEFAULT_VOICE"));
        let config = var(
            "PIPER_VOICE_CONFIG",
            env!("VIZIJ_PIPER_DEFAULT_VOICE_CONFIG"),
        );
        let espeak = var("PIPER_ESPEAK_DATA", env!("VIZIJ_PIPER_DEFAULT_ESPEAK_DATA"));
        let c = |s: &str| CString::new(s).map_err(|e| e.to_string());
        let (m, cf, e) = (c(&model)?, c(&config)?, c(&espeak)?);
        // SAFETY: valid NUL-terminated paths, alive for the call.
        let raw = unsafe { ffi::piper_create(m.as_ptr(), cf.as_ptr(), e.as_ptr()) };
        if raw.is_null() {
            return Err(format!("could not load the Piper voice {model}"));
        }
        Ok(Self { raw })
    }

    /// Phonemize `text` and queue its sentences for [`Voice::next_sentence`].
    fn start(&mut self, text: &str) -> Result<(), String> {
        let t = CString::new(text).map_err(|e| e.to_string())?;
        // SAFETY: a live synthesizer; libpiper copies the text.
        let rc = unsafe {
            let options = ffi::piper_default_synthesize_options(self.raw);
            ffi::piper_synthesize_start(self.raw, t.as_ptr(), &options)
        };
        if rc != ffi::PIPER_OK as i32 {
            return Err(format!("piper_synthesize_start failed ({rc})"));
        }
        Ok(())
    }

    /// Synthesize the next sentence — one model inference. `cursor` is the
    /// utterance's sample count so far and `phonemes_seen` how much of
    /// libpiper's cumulative phoneme buffer is decoded; both advance. Returns
    /// the sentence and whether it was the last.
    fn next_sentence(
        &mut self,
        cursor: &mut i64,
        phonemes_seen: &mut usize,
    ) -> Result<(Sentence, bool), String> {
        // SAFETY: a zeroed chunk is a valid out-parameter.
        let mut chunk: ffi::piper_audio_chunk = unsafe { std::mem::zeroed() };
        // SAFETY: a live synthesizer, started.
        let rc = unsafe { ffi::piper_synthesize_next(self.raw, &mut chunk) };
        let done = rc == ffi::PIPER_DONE as i32;
        if rc != ffi::PIPER_OK as i32 && !done {
            return Err(format!("piper_synthesize_next failed ({rc})"));
        }
        // The chunk's buffers live until the next call: copy them out. A
        // `PIPER_DONE` can still carry the last sentence.
        let samples: Vec<f32> = raw_slice(chunk.samples, chunk.num_samples).to_vec();
        let mut cues = Vec::new();
        if chunk.num_alignments > 0 {
            if chunk.num_alignments != chunk.num_phoneme_ids {
                return Err("alignments and phoneme ids are not parallel".into());
            }
            let all = raw_slice(chunk.phonemes, chunk.num_phonemes);
            let alignments = raw_slice(chunk.alignments, chunk.num_alignments);
            // The phoneme buffer is cumulative across sentences; decode only
            // what is new.
            let new = &all[(*phonemes_seen).min(all.len())..];
            *phonemes_seen = all.len();
            decode(new, alignments, cursor, &mut cues)?;
        } else {
            *cursor += samples.len() as i64;
        }
        Ok((
            Sentence {
                samples,
                sample_rate: chunk.sample_rate.max(1) as u32,
                cues,
            },
            done,
        ))
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        // SAFETY: created by piper_create, freed once.
        unsafe { ffi::piper_free(self.raw) }
    }
}

/// A libpiper chunk buffer as a slice; empty when null.
fn raw_slice<'a, T>(ptr: *const T, len: usize) -> &'a [T] {
    if ptr.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: libpiper's buffer of `len` elements, alive until the next
        // `piper_synthesize_next`; the caller copies out before then.
        unsafe { std::slice::from_raw_parts(ptr, len) }
    }
}

/// libpiper's grouping (piper.h): `phonemes` repeats each codepoint once per
/// corresponding id, groups separated by 0; `alignments` holds one sample
/// count per id, so a phoneme lasts the sum of its group. Each phoneme
/// becomes a cue at its start, as a mouth shape.
fn decode(
    phonemes: &[u32],
    alignments: &[std::os::raw::c_int],
    cursor: &mut i64,
    cues: &mut Vec<(i64, &'static str)>,
) -> Result<(), String> {
    let (mut i, mut a) = (0usize, 0usize);
    while i < phonemes.len() {
        if phonemes[i] == 0 {
            i += 1;
            continue;
        }
        let codepoint = phonemes[i];
        let mut n = 0;
        while i + n < phonemes.len() && phonemes[i + n] == codepoint {
            n += 1;
        }
        if a + n > alignments.len() {
            return Err("alignments shorter than the phoneme groups".into());
        }
        let duration: i64 = alignments[a..a + n].iter().map(|&x| i64::from(x)).sum();
        let phoneme = char::from_u32(codepoint)
            .map(String::from)
            .unwrap_or_default();
        cues.push((*cursor, phoneme_shape(&phoneme)));
        *cursor += duration;
        i += n;
        a += n;
    }
    // Ids past the last group still take time.
    *cursor += alignments[a.min(alignments.len())..]
        .iter()
        .map(|&x| i64::from(x))
        .sum::<i64>();
    Ok(())
}

/// The face-standard shape for an espeak-ng phoneme, by articulation;
/// markers, punctuation and unknown symbols are the rest shape. (Vizij's
/// Piper provider's table.)
fn phoneme_shape(phoneme: &str) -> &'static str {
    let phoneme = phoneme.trim_matches(|c: char| matches!(c, 'ˈ' | 'ˌ' | 'ː' | '.' | ' '));
    let mut chars = phoneme.chars();
    let (Some(first), second) = (chars.next(), chars.next()) else {
        return SILENCE_VISEME;
    };
    match (first, second) {
        ('t', Some('ʃ')) | ('d', Some('ʒ')) => "CH",
        ('p' | 'b' | 'm', _) => "PP",
        ('f' | 'v', _) => "FF",
        ('θ' | 'ð', _) => "TH",
        ('t' | 'd' | 'ɾ', _) => "DD",
        ('k' | 'g' | 'ɡ', _) => "kk",
        ('ʃ' | 'ʒ', _) => "CH",
        ('s' | 'z', _) => "SS",
        ('n' | 'ŋ' | 'l' | 'ɫ', _) => "nn",
        ('r' | 'ɹ' | 'ɻ', _) => "RR",
        ('a' | 'ɑ' | 'ʌ' | 'æ' | 'ɐ', _) => "aa",
        ('e' | 'ɛ' | 'ɜ', _) => "E",
        ('i' | 'ɪ' | 'ɨ' | 'j' | 'ə' | 'ɚ' | 'h', _) => "ih",
        ('o' | 'ɔ' | 'ɒ', _) => "oh",
        ('u' | 'ʊ' | 'ʉ' | 'w', _) => "ou",
        _ => SILENCE_VISEME,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phonemes_map_to_the_standard_shapes() {
        assert_eq!(phoneme_shape("tʃ"), "CH");
        assert_eq!(phoneme_shape("t"), "DD");
        assert_eq!(phoneme_shape("oʊ"), "oh");
        assert_eq!(phoneme_shape("ˈaɪ"), "aa");
        assert_eq!(phoneme_shape("^"), "sil");
    }

    /// The voice synthesizes sentence by sentence, each with its cues, and
    /// the cursor ends where the audio does.
    #[test]
    fn synthesizes_a_sentence_at_a_time() {
        let mut voice = Voice::load().expect("the provisioned voice");
        voice.start("Watch my kick! Up I get.").unwrap();
        let (mut cursor, mut seen, mut samples, mut sentences) = (0i64, 0usize, 0usize, 0);
        loop {
            let started = Instant::now();
            let (sentence, done) = voice.next_sentence(&mut cursor, &mut seen).unwrap();
            eprintln!(
                "sentence {sentences}: {} samples, {} cues, synthesized in {:.1?}",
                sentence.samples.len(),
                sentence.cues.len(),
                started.elapsed()
            );
            if !sentence.samples.is_empty() {
                sentences += 1;
                assert!(!sentence.cues.is_empty(), "the patched voice aligns");
            }
            samples += sentence.samples.len();
            if done {
                break;
            }
        }
        drop(voice);
        assert_eq!(sentences, 2);
        assert_eq!(cursor, samples as i64, "the alignments cover the audio");
    }
}
