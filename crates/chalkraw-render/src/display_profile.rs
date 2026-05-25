//! Phase 8 polish: display profile detection and 3D LUT construction.
//!
//! On Windows, reads the primary monitor's ICC profile via Win32 `GetICMProfileW`,
//! then uses qcms to build a 32×32×32 sRGB→display 3D LUT that is uploaded to
//! the GPU as an Rgba16Float D3 texture (binding 8).
//!
//! macOS and Linux return `None` from `read_display_icc_profile()` for now;
//! ColorSync / colord integration is a future polish item. An identity LUT is
//! always uploaded so binding 8 is always populated.

use crate::device::RenderDevice;
use crate::source::f32_to_f16_bits;

/// A 32×32×32 display 3D LUT — Rgba16Float, stored as f16 bit-pattern u16 values.
/// Layout: b-major (outermost loop b, then g, then r innermost).
pub struct DisplayLut {
    pub data: Vec<u16>, // 32*32*32*4 u16 values (RGBA, f16)
    pub size: u32,      // 32
}

/// Read the primary display's ICC profile. Returns `None` on macOS/Linux
/// (sRGB assumed) or when the profile cannot be obtained.
pub fn read_display_icc_profile() -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        windows_impl::read_primary_monitor_profile()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Build a 32×32×32 sRGB→display 3D LUT using qcms.
/// Returns `None` if the profile cannot be parsed or the transform cannot be
/// created (e.g. unsupported colour space).
pub fn build_srgb_to_display_lut(display_icc_bytes: &[u8]) -> Option<DisplayLut> {
    let dst_profile = qcms::Profile::new_from_slice(display_icc_bytes, true)?;
    let src_profile = qcms::Profile::new_sRGB();

    // Fast path: if destination is already sRGB the LUT is the identity. Return
    // None so the caller uses the pre-built identity LUT and skips the shader sample.
    if dst_profile.is_sRGB() {
        log::debug!("display_profile: display ICC is sRGB — no LUT needed");
        return None;
    }

    let xform = qcms::Transform::new_to(
        &src_profile,
        &dst_profile,
        qcms::DataType::RGBA8,
        qcms::DataType::RGBA8,
        qcms::Intent::Perceptual,
    )?;

    let size = 32u32;
    let total = (size * size * size) as usize;

    // Build the cube grid as RGBA8 bytes.
    let mut samples_u8: Vec<u8> = Vec::with_capacity(total * 4);
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let rv = (r as f32 / (size as f32 - 1.0) * 255.0).round() as u8;
                let gv = (g as f32 / (size as f32 - 1.0) * 255.0).round() as u8;
                let bv = (b as f32 / (size as f32 - 1.0) * 255.0).round() as u8;
                samples_u8.extend_from_slice(&[rv, gv, bv, 255]);
            }
        }
    }

    // Apply the ICC transform in-place.
    xform.apply(&mut samples_u8);

    // Convert RGBA8 → Rgba16Float (f16 bit patterns).
    let mut data: Vec<u16> = Vec::with_capacity(total * 4);
    for chunk in samples_u8.chunks_exact(4) {
        for &c in chunk {
            data.push(f32_to_f16_bits(c as f32 / 255.0));
        }
    }

    log::info!("display_profile: built 32^3 sRGB→display 3D LUT ({} entries)", total);
    Some(DisplayLut { data, size })
}

/// Build an identity 32×32×32 3D LUT (sRGB passthrough) for use when no
/// display profile is available. The shader binding must always be populated.
pub fn build_identity_lut() -> DisplayLut {
    let size = 32u32;
    let total = (size * size * size) as usize;
    let mut data: Vec<u16> = Vec::with_capacity(total * 4);
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let rf = r as f32 / (size as f32 - 1.0);
                let gf = g as f32 / (size as f32 - 1.0);
                let bf = b as f32 / (size as f32 - 1.0);
                data.push(f32_to_f16_bits(rf));
                data.push(f32_to_f16_bits(gf));
                data.push(f32_to_f16_bits(bf));
                data.push(f32_to_f16_bits(1.0));
            }
        }
    }
    DisplayLut { data, size }
}

