// Landscape triangles, built entirely in the vertex shader from two storage buffers (see
// terrain.rs). Formulas from docs/TERRAIN.md (s4_formats::gfx::landscape): node projection and
// height offset, piece -> UV, per-vertex diffuse/specular from the shading index, fog
// darkening, MODULATE + specular add. Colour arithmetic is on the raw 8-bit values, as the
// original (no gamma handling).

struct U {
    origin: vec2<f32>,   // world position of the view's top-left
    scale: vec2<f32>,    // world px -> NDC (zoom and y flip)
    half_px: vec2<f32>,  // +0.5 screen px in NDC: D3D7 puts pixel centres on integer coordinates
    fade: u32,           // fog fade step 0..15 (old -> new fog level)
    flags: u32,          // bit0: target is sRGB (encode raw values so the stored bytes equal them)
                         // bit1: lighting off (diffuse 255, specular 0)
                         // bit2: heightfield AO on (optional effect, not in the original)
    map_w: u32,          // map width in nodes
    border: i32,         // g_iFadeYBorder: vertices of rows above it use fade step 0
    _pad1: u32,
    _pad2: u32,
};

@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var pages: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var<storage, read> nodes: array<u32>;   // height | ground<<8 | gradient<<16 | gfx<<24
@group(0) @binding(4) var<storage, read> pieces: array<u32>;  // 2 per cell, see terrain::pack_piece
@group(0) @binding(5) var<storage, read> ao: array<u32>;      // optional AO, 4 nodes per word (effects.rs)

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) page: u32,
    @location(2) diffuse: vec3<f32>,   // 0..255
    @location(3) specular: vec3<f32>,  // 0..255
    // UV bounds (min.xy, max.xy) the sample stays inside: the pattern cell inset by half a texel,
    // unbounded for tiled pieces.
    @location(4) @interpolate(flat) bounds: vec4<f32>,
};

// byte_3ACD720 (release VA 0x106ebec); index 0 uses 0x472449 instead.
const DIFFUSE = array<f32, 16>(25., 51., 76., 102., 127., 153., 178., 204., 255., 255., 255., 255., 255., 255., 255., 255.);
// Low bytes of CHeightAndTypeTable[0..9] (fog ramp).
const FOG_RAMP = array<u32, 9>(0x49u, 0x00u, 0x0au, 0x16u, 0x23u, 0x3fu, 0x66u, 0xa5u, 0xffu);
// Pattern triangles in units of 1/16 inside a 0.25 cell, vertex order Up [A,B,C] / Down [A,D,C]
// (landscape::pattern_uv, from sub_2F7BC20 rel 0x66b390).
const PATTERN = array<vec2<f32>, 18>(
    vec2(2., 2.), vec2(1., 4.), vec2(3., 4.),
    vec2(0., 2.), vec2(2., 2.), vec2(1., 4.),
    vec2(1., 0.), vec2(0., 2.), vec2(2., 2.),
    vec2(1., 0.), vec2(3., 0.), vec2(2., 2.),
    vec2(3., 0.), vec2(2., 2.), vec2(4., 2.),
    vec2(2., 2.), vec2(4., 2.), vec2(3., 4.),
);
// Tiled triangles: offsets from A's planar UV (landscape::tiled_uv).
const TILED = array<vec2<f32>, 6>(
    vec2(0., 0.), vec2(-0.0625, 0.125), vec2(0.0625, 0.125),
    vec2(0., 0.), vec2(0.125, 0.), vec2(0.0625, 0.125),
);

fn specular_for(s: u32) -> vec3<f32> {
    if (s <= 8u) { return vec3<f32>(0.0); }
    // InitShadeTables: start at (71,48,23) for 15, subtract 71/8 per step, clamp >= 0,
    // then round(v / 256 * 255) (release uses cvtss2si: round half to even = WGSL round()).
    var c = vec3<f32>(71.0, 48.0, 23.0);
    for (var i = 0u; i < 15u - s; i = i + 1u) {
        c = max(c - vec3<f32>(71.0 / 8.0), vec3<f32>(0.0));
    }
    return round(c * (1.0 / 256.0) * 255.0);
}

