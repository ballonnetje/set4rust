//! Sound effect bank `Snd\0.snd` + index `Snd\0.sil`, the sound-id table, and `jobSoundList.xml`.
//!
//! ## Bank (`CSoundManager::OpenSoundFiles` @0x149bfb0, decompiled debug build)
//! - `0.sil`: 20-byte header (`u32 0x8003`, 3 × `u32 0`, `u32 0x24` — not read by the game), then
//!   `n = (size - 20) / 4` × `u32` offsets into `0.snd`. Offset 0 = unmapped slot. The game
//!   compares `n` against `LUT[108].first + LUT[108].count` (= 301) and only traces a mismatch.
//! - `0.snd`: 20-byte header (`u32 0x8002`, 3 × `u32 0`, `u32 0x24`), then at each offset:
//!   `u32 type` (must be 1, else "has unknown Type") + `u32 size` + `size` bytes, which are a
//!   complete RIFF/WAVE file handed to `ISoundEngine::PlaySoundFromMemory(.., ".wav", size, ..)`
//!   (`CSoundManager::PlaySoundFX` @0x149c4d0). The game rounds offsets down to 4 (`4*(off>>2)`).
//! - Observed in the shipped file: 301 slots, 2 unmapped (slot 0 = `SOUND_NO_SOUND`; slot 120 =
//!   the only variant of `SOUND_WORK_HEAL`, so that id is silent in the game), all 299 WAVs are IMA ADPCM
//!   (`WAVE_FORMAT_DVI_ADPCM` = 0x11, 4 bit) with `fmt `/`fact`/`data` chunks:
//!   195 × 22050 Hz mono (block 512), 55 × 44100 Hz mono (block 1024),
//!   48 × 44100 Hz stereo (block 2048), 1 × 8000 Hz mono (block 256).
//!   Decoded here with the standard IMA ADPCM algorithm; plain PCM (tag 1) is also accepted.
//!
//! ## Sound ids (`enum SIV_SOUNDS`, 109 values, `SOUND_MAX` = 109)
//! `PlaySoundFX(id)` picks slot `LUT[id].first + rand() % LUT[id].count` — so each id is a group of
//! random variants. The LUT (`g_sSoundLUT`, 12-byte entries `{first, count, 0}`) and the id names
//! (`CS4DefineNames::GetSoundName`, `{u32 id, char* name}` pairs) were read from the shipped
//! release `S4_Main.exe` at file offsets 0xc18580 and 0xc18218 (VAs 0x1019180 / 0x1018e18); they
//! are reproduced in [`SOUNDS`]. Per-id volume comes from `Config\SoundFX.cfg` `[SOUND_VOLUMES]`
//! (default 100); final volume = `master * id_vol * call_vol * fx_vol / 1e6`, clamped to 127
//! (`CSoundManager::CalcFinalVolume` @0x149b310).
//!
//! ## Who plays what
//! - Settler work animations: `GameData\jobSoundList.xml`, parsed by `CEntityToDoListMgr`
//!   (job-list loader, generated `CEntityToDoListMgr.cpp:705`) into `SJobSoundInfo
//!   { i32 sound; u8 random_percent; u8 frame }` per job id. Only the *first* `<sound>` and its first
//!   `<frame>` / `<random>` are read; `random` missing/0 means 100. In `ISettlerRole` (animation
//!   step, generated `ISettlerRole.cpp:295`) the sound is played via `PlayEnvironmentSound` (panned
//!   by map position) when the animation frame equals `frame`, `rand()%100 <= random`, and the
//!   tile is not fogged. See [`parse_job_sound_list`].
//! - Everything else calls `PlaySoundFX`/`PlayEnvironmentSound` with a literal `SIV_SOUNDS` id
//!   (effects/magic in `CEffect::InitEffect`, GUI clicks in `PlayGuiSound`, ...).
//!
//! ## Music (not in this bank: plain MP3s in `Snd\`)
//! `CSoundManager::LoadRaceTitles` @0x149b370 reads `Config\SoundTracks.cfg` sections
//! `<RACE>SOUNDTRACKS_<STYLE>` (keys `<RACE>0..n` = file names) and `..._FADETIME` (ms), with
//! RACE ∈ {ROMAN, VIKING, MAYA, DARK, TROJAN, MENU} (race index 0..5) and STYLE (`SIV_MUSICSTYLES`,
//! release exe string table at file offset 0xc18210) ∈ {0 SETTLE, 1 FIGHT, 2 SEA, 3 DARK,
//! 4 STARTMENU, 5 CREDITS}. In-game the local player's race playlist is loaded and
//! `CLogic::HandleSoundtracks` @0x146a770 switches style: FIGHT for 32 ticks after a fight
//! trigger, DARK while the screen centre is on dark land (map flag bit 4), otherwise SETTLE.
//! SEA is defined in the cfg but no caller selecting it was found. Main menu / end statistics use
//! MENU STARTMENU, credits MENU CREDITS. `Snd\User\*.mp3` replaces everything when "user MP3" is on.

