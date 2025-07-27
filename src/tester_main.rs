// src/tester_main.rs

use clap::{Args, Parser, Subcommand};
use image::{DynamicImage, GenericImageView};
use openjp2::c_api_types::{
  OPJ_CODEC_FORMAT, OPJ_COLOR_SPACE, OPJ_CPRL, OPJ_LRCP, OPJ_PCRL, OPJ_PROG_ORDER, OPJ_RLCP,
  OPJ_RPCL,
};
use openjp2::consts::{OPJ_FALSE, OPJ_TRUE};
use openjp2::image::opj_image_cmptparm_t;
use openjp2::openjpeg::{
  opj_create_compress, opj_destroy_codec, opj_encode, opj_end_compress, opj_image_create,
  opj_image_destroy, opj_set_default_encoder_parameters, opj_setup_encoder, opj_start_compress,
  opj_stream_create_default_file_stream, opj_stream_destroy,
};
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};

fn parse_prog_order_str(s: &str) -> Result<OPJ_PROG_ORDER, String> {
  match s.to_uppercase().as_str() {
    "LRCP" => Ok(OPJ_LRCP),
    "RLCP" => Ok(OPJ_RLCP),
    "RPCL" => Ok(OPJ_RPCL),
    "PCRL" => Ok(OPJ_PCRL),
    "CPRL" => Ok(OPJ_CPRL),
    _ => Err(format!("Invalid progression order: {}", s)),
  }
}

#[derive(Debug, Clone, clap::ValueEnum)]
enum ParamSet {
  DefaultLossy,
  DefaultLossless,
  Custom,
}

impl ParamSet {
  fn to_opj_cparameters(&self, overrides: &EncodingOverrides) -> openjp2::opj_cparameters_t {
    let mut params = unsafe { MaybeUninit::zeroed().assume_init() };
    unsafe {
      opj_set_default_encoder_parameters(&mut params);
    }

    match self {
      ParamSet::DefaultLossy => {
        params.irreversible = OPJ_TRUE as i32; // Use irreversible DWT (e.g., 9/7)
                                               // Configure for actual lossy compression by rate
        params.tcp_numlayers = 1;
        params.tcp_rates[0] = 10.0; // Example: Target 10:1 compression ratio.
                                    // Adjust as needed. Lower values mean higher quality.
        params.cp_disto_alloc = 1; // Allocate rates to layers based on tcp_rates
      }
      ParamSet::DefaultLossless => {
        params.tcp_numlayers = 1;
        params.tcp_rates[0] = 0.0;
        params.cp_disto_alloc = 0;
        params.cp_fixed_quality = 0;
        params.irreversible = OPJ_FALSE as i32;
      }
      ParamSet::Custom => {
        params.irreversible = OPJ_TRUE as i32;
      }
    }

    if let Some(num_res) = overrides.num_resolutions {
      params.numresolution = num_res;
    }
    if let Some(prog_order_val) = overrides.prog_order {
      params.prog_order = prog_order_val;
    }
    if let Some(rate_val) = overrides.rate {
      if rate_val > 0.0 {
        params.tcp_numlayers = 1;
        params.tcp_rates[0] = rate_val;
        params.cp_disto_alloc = 1;
        params.irreversible = OPJ_TRUE as i32;
      } else {
        params.tcp_numlayers = 1;
        params.tcp_rates[0] = 0.0;
        params.cp_disto_alloc = 0;
        params.irreversible = OPJ_FALSE as i32;
      }
    }
    if let Some(irreversible_override) = overrides.irreversible {
      params.irreversible = if irreversible_override {
        OPJ_TRUE as i32
      } else {
        OPJ_FALSE as i32
      };
      if !irreversible_override && overrides.rate.is_none() {
        params.tcp_rates[0] = 0.0;
        params.cp_disto_alloc = 0;
      }
    }
    params
  }
}

#[derive(Args, Debug, Clone)]
struct EncodingOverrides {
  #[clap(long)]
  num_resolutions: Option<i32>,
  #[clap(long, value_parser = parse_prog_order_str)]
  prog_order: Option<OPJ_PROG_ORDER>,
  #[clap(long)]
  rate: Option<f32>,
  #[clap(long)]
  irreversible: Option<bool>,
}

#[derive(Parser, Debug)]
#[clap(name = "openjp2_tester", version = "0.1", author = "Your Name")]
struct Cli {
  #[command(subcommand)]
  command: Commands,
  #[clap(flatten)]
  overrides: EncodingOverrides,
}

#[derive(Subcommand, Debug)]
enum Commands {
  Single(SingleArgs),
  Folder(FolderArgs),
}

#[derive(Args, Debug)]
struct SingleArgs {
  #[clap(short, long, value_parser)]
  input: PathBuf,
  #[clap(short, long, value_parser)]
  output: PathBuf,
  #[clap(long, value_parser = clap::value_parser!(ParamSet), default_value = "default-lossy")]
  params: ParamSet,
}

