//! AV1 decoding through rav1d's dav1d API, one temporal unit (one IVF frame) at a time.

use std::mem::MaybeUninit;
use std::ptr::NonNull;

use anyhow::bail;
use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture, dav1d_open, dav1d_picture_unref, dav1d_send_data};

use crate::Frame;

const EAGAIN: i32 = -libc::EAGAIN;

pub struct Decoder {
    ctx: Option<Dav1dContext>,
}

impl Decoder {
    pub fn new() -> anyhow::Result<Decoder> {
        let mut s = MaybeUninit::<Dav1dSettings>::uninit();
        // SAFETY: dav1d_default_settings writes every field.
        let mut s = unsafe {
            dav1d_default_settings(NonNull::new_unchecked(s.as_mut_ptr()));
            s.assume_init()
        };
        // One picture out per temporal unit sent (no frame threading delay).
        s.n_threads = 2;
        s.max_frame_delay = 1;
        let mut ctx = None;
        // SAFETY: both pointers are valid for the call.
        let r = unsafe { dav1d_open(Some(NonNull::from(&mut ctx)), Some(NonNull::from(&mut s))) };
        if r.0 < 0 || ctx.is_none() {
            bail!("dav1d_open: {}", r.0);
        }
        Ok(Decoder { ctx })
    }

    /// Decode one temporal unit: the picture it shows, if any.
    pub fn decode(&mut self, tu: &[u8], time: f64) -> anyhow::Result<Option<Frame>> {
        let mut data = Dav1dData::default();
        // SAFETY: `data` is valid; the returned buffer has `tu.len()` bytes.
        let buf = unsafe { dav1d_data_create(Some(NonNull::from(&mut data)), tu.len()) };
        if buf.is_null() {
            bail!("dav1d_data_create failed");
        }
        // SAFETY: `buf` holds `tu.len()` bytes and does not overlap `tu`.
        unsafe { std::ptr::copy_nonoverlapping(tu.as_ptr(), buf, tu.len()) };
        let mut out = None;
        for _ in 0..64 {
            // SAFETY: the context is open; `data` is valid.
            let r = unsafe { dav1d_send_data(self.ctx, Some(NonNull::from(&mut data))) };
            if r.0 < 0 && r.0 != EAGAIN {
                // SAFETY: `data` came from dav1d_data_create.
                unsafe { dav1d_data_unref(Some(NonNull::from(&mut data))) };
                bail!("dav1d_send_data: {}", r.0);
            }
            if let Some(f) = self.picture(time)? {
                out = Some(f);
            }
            if data.sz == 0 {
                break;
            }
        }
        if data.sz != 0 {
            // SAFETY: as above.
            unsafe { dav1d_data_unref(Some(NonNull::from(&mut data))) };
            bail!("the decoder took no data");
        }
        Ok(out)
    }

    fn picture(&mut self, time: f64) -> anyhow::Result<Option<Frame>> {
        let mut p = Dav1dPicture::default();
        // SAFETY: the context is open; `p` is valid to write.
        let r = unsafe { dav1d_get_picture(self.ctx, Some(NonNull::from(&mut p))) };
        if r.0 == EAGAIN {
            return Ok(None);
        }
        if r.0 < 0 {
            bail!("dav1d_get_picture: {}", r.0);
        }
        let frame = copy_planes(&p, time);
        // SAFETY: `p` came from dav1d_get_picture.
        unsafe { dav1d_picture_unref(Some(NonNull::from(&mut p))) };
        frame.map(Some)
    }
}

/// The picture's planes, tightly packed (8-bit 4:2:0 only: what the converter writes).
fn copy_planes(p: &Dav1dPicture, time: f64) -> anyhow::Result<Frame> {
    let (w, h) = (p.p.w as usize, p.p.h as usize);
    if p.p.bpc != 8 {
        bail!("{} bits per component", p.p.bpc);
    }
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let plane = |i: usize, pw: usize, ph: usize, stride: isize| -> anyhow::Result<Vec<u8>> {
        let Some(base) = p.data[i] else { bail!("plane {i} missing") };
        let mut out = Vec::with_capacity(pw * ph);
        for y in 0..ph {
            // SAFETY: dav1d's planes hold `ph` rows of `stride` bytes, each at least `pw` wide.
            let row = unsafe { std::slice::from_raw_parts((base.as_ptr() as *const u8).offset(y as isize * stride), pw) };
            out.extend_from_slice(row);
        }
        Ok(out)
    };
    Ok(Frame { time, width: w as u32, height: h as u32, y: plane(0, w, h, p.stride[0])?, u: plane(1, cw, ch, p.stride[1])?, v: plane(2, cw, ch, p.stride[1])? })
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: the context came from dav1d_open and is closed once.
        unsafe { dav1d_close(Some(NonNull::from(&mut self.ctx))) };
    }
}