use crate::reader::Reader;
use crate::{Result, bail};
use std::path::Path;

/// One `SIV_SOUNDS` id: name and its slot range in the bank.
#[derive(Clone, Copy, Debug)]
pub struct SoundId {
    pub name: &'static str,
    pub first: u16,
    pub count: u16,
}

impl SoundId {
    pub fn slots(&self) -> std::ops::Range<usize> {
        self.first as usize..(self.first + self.count) as usize
    }
}

macro_rules! sounds {
    ($($name:ident $first:literal $count:literal),* $(,)?) => {
        [$(SoundId { name: stringify!($name), first: $first, count: $count }),*]
    };
}

/// `g_sSoundLUT` + `GetSoundName` from the release exe, indexed by `SIV_SOUNDS`.
pub static SOUNDS: [SoundId; 109] = sounds![
    SOUND_NO_SOUND 0 1, SOUND_AMB_BIRD 1 33, SOUND_AMB_CRUSHBIGHOUSE 34 1,
    SOUND_AMB_CRUSHSMALLHOUSE 35 1, SOUND_BUILD_DIG 36 8, SOUND_BUILD_HAMMER 44 10,
    SOUND_GUI_BUILDBUTTONDOWN 54 1, SOUND_GUI_BUILDBUTTONUP 55 1, SOUND_GUI_MAINBUTTONDOWN 56 1,
    SOUND_GUI_MAINBUTTONUP 57 1, SOUND_GUI_SUBBUTTONDOWN 58 1, SOUND_GUI_SUBBUTTONUP 59 1,
    SOUND_WORK_AXE 60 7, SOUND_WORK_PICKAXE 67 2, SOUND_WORK_PLANT 69 1,
    SOUND_WORK_FARMERSEED 70 5, SOUND_WORK_FARMERHARVEST 75 3, SOUND_WORK_GETWATER 78 6,
    SOUND_FIGHT_SWORD 84 12, SOUND_FIGHT_BOW 96 10, SOUND_WORK_MILLDRIVE 106 1,
    SOUND_WORK_MELT_DOOR 107 1, SOUND_WORK_MELT 108 7, SOUND_WORK_SAWMILL 115 5,
    SOUND_WORK_HEAL 120 1, SOUND_ANIMAL_SEAGUL 121 1, SOUND_ANIMAL_BEAR 122 2,
    SOUND_ANIMAL_BUG 124 4, SOUND_ANIMAL_FISH 128 5, SOUND_ANIMAL_DOLPHIN 133 2,
    SOUND_ANIMAL_DUG 135 3, SOUND_ANIMAL_DONKEY 138 2, SOUND_ANIMAL_JAGUAR 140 4,
    SOUND_ANIMAL_SHEEP 144 3, SOUND_ANIMAL_PIG 147 3, SOUND_AMB_OCEAN 150 8,
    SOUND_AMB_RIVER 158 1, SOUND_WORK_GEOLOGIST 159 3, SOUND_WORK_GEOLOGIST_FOUND 162 3,
    SOUND_WORK_SMITH_HAMMER 165 2, SOUND_WORK_SMITH_COOL 167 4, SOUND_FIGHT_DIE 171 25,
    SOUND_WORK_LOOKOUTTOWER 196 2, SOUND_WORK_FISHER_RODOUT 198 1, SOUND_WORK_FISHER_RODIN 199 1,
    SOUND_WORK_FISHER_CATCH 200 3, SOUND_IDLE_SNIFF 203 8, SOUND_AMB_RAIN 211 3,
    SOUND_AMB_FIRE 214 3, SOUND_FIGHT_FIREBALL 217 2, SOUND_WORK_MINER 219 2,
    SOUND_AMB_THUNDER 221 5, SOUND_MISC_BELL 226 2, SOUND_AMB_BEES 228 1,
    SOUND_MISC_HEAL 229 5, SOUND_MAGIC_MAGIC01 234 1, SOUND_MAGIC_MAGIC02 235 1,
    SOUND_MAGIC_MAGIC03 236 1, SOUND_MAGIC_MAGIC04 237 1, SOUND_MAGIC_MAGIC05 238 1,
    SOUND_MAGIC_FRIGHTENSOLDIERS 239 1, SOUND_AMB_WIND 240 1, SOUND_MAGIC_THORSHAMMER 241 1,
    SOUND_MAGIC_TELEPORT 242 1, SOUND_MAGIC_SMELTSNOW 243 1, SOUND_MAGIC_GIFTOFGOD 244 1,
    SOUND_MAGIC_FERTILITY 245 1, SOUND_MAGIC_CHANGEGOOD 246 1, SOUND_MAGIC_BARBARIAN 247 1,
    SOUND_ANIMAL_BURN 248 1, SOUND_MAGIC_BLOODRUSH 249 1, SOUND_MAGIC_STONECURSE 250 1,
    SOUND_MAGIC_DEFENSESHIELD 251 1, SOUND_MAGIC_INCREASEANIMALS 252 1, SOUND_MAGIC_FOODFISH 253 1,
    SOUND_MAGIC_JUPITERS_LIGTHNING 254 1, SOUND_GUI_BELL01 255 1, SOUND_GUI_BELL02 256 1,
    SOUND_GUI_HEAL01 257 1, SOUND_GUI_HEAL02 258 1, SOUND_AMB_DESERT 259 1,
    SOUND_MAGIC_RAISERANK 260 1, SOUND_MAGIC_FREEZESOLDIERS 261 1,
    SOUND_MAGIC_REFRESHRESOURCES 262 1, SOUND_MAGIC_PUNISHSOLDIERS 263 1,
    SOUND_AMB_IMPACTTARGET 264 1, SOUND_AMB_IMPACTWATER 265 1, SOUND_IDLE_NEWSPAPER 266 4,
    SOUND_CRUSH_SMALLBUILDING 270 1, SOUND_CRUSH_BIGBUILDING 271 1,
    SOUND_AMB_ATTACKBIGBUILDING 272 1, SOUND_AMB_ATTACKSMALLBUILDING 273 1,
    SOUND_AMB_WATERHITATARGET 274 1, SOUND_MAGIC_SLAVE 275 1, SOUND_GUI_CLICK01 276 1,
    SOUND_GUI_CLICK02 277 1, SOUND_WORK_TREEFALL 278 6, SOUND_MAGIC_DARKDRY 284 1,
    SOUND_MAGIC_MAGICDOOR 285 1, SOUND_MAGIC_MANAKOPTERTURN 286 1,
    SOUND_MAGIC_MANAKOPTERROTOR 287 1, SOUND_EFFECTS_MANAKOPTEREXPLODE 288 1,
    SOUND_FIGHT_BACKPACKCATAPULTIST 289 3, SOUND_FIGHT_WARMACHINEBALLIST 292 2,
    SOUND_EFFECTS_EXPLOSIONAMMOMAKER 294 1, SOUND_AMB_HORN 295 1, SOUND_AMB_SNORE 296 3,
    SOUND_WORK_SUNFLOWER 299 1, SOUND_AMB_MAGICSTONES 300 1,
];

