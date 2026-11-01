//! Offline audio check: renders a short scripted sequence through the real `Audio` service and
//! kira mixer into `out/audio/demo.wav`, then measures it.
//!
//! * 0–3 s, effects only: an axe at the left screen edge, a hammer at the right edge, a sawmill in
//!   the centre, and an axe off-screen left (which the original silences). Each sound's L/R RMS
//!   is measured.
//! * 3–12 s, music: Roman SETTLE title 0 starts at full music volume. At 5 s one war action
//!   arrives, then a logic tick every 71 ms; the director switches to FIGHT (a 2.5 s crossfade in
//!   71 ms steps). 32 ticks later it switches back to SETTLE (another crossfade).
//! * A voice-cap check: 40 simultaneous requests on a separate manager.
//!
//! Usage: `cargo run --release -p s4-audio --example audio_demo`

use s4_audio::offline::{offline_settings, wav_bytes, OfflineBackend, OfflineRenderer};
use s4_audio::{Audio, AudioEvent, MusicStyle, Played, Race, SoundId, StreamInfo, View};
use std::time::Duration;

const RATE: u32 = 44100;
const BLOCK: usize = 1024;

enum Act {
    Fx(&'static str, i32, i32, &'static str),
    Music,
    War,
}

fn rms(s: &[f32], ch: usize, from: f64, to: f64) -> f64 {
    let a = (from * RATE as f64) as usize;
    let b = ((to * RATE as f64) as usize).min(s.len() / 2);
    if b <= a {
        return 0.0;
    }
    let sum: f64 = (a..b).map(|i| (s[i * 2 + ch] as f64).powi(2)).sum();
    (sum / (b - a) as f64).sqrt()
}

fn db(x: f64) -> f64 {
    if x <= 0.0 { -f64::INFINITY } else { 20.0 * x.log10() }
}

fn new_audio() -> anyhow::Result<(Audio<OfflineBackend>, OfflineRenderer)> {
    let (settings, renderer) = offline_settings(RATE);
    let manager = kira::AudioManager::<OfflineBackend>::new(settings).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    Ok((Audio::new(manager, s4_formats::game_dir())?, renderer))
}

