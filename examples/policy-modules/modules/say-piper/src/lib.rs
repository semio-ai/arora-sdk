//! The say leaf spoken on board: a worker thread synthesizes with Piper, and
//! the behavior tree's ticks play what it produces on the host's audio
//! output.
//!
//! The contract is Vizij's speech skill's `say` (the function and parameter
//! ids of vizij-rs `vizij-arora-host::skills`), under Vizij's Piper module id:
//! `say(text, voice, viseme: &mut String) -> Status`. No network and no
//! credentials: the build provisions libpiper, espeak-ng's data and a voice
//! (see `build.rs`). The voice is chosen at build or run time (`PIPER_VOICE`),
//! so the `voice` argument is ignored.
//!
//! # Synthesis on a worker, playback in the tick
//!
//! Synthesis is long — a model inference per sentence, tens of milliseconds
//! — and a tick must not wait for it: the robot's policies share the tick.
//! So the **worker** thread does it, and only it: it owns the voice, and for
//! each request phonemizes a text or synthesizes its next sentence, sending
//! back the sentence's samples and mouth-shape timeline. The worker and the
//! ticks share no data; they talk through two channels.
//!
//! Playing is the **tick's**: it owns the audio output and each sentence's
//! queue on it. Each tick takes the sentences the worker has produced and
//! queues them, asks for the next one when less than [`LOOKAHEAD`] of audio
//! is queued ahead of the playhead — so speech starts with the first
//! sentence, not the whole text — reads the playhead to write the mouth
//! shape there into `viseme`, and returns `Success` once everything is
//! synthesized and played. The tick never waits on the worker.
//!
//! A finished `say` keeps returning `Success` while its branch keeps ticking
//! it — an episodic leaf's convention — so it pairs with a policy in a
//! `Parallel`, and speaks again when the branch is entered anew. A halt is
//! silence: a leaf no longer ticked asks for no further sentence, and the
//! next `say` tick stops its audio once it has gone [`IDLE_STOP`] unticked;
//! a tree that ticks no `say` at all lets the queued audio — under a
//! sentence and a [`LOOKAHEAD`] — play out. The module ABI hands a call no
//! run id, so sentences are keyed by their text; a new text also forgets the
//! finished ones, so a phase re-entered sooner than the halt bound still
//! speaks. Piper synthesizes one text at a time: a new text takes the voice
//! over, and one still synthesizing plays what it has.
//!
//! The tick's side lives on the thread the runtime ticks on (the audio
//! output is bound to the thread that opened it): [`warm_up`] is called from
//! that thread.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arora_behavior::Status;
use rodio::Sink;

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

/// How much audio is kept queued ahead of the playhead: below it, the tick
/// asks for the next sentence.
pub const LOOKAHEAD: Duration = Duration::from_millis(300);

/// How long a leaf goes unticked before its audio is stopped.
pub const IDLE_STOP: Duration = Duration::from_millis(250);

#[arora_module::module(
    id = "31ca2243-5719-4862-aa57-c30d27cab62e",
    name = "piper-say",
    version = "0.1.0",
    author = "Semio",
    license = "GPL-3.0-or-later",
    description = "Speech synthesized on board by Piper and played on the host's audio output"
)]
pub mod piper {
    use super::*;

    /// Speak `text`: `Running` while it is synthesized and played, `Success`
    /// when the audio has ended (and for as long as the leaf keeps being
    /// ticked afterwards), `Failure` when the voice, the synthesis or the
    /// audio output fails. `viseme` holds the mouth shape at the playhead,
    /// one of the face standard's (`PP`, `aa`, …, [`SILENCE_VISEME`]). An empty
    /// text says nothing and succeeds. `voice` is ignored: Piper's voice is
    /// the build's (`PIPER_VOICE` at run time). Never waits for synthesis.
    #[export(id = "77bf2798-e7ce-47c6-a45c-3c2e9ba1837d")]
    pub fn say(
        #[param(id = "881dc182-d4ba-4ea0-9e81-f4eddab6f669")] text: String,
        #[param(id = "f56ca142-db46-4c58-bc44-7896c4b54d5c")] voice: String,
        #[param(id = "a1fbf58b-bf66-44a6-a503-9d9078ee5755")] viseme: &mut String,
    ) -> Status {
        let _ = voice;
        let started = Instant::now();
        let (status, shape) = PLAYER.with_borrow_mut(|player| player.tick(&text));
        let spent = started.elapsed();
        if spent > Duration::from_millis(2) {
            log::debug!("say: a tick took {spent:.1?}");
        }
        *viseme = shape.to_string();
        status
    }
}