/// Look up a `SIV_SOUNDS` id by its define name (e.g. `"SOUND_WORK_AXE"`).
pub fn sound_id_by_name(name: &str) -> Option<usize> {
    SOUNDS.iter().position(|s| s.name == name)
}

/// Decoded sample: interleaved signed 16-bit PCM.
#[derive(Clone, Debug)]
pub struct Sample {
    pub rate: u32,
    pub channels: u16,
    pub pcm: Vec<i16>,
}

impl Sample {
    pub fn frames(&self) -> usize {
        self.pcm.len() / self.channels.max(1) as usize
    }
    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }
    /// Serialise as a canonical 16-bit PCM WAV file.
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = (self.pcm.len() * 2) as u32;
        let block = self.channels as u32 * 2;
        let mut w = Vec::with_capacity(44 + data_len as usize);
        w.extend(b"RIFF");
        w.extend((36 + data_len).to_le_bytes());
        w.extend(b"WAVEfmt ");
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes());
        w.extend(self.channels.to_le_bytes());
        w.extend(self.rate.to_le_bytes());
        w.extend((self.rate * block).to_le_bytes());
        w.extend((block as u16).to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend(data_len.to_le_bytes());
        for s in &self.pcm {
            w.extend(s.to_le_bytes());
        }
        w
    }
}