fn main() -> anyhow::Result<()> {
    let (mut audio, renderer) = new_audio()?;
    let t0 = std::time::Instant::now();
    let slots = audio.preload()?;
    println!("decoded {slots} bank slots in {:.0} ms", t0.elapsed().as_secs_f64() * 1000.0);

    // 1024x768 view centred on node (100,100), closest zoom (72 px triangles: zoom factor 100)
    audio.set_listener(View::centred_on(100.0, 100.0, 1024, 768, 72.0, 256));

    let mut script: Vec<(f64, Act)> = vec![
        (0.2, Act::Fx("SOUND_WORK_AXE", 90, 100, "axe, off-screen left")),
        (0.8, Act::Fx("SOUND_WORK_AXE", 93, 100, "axe, left edge")),
        (1.4, Act::Fx("SOUND_BUILD_HAMMER", 107, 100, "hammer, right edge")),
        (2.0, Act::Fx("SOUND_WORK_SAWMILL", 100, 100, "sawmill, centre")),
        (3.0, Act::Music),
        (5.0, Act::War),
    ];
    script.reverse();
    let total = 12.0;
    let mut out = Vec::with_capacity((total * RATE as f64) as usize * 2);
    let mut fx_windows = Vec::new();
    let mut fade_log: Vec<(f64, Vec<StreamInfo>, MusicStyle)> = Vec::new();
    let mut ticking = false;
    let mut next_tick = 0.0;
    let mut t = 0.0;
    while t < total {
        while script.last().is_some_and(|(at, _)| *at <= t) {
            let (at, act) = script.pop().unwrap();
            match act {
                Act::Fx(name, x, y, what) => {
                    let id = SoundId::by_name(name).unwrap();
                    let p = audio.play_fx(id, x, y)?;
                    println!("{at:5.2}s play_fx {name:22} at ({x},{y}) -> {p:?}  [{what}]");
                    // measure until the next sound (music starts at 3.0 s)
                    let end = script.last().map_or(at + 0.7, |(next, _)| *next).min(at + 0.7);
                    fx_windows.push((what, at, end, p));
                }
                Act::Music => {
                    audio.set_music_state(Race::Roman, MusicStyle::Settle)?;
                    println!("{at:5.2}s set_music_state(Roman, Settle): {} titles", audio.playlist(MusicStyle::Settle).titles.len());
                    ticking = true;
                    next_tick = at;
                }
                Act::War => {
                    audio.handle(&AudioEvent::WarAction)?;
                    println!("{at:5.2}s WarAction");
                }
            }
        }
        // logic ticks at 1/14 s
        while ticking && next_tick <= t {
            let before = audio.music_style();
            audio.handle(&AudioEvent::LogicTick { centre_dark: Some(false) })?;
            if audio.music_style() != before {
                println!("{next_tick:5.2}s style {before:?} -> {:?}", audio.music_style());
            }
            next_tick += 1.0 / 14.0;
        }
        let dt = BLOCK as f64 / RATE as f64;
        audio.update(Duration::from_secs_f64(dt))?;
        fade_log.push((t, audio.music_streams(), audio.music_style()));
        out.extend(renderer.render(BLOCK));
        // let kira's MP3 decoder threads keep ahead of the (faster than real time) mixer
        std::thread::sleep(Duration::from_millis(3));
        t += dt;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/audio");
    std::fs::create_dir_all(&dir)?;
    let wav = dir.join("demo.wav");
    std::fs::write(&wav, wav_bytes(RATE, &out))?;
    println!("\nwrote {} ({:.1} s, {} frames)", wav.display(), out.len() as f64 / 2.0 / RATE as f64, out.len() / 2);

    println!("\n-- effects: RMS per channel from onset to min(next event, sample end) --");
    for name in ["SOUND_AMB_BIRD", "SOUND_AMB_RIVER", "SOUND_MISC_HEAL", "SOUND_WORK_AXE"] {
        println!("SoundFX.cfg volume {name} = {}", audio.id_volume(SoundId::by_name(name).unwrap()));
    }
    // prediction: the sample's own RMS over the same span x the Miles mixer's channel gains
    let bank = s4_formats::snd::SoundBank::open(s4_formats::game_dir())?;
    for (what, a, b, p) in &fx_windows {
        // span = onset .. min(next event, end of the sample)
        let b = match p {
            Played::Voice { slot, .. } => b.min(a + bank.sample(*slot as usize)?.duration_secs()),
            _ => *b,
        };
        let (l, r) = (rms(&out, 0, *a, b), rms(&out, 1, *a, b));
        let pred = match p {
            Played::Voice { slot, volume, pan } => {
                let smp = bank.sample(*slot as usize)?;
                let n = (((b - a) * smp.rate as f64) as usize).min(smp.frames());
                let ch = smp.channels as usize;
                let srms = |c: usize| {
                    ((0..n).map(|i| (smp.pcm[i * ch + c.min(ch - 1)] as f64 / 32768.0).powi(2)).sum::<f64>() / n.max(1) as f64).sqrt()
                };
                // The Miles 6.1c mixer's channel gains (s4_audio::miles_gains).
                let (gl, gr) = s4_audio::miles_gains(*volume, *pan);
                format!("predicted L {:7.1} R {:7.1}", db(srms(0) * gl as f64), db(srms(1) * gr as f64))
            }
            _ => String::new(),
        };
        println!("{what:22} L {:7.1} dBFS  R {:7.1} dBFS   {pred}   {p:?}", db(l), db(r));
    }

    println!("\n-- music: stream volumes (Miles units) and output level, every ~0.25 s from 4.5 s --");
    let mut last = -1.0;
    for (tt, streams, style) in &fade_log {
        if *tt >= 4.5 && *tt - last >= 0.25 && *tt < total - 0.1 {
            last = *tt;
            let lvl = db(rms(&out, 0, *tt, *tt + 0.1).max(rms(&out, 1, *tt, *tt + 0.1)));
            let s: Vec<String> = streams.iter().map(|(id, l, v)| format!("#{id}:{}={v}", l.map_or("-".into(), |l| l.to_string()))).collect();
            println!("{tt:5.2}s {style:?}  {:<26} out {lvl:6.1} dBFS", s.join(" "));
        }
    }
    // starvation check: 50 ms windows of digital silence while music should be audible
    let silent: Vec<f64> = (0..((total - 3.2) / 0.05) as usize)
        .map(|i| 3.2 + i as f64 * 0.05)
        .filter(|&a| rms(&out, 0, a, a + 0.05) + rms(&out, 1, a, a + 0.05) < 1e-5)
        .collect();
    println!("\n50 ms windows of digital silence during music (3.2 s .. {total} s): {} {:?}", silent.len(), &silent[..silent.len().min(8)]);
    println!("stats: {:?}", audio.stats);

    // voice cap
    let (mut cap, _r) = new_audio()?;
    cap.set_listener(View::centred_on(100.0, 100.0, 1024, 768, 72.0, 256));
    let id = SoundId::by_name("SOUND_WORK_SAWMILL").unwrap();
    let res: Vec<Played> = (0..40).map(|_| cap.play_fx(id, 100, 100)).collect::<Result<_, _>>()?;
    let voiced = res.iter().filter(|p| matches!(p, Played::Voice { .. })).count();
    let dropped = res.iter().filter(|p| **p == Played::NoVoice).count();
    println!("voice cap: 40 requests at once -> {voiced} voices, {dropped} dropped (original: 28 sample handles)");

    // ambient loops: ocean+river square for 1.5 s, then ocean only, then nothing
    let (mut amb, ar) = new_audio()?;
    amb.set_listener(View::centred_on(100.0, 100.0, 1024, 768, 43.0, 256)); // zoom factor 50
    println!("\n-- ambient loops (CSoundManager::Update), logic tick 1/14 s --");
    let mut amb_out = Vec::new();
    let (mut t, mut next_tick, mut last) = (0.0f64, 0.0f64, -1.0f64);
    while t < 4.0 {
        while next_tick <= t {
            let flags = if next_tick < 1.5 { 0x02 | 0x04 } else if next_tick < 2.5 { 0x02 } else { 0 };
            amb.handle(&AudioEvent::Ambient { x: 100, y: 100, flags })?;
            next_tick += 1.0 / 14.0;
        }
        let dt = BLOCK as f64 / RATE as f64;
        amb.update(Duration::from_secs_f64(dt))?;
        if t - last >= 0.25 {
            last = t;
            let loops: Vec<String> = amb.ambient_loops().iter().map(|(id, v)| format!("{}={v}", id.name())).collect();
            println!("{t:5.2}s {}", if loops.is_empty() { "-".into() } else { loops.join(" ") });
        }
        amb_out.extend(ar.render(BLOCK));
        t += dt;
    }
    for (a, b) in [(0.0, 0.5), (0.6, 1.4), (1.6, 2.4), (2.6, 3.0), (3.2, 4.0)] {
        println!("output {a:.1}-{b:.1} s: {:6.1} dBFS", db(rms(&amb_out, 0, a, b)));
    }
    std::fs::write(dir.join("ambient.wav"), wav_bytes(RATE, &amb_out))?;
    Ok(())
}