#[derive(Args, Debug)]
struct FolderArgs {
  #[clap(short, long, value_parser)]
  input_folder: PathBuf,
  #[clap(short, long, value_parser)]
  output_folder: PathBuf,
  #[clap(long, value_parser = clap::value_parser!(ParamSet), default_value = "default-lossy")]
  params: ParamSet,
}

fn create_opj_image_from_image_crate(
  dyn_img: DynamicImage,
  _cparams: &openjp2::opj_cparameters_t,
) -> Result<*mut openjp2::opj_image_t, String> {
  let (width, height) = dyn_img.dimensions();
  let (num_comps, color_space) = match dyn_img {
    DynamicImage::ImageLuma8(_) => (1, OPJ_COLOR_SPACE::OPJ_CLRSPC_GRAY),
    DynamicImage::ImageLumaA8(_) => (2, OPJ_COLOR_SPACE::OPJ_CLRSPC_GRAY),
    DynamicImage::ImageRgb8(_) => (3, OPJ_COLOR_SPACE::OPJ_CLRSPC_SRGB),
    DynamicImage::ImageRgba8(_) => (4, OPJ_COLOR_SPACE::OPJ_CLRSPC_SRGB),
    _ => return Err("Unsupported image type for OpenJPEG conversion".to_string()),
  };

  let mut image_params_vec: Vec<opj_image_cmptparm_t> = Vec::with_capacity(num_comps);
  for _ in 0..num_comps {
    let mut p = unsafe { MaybeUninit::<opj_image_cmptparm_t>::zeroed().assume_init() };
    p.dx = 1;
    p.dy = 1;
    p.w = width;
    p.h = height;
    p.prec = 8;
    p.sgnd = 0;
    p.x0 = 0;
    p.y0 = 0;
    image_params_vec.push(p);
  }

  let opj_img = opj_image_create(num_comps as u32, image_params_vec.as_mut_ptr(), color_space);

  if opj_img.is_null() {
    return Err("Failed to create opj_image (opj_image_create returned null)".to_string());
  }

  unsafe {
    match dyn_img {
      DynamicImage::ImageLuma8(gray_img) => {
        let pixels = gray_img.as_raw();
        let comp = &mut *(*opj_img).comps.add(0);
        for i in 0..(width * height) as usize {
          *comp.data.add(i) = pixels[i] as i32;
        }
      }
      DynamicImage::ImageLumaA8(la_img) => {
        let pixels = la_img.as_raw();
        let comp_l = &mut *(*opj_img).comps.add(0);
        let comp_a = &mut *(*opj_img).comps.add(1);
        for y_idx in 0..height {
          for x_idx in 0..width {
            let src_pixel_idx = ((y_idx * width + x_idx) * 2) as usize;
            let dest_comp_idx = (y_idx * width + x_idx) as usize;
            *comp_l.data.add(dest_comp_idx) = pixels[src_pixel_idx] as i32;
            *comp_a.data.add(dest_comp_idx) = pixels[src_pixel_idx + 1] as i32;
          }
        }
      }
      DynamicImage::ImageRgb8(rgb_img) => {
        let pixels = rgb_img.as_raw();
        let comp_r = &mut *(*opj_img).comps.add(0);
        let comp_g = &mut *(*opj_img).comps.add(1);
        let comp_b = &mut *(*opj_img).comps.add(2);
        for y_idx in 0..height {
          for x_idx in 0..width {
            let src_pixel_idx = ((y_idx * width + x_idx) * 3) as usize;
            let dest_comp_idx = (y_idx * width + x_idx) as usize;
            *comp_r.data.add(dest_comp_idx) = pixels[src_pixel_idx] as i32;
            *comp_g.data.add(dest_comp_idx) = pixels[src_pixel_idx + 1] as i32;
            *comp_b.data.add(dest_comp_idx) = pixels[src_pixel_idx + 2] as i32;
          }
        }
      }
      DynamicImage::ImageRgba8(rgba_img) => {
        let pixels = rgba_img.as_raw();
        let comp_r = &mut *(*opj_img).comps.add(0);
        let comp_g = &mut *(*opj_img).comps.add(1);
        let comp_b = &mut *(*opj_img).comps.add(2);
        let comp_a = &mut *(*opj_img).comps.add(3);
        for y_idx in 0..height {
          for x_idx in 0..width {
            let src_pixel_idx = ((y_idx * width + x_idx) * 4) as usize;
            let dest_comp_idx = (y_idx * width + x_idx) as usize;
            *comp_r.data.add(dest_comp_idx) = pixels[src_pixel_idx] as i32;
            *comp_g.data.add(dest_comp_idx) = pixels[src_pixel_idx + 1] as i32;
            *comp_b.data.add(dest_comp_idx) = pixels[src_pixel_idx + 2] as i32;
            *comp_a.data.add(dest_comp_idx) = pixels[src_pixel_idx + 3] as i32;
          }
        }
      }
      _ => {
        opj_image_destroy(opj_img);
        return Err("Unsupported image type during pixel data population".to_string());
      }
    }
  }

  Ok(opj_img)
}

