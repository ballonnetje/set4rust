//! Decode every slot of `Snd\0.snd`, print stats, write a few as WAV to `out/snd/`, and list the
//! job -> sound mapping from `GameData\jobSoundList.xml`.
//! Usage: `cargo run -p s4-formats --example snd_dump [all]`

use s4_formats::snd::{SOUNDS, SoundBank, parse_job_sound_list};
use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let game = s4_formats::game_dir();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/snd");
    std::fs::create_dir_all(&out)?;
    let write_all = std::env::args().any(|a| a == "all");

    let bank = SoundBank::open(&game)?;
    let mut formats: BTreeMap<(u32, u16), (usize, f64)> = BTreeMap::new();
    let (mut ok, mut unmapped) = (0, Vec::new());
    let mut peak_total = 0i32;
    for i in 0..bank.len() {
        if bank.raw_wav(i)?.is_none() {
            unmapped.push(i);
            continue;
        }
        let s = bank.sample(i)?;
        let e = formats.entry((s.rate, s.channels)).or_default();
        e.0 += 1;
        e.1 += s.duration_secs();
        peak_total = peak_total.max(s.pcm.iter().map(|&v| (v as i32).abs()).max().unwrap_or(0));
        ok += 1;
    }
    println!(
        "slots {} decoded {ok} unmapped {unmapped:?} max |sample| {peak_total}",
        bank.len()
    );
    for ((rate, ch), (n, secs)) in &formats {
        println!(
            "  {rate:5} Hz x{ch}: {n:3} samples, {secs:7.1} s total, avg {:.2} s",
            secs / *n as f64
        );
    }

    // Owner id of each slot, for naming.
    let owner = |slot: usize| {
        SOUNDS
            .iter()
            .find(|s| s.slots().contains(&slot))
            .map_or("?", |s| s.name)
    };
    let picks: Vec<usize> = if write_all {
        (0..bank.len()).collect()
    } else {
        vec![1, 36, 44, 60, 84, 96, 150, 171, 221, 241, 276, 288]
    };
    for &i in &picks {
        if bank.raw_wav(i)?.is_none() {
            continue;
        }
        let s = bank.sample(i)?;
        let path = out.join(format!("{i:03}_{}.wav", owner(i)));
        std::fs::write(&path, s.to_wav())?;
        println!(
            "{i:3} {:40} {:5} Hz x{} {:6} frames {:.2} s -> {}",
            owner(i),
            s.rate,
            s.channels,
            s.frames(),
            s.duration_secs(),
            path.display()
        );
    }

    let xml = std::fs::read_to_string(game.join("GameData/jobSoundList.xml"))?;
    let jobs = parse_job_sound_list(&xml)?;
    println!("\njobSoundList.xml: {} jobs", jobs.len());
    for j in &jobs {
        println!(
            "  {:24} -> {:28} (id {:?}) frame {} random {}%",
            j.job, j.sound, j.sound_id, j.frame, j.random
        );
    }
    Ok(())
}