/// Load the voice and run a first inference on the worker, and open the
/// audio output, waiting for both: they are the slow parts, and a device
/// pays them before it runs, not in a tick. Call it on the thread the
/// runtime ticks on.
pub fn warm_up() -> Result<(), String> {
    PLAYER.with_borrow_mut(|player| {
        let (reply, answer) = mpsc::channel();
        player.send(Request::WarmUp(reply));
        player.output()?;
        answer
            .recv_timeout(Duration::from_secs(60))
            .map_err(|_| "the speech worker did not answer".to_string())?
    })
}

/// Stop the worker and wait for it: it frees the voice (`piper_free`) on its
/// way out, which libpiper needs before the process exits.
pub fn shut_down() {
    PLAYER.with_borrow_mut(|player| {
        player.send(Request::ShutDown);
        if let Some(worker) = player.worker.take() {
            let _ = worker.join();
        }
    });
}

// ─── The messages ────────────────────────────────────────────────────────────

/// What the tick asks of the worker.
enum Request {
    /// Load the voice and run a first inference; answer when done.
    WarmUp(Sender<Result<(), String>>),
    /// Phonemize `text` as utterance `id` and synthesize its first sentence.
    Start { id: u64, text: String },
    /// Synthesize utterance `id`'s next sentence.
    Next { id: u64 },
    /// Free the voice and end.
    ShutDown,
}

/// What the worker sends back.
enum Produced {
    /// A sentence of utterance `id`; `last` when the text is done.
    Sentence {
        id: u64,
        sentence: Synthesized,
        last: bool,
    },
    /// Utterance `id` could not be synthesized.
    Failed { id: u64 },
}

// ─── The tick's side: playback ───────────────────────────────────────────────

thread_local! {
    static PLAYER: RefCell<Player> = RefCell::new(Player::start());
}

/// The tick's side: the channels to the worker, the audio output, and each
/// sentence's playback.
struct Player {
    requests: Sender<Request>,
    produced: Receiver<Produced>,
    worker: Option<JoinHandle<()>>,
    /// The output stream and its handle; the stream must stay alive for the
    /// queues to play.
    output: Option<(rodio::OutputStream, rodio::OutputStreamHandle)>,
    utterances: HashMap<String, Utterance>,
    next_id: u64,
}

/// One text being said, as its leaf plays it.
struct Utterance {
    id: u64,
    state: State,
    last_tick: Instant,
    /// Its queue on the audio output: one source per sentence.
    sink: Option<Sink>,
    /// Where each queued sentence starts, in seconds of the utterance.
    starts: Vec<f64>,
    /// Seconds of audio queued so far.
    queued: f64,
    /// A sentence is asked for and not yet received.
    pending: bool,
    /// The worker has sent the last sentence.
    synthesized: bool,
    /// The phoneme timeline as mouth shapes, in seconds of the utterance.
    cues: Vec<(f64, &'static str)>,
    next_cue: usize,
    shape: &'static str,
}

enum State {
    Speaking,
    /// Over; latched until the leaf stops being ticked.
    Over(Status),
}

impl Utterance {
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

    fn fail(&mut self) {
        if let Some(sink) = &self.sink {
            sink.stop();
        }
        self.state = State::Over(Status::Failure);
    }
}

impl Player {
    fn start() -> Self {
        let (requests, worker_requests) = mpsc::channel();
        let (worker_produced, produced) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("say-piper".into())
            .spawn(move || synthesize(worker_requests, worker_produced))
            .expect("the speech worker thread starts");
        Self {
            requests,
            produced,
            worker: Some(worker),
            output: None,
            utterances: HashMap::new(),
            next_id: 0,
        }
    }