/// `0.snd` + `0.sil`, held in memory (≈11 MB).
pub struct SoundBank {
    snd: Vec<u8>,
    offsets: Vec<u32>,
}

impl SoundBank {
    /// `dir` is the game root (containing `Snd\`) or the `Snd` directory itself.
    pub fn open(dir: impl AsRef<Path>) -> Result<SoundBank> {
        let dir = dir.as_ref();
        let snd_dir = if dir.join("0.sil").is_file() {
            dir.to_path_buf()
        } else {
            crate::path::resolve(dir.join("Snd"))
        };
        Self::parse(
            std::fs::read(crate::path::resolve(snd_dir.join("0.sil")))?,
            std::fs::read(crate::path::resolve(snd_dir.join("0.snd")))?,
        )
    }

    pub fn parse(sil: Vec<u8>, snd: Vec<u8>) -> Result<SoundBank> {
        if sil.len() < 20 {
            bail!("0.sil: {} bytes, too short", sil.len());
        }
        let mut r = Reader::at(&sil, 20, "0.sil");
        let n = (sil.len() - 20) / 4;
        let offsets = (0..n).map(|_| r.u32()).collect::<Result<Vec<_>>>()?;
        Ok(SoundBank { snd, offsets })
    }

    /// Number of slots (301 in the shipped bank).
    pub fn len(&self) -> usize {
        self.offsets.len()
    }
    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// The embedded WAV file of slot `i`; `Ok(None)` for an unmapped slot (offset 0).
    pub fn raw_wav(&self, i: usize) -> Result<Option<&[u8]>> {
        let Some(&off) = self.offsets.get(i) else {
            bail!("sound slot {i} out of range")
        };
        if off == 0 {
            return Ok(None);
        }
        let mut r = Reader::at(&self.snd, (off & !3) as usize, "0.snd");
        let ty = r.u32()?;
        let size = r.u32()? as usize;
        if ty != 1 {
            bail!("sound slot {i}: unknown type {ty}");
        }
        Ok(Some(r.bytes(size)?))
    }

    /// Decode slot `i` to PCM. Errors for unmapped slots; use [`Self::raw_wav`] to test mapping.
    pub fn sample(&self, i: usize) -> Result<Sample> {
        match self.raw_wav(i)? {
            Some(wav) => decode_wav(wav),
            None => bail!("sound slot {i} is unmapped"),
        }
    }
}

/// WAV `fmt ` chunk fields we use.
#[derive(Clone, Copy, Debug)]
pub struct WavFormat {
    pub tag: u16,
    pub channels: u16,
    pub rate: u32,
    pub block_align: u16,
    pub bits: u16,
}