fn process_single_image(
  input_path: &Path,
  output_path: &Path,
  params_set: &ParamSet,
  overrides: &EncodingOverrides,
) -> Result<(), String> {
  println!(
    "Processing single image: {:?} -> {:?}",
    input_path, output_path
  );
  println!(
    "Parameter set: {:?}, Overrides: {:?}",
    params_set, overrides
  );

  let img = image::open(input_path)
    .map_err(|e| format!("Failed to open image: {}: {}", input_path.display(), e))?;
  println!(
    "Input image dimensions: {}x{}, Color type: {:?}",
    img.width(),
    img.height(),
    img.color()
  );

  let mut params = params_set.to_opj_cparameters(overrides);
  let codec_format = match output_path.extension().and_then(|s| s.to_str()) {
    Some("j2k") => OPJ_CODEC_FORMAT::OPJ_CODEC_J2K,
    Some("jp2") => OPJ_CODEC_FORMAT::OPJ_CODEC_JP2,
    _ => {
      return Err(format!(
        "Output file extension must be .j2k or .jp2: {}",
        output_path.display()
      ))
    }
  };

  let opj_img = match create_opj_image_from_image_crate(img, &params) {
    Ok(img_ptr) => img_ptr,
    Err(e) => return Err(format!("Failed to create OpenJPEG image structure: {}", e)),
  };

  let encoding_result = (|| {
    let l_codec = unsafe { opj_create_compress(codec_format) };
    if l_codec.is_null() {
      return Err("Failed to create OpenJPEG codec".to_string());
    }

    let setup_res = unsafe { opj_setup_encoder(l_codec, &mut params, opj_img) };
    if setup_res == OPJ_FALSE as i32 {
      unsafe { opj_destroy_codec(l_codec) };
      return Err("Failed to setup OpenJPEG encoder".to_string());
    }

    let c_output_path_str = output_path
      .to_str()
      .ok_or_else(|| "Output path is not valid UTF-8".to_string())?;
    let c_output_path = CString::new(c_output_path_str)
      .map_err(|e| format!("Failed to create CString for output path: {}", e))?;

    let l_stream =
      unsafe { opj_stream_create_default_file_stream(c_output_path.as_ptr(), OPJ_FALSE as i32) };
    if l_stream.is_null() {
      unsafe { opj_destroy_codec(l_codec) };
      return Err(format!(
        "Failed to create OpenJPEG stream for: {}",
        output_path.display()
      ));
    }

    let start_compress_res = unsafe { opj_start_compress(l_codec, opj_img, l_stream) };
    println!("opj_start_compress result: {}", start_compress_res);

    let encode_res = unsafe { opj_encode(l_codec, l_stream) };
    println!("opj_encode result: {}", encode_res);

    let end_compress_res = unsafe { opj_end_compress(l_codec, l_stream) };
    println!("opj_end_compress result: {}", end_compress_res);

    let success = start_compress_res == OPJ_TRUE as i32
      && encode_res == OPJ_TRUE as i32
      && end_compress_res == OPJ_TRUE as i32;

    unsafe {
      opj_stream_destroy(l_stream);
      opj_destroy_codec(l_codec);
    }

    if success {
      println!("Successfully encoded image to {:?}", output_path);
      Ok(())
    } else {
      Err(format!(
        "OpenJPEG encoding failed for: {}",
        output_path.display()
      ))
    }
  })();

  if !opj_img.is_null() {
    unsafe { opj_image_destroy(opj_img) };
  }

  encoding_result
}

fn main() -> Result<(), String> {
  let cli = Cli::parse();

  match cli.command {
    Commands::Single(args) => {
      process_single_image(&args.input, &args.output, &args.params, &cli.overrides)
    }
    Commands::Folder(args) => {
      if !args.output_folder.exists() {
        std::fs::create_dir_all(&args.output_folder).map_err(|e| {
          format!(
            "Failed to create output folder: {}: {}",
            args.output_folder.display(),
            e
          )
        })?;
      }

      for entry in std::fs::read_dir(&args.input_folder).map_err(|e| {
        format!(
          "Failed to read input folder: {}: {}",
          args.input_folder.display(),
          e
        )
      })? {
        let entry = entry.map_err(|e| format!("Error reading directory entry: {}", e))?;
        let path = entry.path();
        if path.is_file() {
          if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            let output_extension = "jp2";
            let output_file_path = args
              .output_folder
              .join(name)
              .with_extension(output_extension);

            println!("Processing file: {:?} -> {:?}", path, output_file_path);
            match process_single_image(&path, &output_file_path, &args.params, &cli.overrides) {
              Ok(_) => println!("Successfully processed {:?}", path),
              Err(e) => eprintln!("Failed to process {:?}: {}", path, e),
            }
          }
        }
      }
      Ok(())
    }
  }
}