/// Upload a `DisplayLut` as a wgpu 3D texture (Rgba16Float, D3).
/// Returns (texture, view) — the caller must keep the texture alive.
pub fn upload_lut_3d(rd: &RenderDevice, lut: &DisplayLut) -> (wgpu::Texture, wgpu::TextureView) {
    let extent = wgpu::Extent3d {
        width: lut.size,
        height: lut.size,
        depth_or_array_layers: lut.size,
    };
    let tex = rd.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("display 3D LUT"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    rd.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&lut.data),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(lut.size * 4 * 2),  // 4 channels × 2 bytes (f16)
            rows_per_image: Some(lut.size),
        },
        extent,
    );
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    (tex, view)
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(windows)]
mod windows_impl {
    use std::path::PathBuf;
    use windows::Win32::Graphics::Gdi::{GetDC, ReleaseDC};
    use windows::core::PWSTR;

    pub fn read_primary_monitor_profile() -> Option<Vec<u8>> {
        // SAFETY: Win32 call. GetDC(None) returns the screen DC.
        unsafe {
            let hdc = GetDC(None);
            if hdc.is_invalid() {
                log::warn!("display_profile: GetDC failed for primary monitor");
                return None;
            }

            // First call: retrieve the required buffer size (in WCHARs including null).
            let mut size: u32 = 0;
            // GetICMProfileW with null PWSTR retrieves the size. The return value is
            // FALSE when called with a null buffer (expected); size is set.
            let _ = windows::Win32::UI::ColorSystem::GetICMProfileW(hdc, &mut size, PWSTR::null());

            if size == 0 {
                let _ = ReleaseDC(None, hdc);
                log::info!("display_profile: no display ICC profile found via GetICMProfileW");
                return None;
            }

            // Second call: retrieve the actual path.
            let mut buf: Vec<u16> = vec![0u16; size as usize];
            let ok = windows::Win32::UI::ColorSystem::GetICMProfileW(
                hdc,
                &mut size,
                PWSTR(buf.as_mut_ptr()),
            )
            .is_ok();
            let _ = ReleaseDC(None, hdc);

            if !ok {
                log::warn!("display_profile: GetICMProfileW failed on second call");
                return None;
            }

            // The returned string is null-terminated; strip the null.
            let len = size.saturating_sub(1) as usize;
            let path_str = String::from_utf16_lossy(&buf[..len]);
            let path = PathBuf::from(path_str.trim_end_matches('\0'));
            log::info!("display_profile: primary monitor ICC profile path: {:?}", path);

            match std::fs::read(&path) {
                Ok(bytes) => {
                    log::info!("display_profile: loaded {} bytes from {:?}", bytes.len(), path);
                    Some(bytes)
                }
                Err(e) => {
                    log::warn!("display_profile: failed to read ICC file {:?}: {}", path, e);
                    None
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: read_display_icc_profile must not panic on any platform.
    #[test]
    fn read_display_icc_profile_does_not_panic() {
        // On Linux/macOS returns None; on Windows may return bytes or None.
        let _ = read_display_icc_profile();
    }

    /// Identity LUT must have the correct number of entries and sample
    /// (0,0,0) → (0.0, 0.0, 0.0, 1.0) and (31,31,31) → (1.0, 1.0, 1.0, 1.0).
    #[test]
    fn identity_lut_has_correct_size_and_corners() {
        let lut = build_identity_lut();
        let size = lut.size;
        assert_eq!(size, 32);
        assert_eq!(lut.data.len(), (size * size * size * 4) as usize);

        // Entry at r=0, g=0, b=0 (first entry): all channels should be ≈ 0.0.
        // f16 representation of 0.0 is 0x0000.
        let entry0 = &lut.data[0..4];
        assert_eq!(entry0[0], 0x0000, "r=0 should be f16(0.0)=0x0000");
        assert_eq!(entry0[1], 0x0000, "g=0 should be f16(0.0)=0x0000");
        assert_eq!(entry0[2], 0x0000, "b=0 should be f16(0.0)=0x0000");
        // A channel is always 1.0 = 0x3c00
        assert_eq!(entry0[3], 0x3c00, "alpha should be f16(1.0)=0x3c00");

        // Last entry (r=31, g=31, b=31): all channels should be f16(1.0) = 0x3c00.
        let last_start = ((size * size * size - 1) * 4) as usize;
        let entry_last = &lut.data[last_start..last_start + 4];
        assert_eq!(entry_last[0], 0x3c00, "r=31 should be f16(1.0)=0x3c00");
        assert_eq!(entry_last[1], 0x3c00, "g=31 should be f16(1.0)=0x3c00");
        assert_eq!(entry_last[2], 0x3c00, "b=31 should be f16(1.0)=0x3c00");
        assert_eq!(entry_last[3], 0x3c00, "alpha should be f16(1.0)=0x3c00");
    }
}
