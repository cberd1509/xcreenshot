//! MetalFX spatial scaler (macOS 13+) through objc2-metal-fx: upload, scale, read back.

use std::ffi::c_void;
use std::ptr::NonNull;

use image::RgbaImage;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLBlitCommandEncoder, MTLBuffer, MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandEncoder,
    MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLOrigin, MTLPixelFormat, MTLRegion,
    MTLResourceOptions, MTLSize, MTLStorageMode, MTLTexture, MTLTextureDescriptor, MTLTextureUsage,
};
use objc2_metal_fx::{
    MTLFXSpatialScaler, MTLFXSpatialScalerBase, MTLFXSpatialScalerColorProcessingMode,
    MTLFXSpatialScalerDescriptor,
};

pub type Mode = MTLFXSpatialScalerColorProcessingMode;

/// The device and queue are made once per process; everything else per call, since every
/// capture has its own size.
pub struct Gpu {
    pub device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
}

impl Gpu {
    pub fn new() -> Result<Self, String> {
        let device = MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
        if !unsafe { MTLFXSpatialScalerDescriptor::supportsDevice(&device) } {
            return Err("MetalFX spatial scaling is not supported on this GPU".into());
        }
        let queue = device.newCommandQueue().ok_or("no command queue")?;
        Ok(Self { device, queue })
    }

    pub fn name(&self) -> String {
        self.device.name().to_string()
    }

    /// Upscale an RGBA8 image by a whole factor. `format` is used for both textures.
    pub fn upscale(
        &self,
        src: &RgbaImage,
        factor: u32,
        format: MTLPixelFormat,
        mode: Mode,
    ) -> Result<RgbaImage, String> {
        let (w, h) = (src.width() as usize, src.height() as usize);
        let (ow, oh) = (w * factor as usize, h * factor as usize);
        let bpp = bytes_per_pixel(format);
        unsafe {
            let desc = MTLFXSpatialScalerDescriptor::new();
            desc.setInputWidth(w);
            desc.setInputHeight(h);
            desc.setOutputWidth(ow);
            desc.setOutputHeight(oh);
            desc.setColorTextureFormat(format);
            desc.setOutputTextureFormat(format);
            desc.setColorProcessingMode(mode);
            let scaler = desc
                .newSpatialScalerWithDevice(&self.device)
                .ok_or("MetalFX refused the descriptor")?;

            let input = self.texture(
                format,
                w,
                h,
                scaler.colorTextureUsage(),
                MTLStorageMode::Shared,
            )?;
            let pixels = to_format(src, format);
            input.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                region(w, h),
                0,
                NonNull::new(pixels.as_ptr() as *mut c_void).unwrap(),
                w * bpp,
            );
            // The output is a render target the GPU writes; keep it private and blit it out.
            let output = self.texture(
                format,
                ow,
                oh,
                scaler.outputTextureUsage(),
                MTLStorageMode::Private,
            )?;
            let readback = self
                .device
                .newBufferWithLength_options(ow * oh * bpp, MTLResourceOptions::StorageModeShared)
                .ok_or("could not allocate the readback buffer")?;

            scaler.setColorTexture(Some(&input));
            scaler.setOutputTexture(Some(&output));
            scaler.setInputContentWidth(w);
            scaler.setInputContentHeight(h);

            let cmd = self.queue.commandBuffer().ok_or("no command buffer")?;
            scaler.encodeToCommandBuffer(&cmd);
            let blit = cmd.blitCommandEncoder().ok_or("no blit encoder")?;
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                &output,
                0,
                0,
                MTLOrigin { x: 0, y: 0, z: 0 },
                MTLSize { width: ow, height: oh, depth: 1 },
                &readback,
                0,
                ow * bpp,
                ow * oh * bpp,
            );
            blit.endEncoding();
            cmd.commit();
            cmd.waitUntilCompleted();
            if cmd.status() != MTLCommandBufferStatus::Completed {
                return Err(format!("command buffer failed: {:?}", cmd.error()));
            }
            let bytes = std::slice::from_raw_parts(
                readback.contents().as_ptr() as *const u8,
                ow * oh * bpp,
            );
            Ok(from_format(bytes, ow as u32, oh as u32, format))
        }
    }

    fn texture(
        &self,
        format: MTLPixelFormat,
        w: usize,
        h: usize,
        usage: MTLTextureUsage,
        storage: MTLStorageMode,
    ) -> Result<Retained<ProtocolObject<dyn MTLTexture>>, String> {
        let td = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                format, w, h, false,
            )
        };
        td.setUsage(usage | MTLTextureUsage::ShaderRead);
        td.setStorageMode(storage);
        self.device
            .newTextureWithDescriptor(&td)
            .ok_or_else(|| format!("could not make a {w}x{h} texture"))
    }
}

fn region(w: usize, h: usize) -> MTLRegion {
    MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    }
}

fn bytes_per_pixel(format: MTLPixelFormat) -> usize {
    if format == MTLPixelFormat::RGBA16Float {
        8
    } else {
        4
    }
}

fn is_bgra(format: MTLPixelFormat) -> bool {
    format == MTLPixelFormat::BGRA8Unorm || format == MTLPixelFormat::BGRA8Unorm_sRGB
}

/// RGBA8 -> the texture's byte layout. Half floats keep the sRGB-encoded values (0..1).
fn to_format(src: &RgbaImage, format: MTLPixelFormat) -> Vec<u8> {
    if format == MTLPixelFormat::RGBA16Float {
        return src
            .as_raw()
            .iter()
            .flat_map(|&v| f32_to_f16(v as f32 / 255.0).to_le_bytes())
            .collect();
    }
    let mut out = src.as_raw().clone();
    if is_bgra(format) {
        out.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
    }
    out
}

fn from_format(bytes: &[u8], w: u32, h: u32, format: MTLPixelFormat) -> RgbaImage {
    let raw: Vec<u8> = if format == MTLPixelFormat::RGBA16Float {
        bytes
            .chunks_exact(2)
            .map(|b| {
                (f16_to_f32(u16::from_le_bytes([b[0], b[1]])) * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8
            })
            .collect()
    } else {
        let mut v = bytes.to_vec();
        if is_bgra(format) {
            v.chunks_exact_mut(4).for_each(|p| p.swap(0, 2));
        }
        v
    };
    RgbaImage::from_raw(w, h, raw).unwrap()
}

// Minimal f16 conversion, enough for 0..1 colour values in the format test.
fn f32_to_f16(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mant = bits & 0x7f_ffff;
    if v == 0.0 || exp <= 0 {
        return sign;
    }
    sign | ((exp as u16) << 10) | ((mant >> 13) as u16)
}

fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1f) as i32;
    let mant = (h & 0x3ff) as f32;
    match exp {
        0 => sign * mant * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => sign * (1.0 + mant / 1024.0) * 2f32.powi(exp - 15),
    }
}