    /// Send a request, restarting the worker if it was shut down.
    fn send(&mut self, request: Request) {
        if self.worker.is_none() {
            let fresh = Self::start();
            self.requests = fresh.requests;
            self.produced = fresh.produced;
            self.worker = fresh.worker;
        }
        let _ = self.requests.send(request);
    }

    fn output(&mut self) -> Result<&rodio::OutputStreamHandle, String> {
        if self.output.is_none() {
            self.output =
                Some(rodio::OutputStream::try_default().map_err(|e| format!("audio output: {e}"))?);
        }
        Ok(&self.output.as_ref().expect("just opened").1)
    }

    /// One tick of `say(text)`: never waits on the worker.
    fn tick(&mut self, text: &str) -> (Status, &'static str) {
        self.receive();
        // A leaf nobody ticks any more was halted: its audio stops.
        self.utterances.retain(|_, u| {
            let wanted = u.last_tick.elapsed() <= IDLE_STOP;
            if !wanted {
                if let Some(sink) = &u.sink {
                    sink.stop();
                }
            }
            wanted
        });
        if text.is_empty() {
            return (Status::Success, SILENCE_VISEME);
        }
        if !self.utterances.contains_key(text) {
            self.begin(text);
        }

        let mut request = None;
        let u = self.utterances.get_mut(text).expect("present");
        u.last_tick = Instant::now();
        if let State::Over(status) = u.state {
            return (status, SILENCE_VISEME);
        }
        let playhead = u.playhead();
        if u.synthesized {
            if u.sink.as_ref().is_none_or(Sink::empty) {
                u.state = State::Over(Status::Success);
                return (Status::Success, SILENCE_VISEME);
            }
        } else if !u.pending && u.queued - playhead < LOOKAHEAD.as_secs_f64() {
            u.pending = true;
            request = Some(Request::Next { id: u.id });
        }
        while u.next_cue < u.cues.len() && u.cues[u.next_cue].0 <= playhead {
            u.shape = u.cues[u.next_cue].1;
            u.next_cue += 1;
        }
        let shape = u.shape;
        if let Some(request) = request {
            self.send(request);
        }
        (Status::Running, shape)
    }

    /// A new text: finished utterances are forgotten, so a phase entered
    /// again speaks again, and the worker is asked for its first sentence.
    fn begin(&mut self, text: &str) {
        self.utterances
            .retain(|_, u| matches!(u.state, State::Speaking));
        let id = self.next_id;
        self.next_id += 1;
        let mut utterance = Utterance {
            id,
            state: State::Speaking,
            last_tick: Instant::now(),
            sink: None,
            starts: Vec::new(),
            queued: 0.0,
            pending: true,
            synthesized: false,
            cues: Vec::new(),
            next_cue: 0,
            shape: SILENCE_VISEME,
        };
        match self
            .output()
            .and_then(|handle| Sink::try_new(handle).map_err(|e| format!("audio sink: {e}")))
        {
            Ok(sink) => {
                utterance.sink = Some(sink);
                self.send(Request::Start {
                    id,
                    text: text.to_string(),
                });
            }
            Err(why) => {
                log::error!("say (piper): {why}");
                utterance.fail();
            }
        }
        self.utterances.insert(text.to_string(), utterance);
    }

