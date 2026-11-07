//! Binary-size probe. Build it with one feature set at a time and compare stripped sizes:
//!   --no-default-features                      PNG decode/encode only (what the app has)
//!   --no-default-features --features probe-image-lanczos
//!   --no-default-features --features fir
//!   --no-default-features --features metalfx

#[allow(unused_variables)]
fn main() {
    let path = std::env::args().nth(1).expect("png path");
    let img = image::open(&path).unwrap().to_rgba8();
    let (w, h) = img.dimensions();
    #[allow(unused_mut)]
    let mut out = img.clone();

    #[cfg(feature = "probe-image-lanczos")]
    {
        out = image::imageops::resize(&img, w * 2, h * 2, image::imageops::FilterType::Lanczos3);
    }

    #[cfg(feature = "fir")]
    {
        use fast_image_resize::{
            PixelType, ResizeOptions, Resizer,
            images::{Image, ImageRef},
        };
        let src = ImageRef::new(w, h, img.as_raw(), PixelType::U8x4).unwrap();
        let mut dst = Image::new(w * 2, h * 2, PixelType::U8x4);
        Resizer::new()
            .resize(&src, &mut dst, &ResizeOptions::new())
            .unwrap();
        out = image::RgbaImage::from_raw(w * 2, h * 2, dst.into_vec()).unwrap();
    }

    #[cfg(all(target_os = "macos", feature = "metalfx"))]
    {
        use objc2_metal::{MTLCreateSystemDefaultDevice, MTLPixelFormat};
        use objc2_metal_fx::{MTLFXSpatialScalerBase, MTLFXSpatialScalerDescriptor};
        let device = MTLCreateSystemDefaultDevice().unwrap();
        unsafe {
            let desc = MTLFXSpatialScalerDescriptor::new();
            desc.setInputWidth(w as usize);
            desc.setInputHeight(h as usize);
            desc.setOutputWidth(w as usize * 2);
            desc.setOutputHeight(h as usize * 2);
            desc.setColorTextureFormat(MTLPixelFormat::RGBA8Unorm);
            desc.setOutputTextureFormat(MTLPixelFormat::RGBA8Unorm);
            let scaler = desc.newSpatialScalerWithDevice(&device).unwrap();
            println!("{:?}", scaler.outputTextureUsage());
        }
    }

    out.save(format!("{path}.out.png")).unwrap();
}