/// Parse a RIFF/WAVE file and decode PCM16 or IMA ADPCM (tag 0x11) to interleaved i16.
pub fn decode_wav(wav: &[u8]) -> Result<Sample> {
    let mut r = Reader::new(wav, "wav");
    let riff = r.bytes(4)?;
    r.skip(4)?;
    if riff != b"RIFF" || r.bytes(4)? != b"WAVE" {
        bail!("wav: not a RIFF/WAVE file");
    }
    let mut fmt = None;
    let mut fact = None;
    let mut data = None;
    while r.remaining() >= 8 {
        let id = r.bytes(4)?;
        let len = r.u32()? as usize;
        let body = r.bytes(len.min(r.remaining()))?;
        if len & 1 == 1 && r.remaining() > 0 {
            r.skip(1)?;
        }
        let mut b = Reader::new(body, "wav chunk");
        match id {
            b"fmt " => {
                let tag = b.u16()?;
                let channels = b.u16()?;
                let rate = b.u32()?;
                b.skip(4)?;
                let block_align = b.u16()?;
                let bits = b.u16()?;
                fmt = Some(WavFormat {
                    tag,
                    channels,
                    rate,
                    block_align,
                    bits,
                });
            }
            b"fact" => fact = Some(b.u32()? as usize),
            b"data" => data = Some(body),
            _ => {}
        }
    }
    let (Some(f), Some(data)) = (fmt, data) else {
        bail!("wav: missing fmt or data chunk")
    };
    if f.channels == 0 || f.channels > 2 {
        bail!("wav: {} channels unsupported", f.channels);
    }
    let pcm = match (f.tag, f.bits) {
        (1, 16) => data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c))
            .collect(),
        (0x11, 4) => {
            let mut p = decode_ima_adpcm(data, f.channels as usize, f.block_align as usize)?;
            if let Some(frames) = fact {
                p.truncate(frames * f.channels as usize);
            }
            p
        }
        (t, b) => bail!("wav: unsupported format tag {t:#x} / {b} bits"),
    };
    Ok(Sample {
        rate: f.rate,
        channels: f.channels,
        pcm,
    })
}

const IMA_INDEX: [i8; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];
const IMA_STEP: [i16; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

struct ImaState {
    pred: i32,
    index: i32,
}

impl ImaState {
    fn step(&mut self, nib: u8) -> i16 {
        let step = IMA_STEP[self.index as usize] as i32;
        let mut diff = step >> 3;
        if nib & 1 != 0 {
            diff += step >> 2;
        }
        if nib & 2 != 0 {
            diff += step >> 1;
        }
        if nib & 4 != 0 {
            diff += step;
        }
        if nib & 8 != 0 {
            diff = -diff;
        }
        self.pred = (self.pred + diff).clamp(-32768, 32767);
        self.index = (self.index + IMA_INDEX[nib as usize] as i32).clamp(0, 88);
        self.pred as i16
    }
}

/// Microsoft/DVI IMA ADPCM: per block, per channel a 4-byte header `{i16 sample, u8 index, u8 0}`
/// (that sample is the first output), then groups of 4 bytes per channel (8 nibbles, low first).
fn decode_ima_adpcm(data: &[u8], ch: usize, block_align: usize) -> Result<Vec<i16>> {
    if block_align <= 4 * ch || !(block_align - 4 * ch).is_multiple_of(4 * ch) {
        bail!("ima adpcm: bad block align {block_align} for {ch} channels");
    }
    let mut out = Vec::with_capacity(data.len() * 2);
    for block in data.chunks(block_align) {
        if block.len() < 4 * ch {
            break;
        }
        let mut st: Vec<ImaState> = (0..ch)
            .map(|c| {
                let h = &block[4 * c..4 * c + 4];
                ImaState {
                    pred: i16::from_le_bytes([h[0], h[1]]) as i32,
                    index: (h[2] as i32).min(88),
                }
            })
            .collect();
        for s in &st {
            out.push(s.pred as i16);
        }
        let body = &block[4 * ch..];
        let groups = body.len() / (4 * ch);
        let mut frame = vec![0i16; 8 * ch];
        for g in 0..groups {
            for (c, s) in st.iter_mut().enumerate() {
                let bytes = &body[(g * ch + c) * 4..(g * ch + c) * 4 + 4];
                for (k, &b) in bytes.iter().enumerate() {
                    frame[(2 * k) * ch + c] = s.step(b & 0x0F);
                    frame[(2 * k + 1) * ch + c] = s.step(b >> 4);
                }
            }
            out.extend_from_slice(&frame);
        }
    }
    Ok(out)
}

/// One `<job>` of `jobSoundList.xml`, as the game reads it (first `<sound>` only).
#[derive(Clone, Debug)]
pub struct JobSound {
    /// Job define name, e.g. `"WC_CUT_TREE"` (resolved to `SIV_JOBS` by the define table).
    pub job: String,
    /// Sound define name, e.g. `"SOUND_WORK_AXE"`.
    pub sound: String,
    /// `SIV_SOUNDS` index of `sound`; `None` if the name is unknown or the job has no `<sound>`
    /// (the game stores 0 = `SOUND_NO_SOUND` then, i.e. silent).
    pub sound_id: Option<usize>,
    /// Animation frame on which the sound starts.
    pub frame: u8,
    /// Play probability in percent (missing or 0 -> 100).
    pub random: u8,
}

/// Minimal reader for the fixed shape of `GameData\jobSoundList.xml`:
/// `<job id=".."><sound id=".."><frame>N</frame>[<random>N</random>]</sound>...</job>`.
pub fn parse_job_sound_list(xml: &str) -> Result<Vec<JobSound>> {
    let xml = strip_comments(xml);
    let mut out = Vec::new();
    let mut rest = xml.as_str();
    while let Some(p) = rest.find("<job ") {
        rest = &rest[p + 5..];
        let end = rest.find("</job>").unwrap_or(rest.len());
        let body = &rest[..end];
        let job = attr(body, "id").unwrap_or_default();
        // A job without <sound> exists (BA_WORK_DOUGH); the game then resolves "" -> -1 -> 0 (none).
        let sbody = body.find("<sound").map_or("", |p| &body[p + 6..]);
        let sound = attr(sbody, "id").unwrap_or_default();
        let sbody = &sbody[..sbody.find("</sound>").unwrap_or(sbody.len())];
        let num = |tag: &str| -> u8 {
            elem(sbody, tag)
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0)
        };
        let random = match num("random") {
            0 => 100,
            r => r,
        };
        out.push(JobSound {
            sound_id: sound_id_by_name(&sound),
            job,
            sound,
            frame: num("frame"),
            random,
        });
        rest = &rest[end..];
    }
    Ok(out)
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find("<!--") {
        out.push_str(&rest[..p]);
        rest = rest[p..].find("-->").map_or("", |e| &rest[p + e + 3..]);
    }
    out.push_str(rest);
    out
}