    /// Queue what the worker has produced.
    fn receive(&mut self) {
        while let Ok(produced) = self.produced.try_recv() {
            let id = match &produced {
                Produced::Sentence { id, .. } | Produced::Failed { id } => *id,
            };
            let Some(u) = self.utterances.values_mut().find(|u| u.id == id) else {
                continue; // halted meanwhile
            };
            u.pending = false;
            match produced {
                Produced::Failed { .. } => u.fail(),
                Produced::Sentence { sentence, last, .. } => {
                    if !sentence.samples.is_empty() {
                        let rate = f64::from(sentence.sample_rate);
                        u.cues.extend(
                            sentence
                                .cues
                                .iter()
                                .map(|(samples, shape)| (*samples as f64 / rate, *shape)),
                        );
                        u.starts.push(u.queued);
                        u.queued += sentence.samples.len() as f64 / rate;
                        if let Some(sink) = &u.sink {
                            sink.append(rodio::buffer::SamplesBuffer::new(
                                1,
                                sentence.sample_rate,
                                sentence.samples,
                            ));
                        }
                    }
                    u.synthesized |= last;
                }
            }
        }
    }
}

// ─── The worker: synthesis ───────────────────────────────────────────────────

/// The worker thread: owns the voice, answers each request in turn. One text
/// is phonemized at a time: a `Start` replaces the text in progress, and a
/// `Next` for any other utterance is answered as done.
fn synthesize(requests: Receiver<Request>, produced: Sender<Produced>) {
    let mut voice: Option<Voice> = None;
    // The utterance being synthesized, its sample cursor and how much of
    // libpiper's cumulative phoneme buffer is decoded.
    let mut current: Option<(u64, i64, usize)> = None;
    let ensure = |voice: &mut Option<Voice>| -> Result<(), String> {
        if voice.is_none() {
            *voice = Some(Voice::load()?);
        }
        Ok(())
    };
    // The tick side is gone only when the process ends.
    let send = |message: Produced| {
        let _ = produced.send(message);
    };
    for request in requests {
        match request {
            Request::WarmUp(reply) => {
                let ready = ensure(&mut voice).and_then(|()| {
                    // The first inference is several times slower than the
                    // next ones; pay it here.
                    let voice = voice.as_mut().expect("loaded");
                    voice.start("Ready.")?;
                    let (mut cursor, mut seen) = (0, 0);
                    while !voice.next_sentence(&mut cursor, &mut seen)?.1 {}
                    Ok(())
                });
                current = None;
                let _ = reply.send(ready);
            }
            Request::Start { id, text } => {
                current = None;
                let started =
                    ensure(&mut voice).and_then(|()| voice.as_mut().expect("loaded").start(&text));
                match started {
                    Ok(()) => {
                        current = Some((id, 0, 0));
                        next(&mut voice, &mut current, id, &send);
                    }
                    Err(why) => {
                        log::error!("say (piper): {why}");
                        send(Produced::Failed { id });
                    }
                }
            }
            Request::Next { id } => next(&mut voice, &mut current, id, &send),
            Request::ShutDown => break,
        }
    }
    // `voice` drops here, on the worker: piper_free.
}

/// Synthesize utterance `id`'s next sentence and send it; an utterance the
/// voice is no longer on is done.
fn next(
    voice: &mut Option<Voice>,
    current: &mut Option<(u64, i64, usize)>,
    id: u64,
    send: &impl Fn(Produced),
) {
    let (Some(voice), Some((current_id, cursor, seen))) = (voice.as_mut(), current.as_mut()) else {
        return send(Produced::Sentence {
            id,
            sentence: Synthesized::default(),
            last: true,
        });
    };
    if *current_id != id {
        return send(Produced::Sentence {
            id,
            sentence: Synthesized::default(),
            last: true,
        });
    }
    match voice.next_sentence(cursor, seen) {
        Ok((sentence, last)) => {
            if last {
                *current = None;
            }
            send(Produced::Sentence { id, sentence, last });
        }
        Err(why) => {
            log::error!("say (piper): {why}");
            *current = None;
            send(Produced::Failed { id });
        }
    }
}

// ─── Piper ───────────────────────────────────────────────────────────────────

/// A loaded Piper voice. Created, used and freed on the worker thread.
struct Voice {
    raw: *mut ffi::piper_synthesizer,
}

/// One synthesized sentence: mono PCM and its mouth shapes, each at a sample
/// offset from the utterance's start.
#[derive(Default)]
struct Synthesized {
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
    ) -> Result<(Synthesized, bool), String> {
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
            Synthesized {
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
