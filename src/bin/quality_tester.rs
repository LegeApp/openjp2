//! Test utility to generate JP2 files at different quality levels
//! 
//! This program takes an input image and generates 10 JP2 files at different quality levels:
//! 20, 30, 40, 50, 60, 75, 80, 85, 90, 95

use openjp2::openjpeg::opj_set_default_encoder_parameters;
use openjp2::{
    opj_cparameters_t, opj_image, opj_image_comptparm, Codec, Stream, CODEC_FORMAT, OPJ_CLRSPC_SRGB,
};
use std::{env, fs, path::Path, process::exit, time::Instant};

fn usage_and_exit(program: &str) -> ! {
    eprintln!("Usage: {} <input.png> <output_dir>", program);
    eprintln!("  input.png: Input image file (PNG, JPEG, etc. - any format supported by the `image` crate)");
    eprintln!("  output_dir: Directory where output JP2 files will be saved");
    exit(1);
}

fn main() {
    println!("JP2 Quality Tester");
    println!("===================\n");

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();
    
    if args.len() != 3 {
        usage_and_exit(&program);
    }

    let input_path = &args[1];
    let output_dir = Path::new(&args[2]);

    // Create output directory if it doesn't exist
    if !output_dir.exists() {
        if let Err(e) = fs::create_dir_all(output_dir) {
            eprintln!("ERROR: Failed to create output directory '{}': {}", output_dir.display(), e);
            exit(1);
        }
    } else if !output_dir.is_dir() {
        eprintln!("ERROR: '{}' exists but is not a directory", output_dir.display());
        exit(1);
    }

    // Define quality levels to test
    let quality_levels = [20, 30, 40, 50, 60, 75, 80, 85, 90, 95];
    let mut results = Vec::new();

    // Load the input image once
    println!("Loading input image: {}", input_path);
    let img = match image::open(input_path) {
        Ok(img) => img,
        Err(e) => {
            eprintln!("ERROR: Failed to open '{}': {}", input_path, e);
            exit(1);
        }
    };

    // Get input file size
    let input_size = match fs::metadata(input_path) {
        Ok(meta) => meta.len(),
        Err(e) => {
            eprintln!("WARNING: Could not get input file size: {}", e);
            0
        }
    };

    println!("Input image size: {}x{} pixels", img.width(), img.height());
    println!("Input file size: {:.2} KB\n", input_size as f64 / 1024.0);
    println!("Generating JP2 files at different quality levels...\n");
    println!("Quality | Output File | Size (KB) | Ratio % | Time (ms)");
    println!("--------|-------------|-----------|---------|-----------");

    // Process each quality level
    for &quality in &quality_levels {
        let start_time = Instant::now();
        
        // Create output filename
        let output_filename = format!("quality_{:03}.jp2", quality);
        let output_path = output_dir.join(&output_filename);
        
        // Convert image to JP2 with the specified quality
        let result = convert_to_jp2(&img, &output_path, quality as f32);
        
        let duration = start_time.elapsed();
        
        match result {
            Ok(output_size) => {
                let ratio = (output_size as f64 / input_size as f64) * 100.0;
                println!("{:7} | {:<11} | {:8.2} | {:6.2}% | {:8.2}",
                         quality,
                         output_filename,
                         output_size as f64 / 1024.0,
                         ratio,
                         duration.as_secs_f64() * 1000.0);
                
                results.push((quality, output_size, ratio, duration));
            }
            Err(e) => {
                eprintln!("ERROR: Failed to generate quality {}: {}", quality, e);
            }
        }
    }
    
    // Print summary
    println!("\n=== Summary ===");
    println!("Input file: {}", input_path);
    println!("Output directory: {}", output_dir.display());
    println!("\nQuality | Size (KB) | Ratio % | Time (ms)");
    println!("--------|-----------|---------|-----------");
    
    for (quality, size, ratio, duration) in &results {
        println!("{:7} | {:9.2} | {:6.2}% | {:8.2}",
                 quality,
                 *size as f64 / 1024.0,
                 ratio,
                 duration.as_secs_f64() * 1000.0);
    }
}

fn convert_to_jp2(
    img: &image::DynamicImage,
    output_path: &Path,
    quality: f32,
) -> Result<u64, String> {
    // Convert to RGB8 if not already in that format
    let rgb = img.to_rgb8();
    let (width, height) = rgb.dimensions();
    let pixels = rgb.into_raw();

    // Prepare component parameters
    let mut cmptparms = Vec::with_capacity(3);
    for _ in 0..3 {
        cmptparms.push(opj_image_comptparm {
            dx: 1,
            dy: 1,
            w: width,
            h: height,
            x0: 0,
            y0: 0,
            prec: 8,
            bpp: 8,
            sgnd: 0,
        });
    }

    // Create JP2 image container
    let mut jp2_image = opj_image::create(&cmptparms, OPJ_CLRSPC_SRGB);
    jp2_image.x0 = 0;
    jp2_image.y0 = 0;
    jp2_image.x1 = width;
    jp2_image.y1 = height;

    // Copy pixel data to components
    if let Some(chans) = jp2_image.comps_data_mut_iter() {
        for (c, buf) in chans.enumerate() {
            for (i, pix) in buf.iter_mut().enumerate() {
                *pix = pixels[3 * i + c] as i32;
            }
        }
    } else {
        return Err("Failed to access image component buffers".to_string());
    }

    // Open output stream
    let mut stream = Stream::new_file(output_path, 1 << 20, false)
        .map_err(|e| format!("Failed to open output file: {}", e))?;

    // Create encoder
    let mut codec = Codec::new_encoder(CODEC_FORMAT::OPJ_CODEC_JP2)
        .ok_or_else(|| "Failed to create JP2 encoder".to_string())?;

    // Configure encoder parameters
    let mut params = opj_cparameters_t::default();
    unsafe { opj_set_default_encoder_parameters(&mut params) };
    
    if !(0.0..=100.0).contains(&quality) {
        return Err("Quality must be between 0 and 100".to_string());
    }
    
    params.tcp_numlayers = 1;
    params.tcp_rates[0] = 100.0 - quality;  // Lower rate means higher quality
    
    // Setup encoder
    if codec.setup_encoder(&mut params, &mut jp2_image) == 0 {
        return Err("setup_encoder failed".to_string());
    }

    // Start compression
    if codec.start_compress(&mut jp2_image, &mut stream) == 0 {
        return Err("start_compress failed".to_string());
    }

    // Encode the image
    if codec.encode(&mut stream) == 0 {
        return Err("encode failed".to_string());
    }

    // Finish compression
    if codec.end_compress(&mut stream) == 0 {
        return Err("end_compress failed".to_string());
    }

    // Flush to disk
    stream.flush().map_err(|e| format!("flush failed: {}", e))?;

    // Get output file size
    let output_size = fs::metadata(output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(output_size)
}