/// Value of `name="..."` in the text up to the first `>`.
fn attr(s: &str, name: &str) -> Option<String> {
    let tag = &s[..s.find('>')?];
    let p = tag.find(&format!("{name}="))? + name.len() + 1;
    let q = tag[p..].chars().next()?;
    let v = &tag[p + 1..];
    Some(v[..v.find(q)?].to_string())
}

fn elem<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let p = s.find(&open)? + open.len();
    let e = s[p..].find(&format!("</{tag}>"))?;
    Some(&s[p..p + e])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lut_is_contiguous() {
        for w in SOUNDS.windows(2) {
            assert_eq!(w[0].first + w[0].count, w[1].first);
        }
        assert_eq!(SOUNDS[108].slots().end, 301);
    }

    #[test]
    fn ima_block() {
        // header: sample 100, index 0; one group: all-zero nibbles -> each step adds 7>>3 = 0
        let mut b = vec![100, 0, 0, 0];
        b.extend([0x00; 4]);
        let p = decode_ima_adpcm(&b, 1, 8).unwrap();
        assert_eq!(p.len(), 9);
        assert!(p.iter().all(|&s| s == 100));
        // nibble 7 at index 0: diff = 0+1+3+7 = 11
        let b = vec![0, 0, 0, 0, 0x07, 0, 0, 0];
        assert_eq!(decode_ima_adpcm(&b, 1, 8).unwrap()[1], 11);
    }

    #[test]
    fn job_xml() {
        let x = r#"<!-- c --><jobsoundlist>
            <job id="A"><sound id="SOUND_WORK_AXE">
            <frame>6</frame></sound></job><job id="B"><sound id="X"><frame>2</frame>
            <random>30</random></sound></job></jobsoundlist>"#;
        let j = parse_job_sound_list(x).unwrap();
        assert_eq!(j.len(), 2);
        assert_eq!((j[0].frame, j[0].random, j[0].sound_id), (6, 100, Some(12)));
        assert_eq!(
            (j[1].job.as_str(), j[1].random, j[1].sound_id),
            ("B", 30, None)
        );
    }
}