@vertex
fn vs(@builtin(vertex_index) vid: u32) -> Out {
    let t = vid / 3u;
    let c = vid % 3u;
    let cell = t / 2u;
    let down = (t & 1u) == 1u;
    let cw = u.map_w - 1u;
    let cy = cell / cw;
    let cx = cell % cw;
    var nx = cx;
    var ny = cy;
    if (c == 2u) {
        nx = cx + 1u;
        ny = cy + 1u;
    } else if (c == 1u) {
        if (down) { nx = cx + 1u; } else { ny = cy + 1u; }
    }
    let node = nodes[ny * u.map_w + nx];

    // Position: (24x - 12y, (12y<<16 - CalcFinalHeightOffset(h)) >> 16) at the default
    // triangle size 24<<16; 0x3D533333 = the exe's 0.051562499f.
    let h = f32(node & 0xffu);
    let hoff = i32(h * bitcast<f32>(0x3D533333u) * 1572864.0);
    let wx = 24 * i32(nx) - 12 * i32(ny);
    let wy = (((12 * i32(ny)) << 16u) - hoff) >> 16u;

    var o: Out;
    o.pos = vec4<f32>((vec2<f32>(f32(wx), f32(wy)) - u.origin) * u.scale + vec2<f32>(-1.0, 1.0) + u.half_px, 0.0, 1.0);

    let piece = pieces[t];
    o.page = piece & 63u;
    var uv: vec2<f32>;
    let corner = select(0u, 3u, down) + c;
    if ((piece & 64u) != 0u) {
        let ua = fract((f32(cx) - f32(cy) / 2.0) / 8.0 + 1.0 / 16.0);
        let vy = f32(cy % 8u) / 8.0;
        var tiled = TILED;
        uv = vec2<f32>(ua, vy) + tiled[corner];
        o.bounds = vec4<f32>(vec2<f32>(-1e9), vec2<f32>(1e9));
    } else {
        let cell_uv = vec2<f32>(f32((piece >> 7u) & 3u), f32((piece >> 9u) & 7u)) * 0.25;
        let k = (piece >> 12u) & 7u;
        var pat = PATTERN;
        uv = cell_uv + pat[k * 3u + c] * 0.0625;
        // The +1/512 bias below puts the bottom and right cell edges on the first texel of the
        // next cell. At zoom 1 on flat ground nothing samples there (vertices on whole pixels, the
        // pixels on a horizontal edge belong to the triangle below); at other zooms pixel
        // centres fall up to half a texel short of the edge and fetch the neighbouring cell or
        // the filler: 1 px lines in the colour of that cell along k = 0 bottom edges. The clamp
        // also changes the few zoom-1 pixels on slopes and bilinear footprints that reached
        // the next cell.
        o.bounds = vec4<f32>(cell_uv + vec2<f32>(1.0 / 512.0), cell_uv + vec2<f32>(0.25 - 1.0 / 512.0));
    }
    o.uv = uv + vec2<f32>(1.0 / 512.0);

    // Shading index: byte 2 low nibble, forced to 0 by bit 7.
    let grad = (node >> 16u) & 0xffu;
    var s = grad & 15u;
    if ((grad & 0x80u) != 0u) { s = 0u; }
    var diffuse_tbl = DIFFUSE;
    var d = vec3<f32>(diffuse_tbl[s]);
    if (s == 0u) { d = vec3<f32>(71.0, 36.0, 73.0); }
    var sp = specular_for(s);
    if ((u.flags & 4u) != 0u) {
        let ni = ny * u.map_w + nx;
        d = d * (f32((ao[ni / 4u] >> (8u * (ni % 4u))) & 255u) / 255.0);
    }

    // CHeightAndTypeTable::CalcFogging with the node's old/new fog level (byte 3).
    let gfx = node >> 24u;
    let fog_old = i32(gfx & 7u);
    let fog_new = i32((gfx >> 3u) & 7u);
    // DrawTexturedLandscape: rows above g_iFadeYBorder use step 0; the lower vertices of the row
    // just above it already use the global step (g_iUsedFogFadeStep = 15 there).
    let fade = select(i32(u.fade), 0, i32(ny) < u.border);
    let mix_ = fog_old * (15 - fade) + fog_new * fade;
    let f = 16 * mix_ / 15;
    if (f != 0) {
        let idx = u32(f >> 4u);
        let frac = u32(f & 15);
        var ramp = FOG_RAMP;
        let a = ramp[8u - idx];
        var b = a;
        if (idx != 8u) { b = ramp[7u - idx]; }
        let lum = min((a * (15u - frac) + b * frac) >> 4u, u32(d.b));
        d = round(d * (f32(lum) / 255.0));
        sp = round(sp * (1.0 - f32(mix_) / 105.0));
    }
    if ((u.flags & 2u) != 0u) {
        d = vec3<f32>(255.0);
        sp = vec3<f32>(0.0);
    }
    o.diffuse = d;
    o.specular = sp;
    return o;
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    // Gradients of the unclamped UV, so the clamp leaves the mip choice alone.
    let uv = clamp(i.uv, i.bounds.xy, i.bounds.zw);
    let t = textureSampleGrad(pages, samp, uv, i.page, dpdx(i.uv), dpdy(i.uv)).rgb;
    var c = clamp(t * (i.diffuse / 255.0) + i.specular / 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
    if ((u.flags & 1u) != 0u) { c = to_linear(c); }
    return vec4<f32>(c, 1.0);
}
