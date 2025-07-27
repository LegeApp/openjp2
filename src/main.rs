//! Command-line PNG → JP2/J2K converter with debug logging.

use openjp2::openjpeg::opj_set_default_encoder_parameters;
use openjp2::{
  opj_cparameters_t, opj_image, opj_image_comptparm, Codec, Stream, CODEC_FORMAT, OPJ_CLRSPC_SRGB,
};
use std::{env, path::Path, process::exit};

fn usage_and_exit(program: &str) -> ! {
  eprintln!(
    "Usage: {} <input.png> <output.jp2|output.j2k> [quality 0-100] [jp2|j2k]",
    program
  );
  exit(1);
}

fn main() {
  println!("DEBUG: png2jp2 starting");
  let mut args = env::args();
  let prog = args.next().unwrap_or_else(|| "png2jp2".into());

  // --- Parse arguments ---
  let input = args.next().unwrap_or_else(|| usage_and_exit(&prog));
  let output = args.next().unwrap_or_else(|| usage_and_exit(&prog));
  let quality: f32 = args.next().and_then(|q| q.parse().ok()).unwrap_or(100.0);
  let fmt_arg = args.next().unwrap_or_else(|| {
    if output.to_lowercase().ends_with(".j2k") {
      "j2k".into()
    } else {
      "jp2".into()
    }
  });

  println!(
    "DEBUG: input='{}', output='{}', quality={}, fmt_arg='{}'",
    input, output, quality, fmt_arg
  );

  // --- Determine codec format ---
  let codec_fmt = match fmt_arg.to_lowercase().as_str() {
    "j2k" => CODEC_FORMAT::OPJ_CODEC_J2K,
    "codestream" => CODEC_FORMAT::OPJ_CODEC_J2K,
    "jp2" => CODEC_FORMAT::OPJ_CODEC_JP2,
    "container" => CODEC_FORMAT::OPJ_CODEC_JP2,
    other => {
      eprintln!("ERROR: Unknown format '{}'; use 'jp2' or 'j2k'", other);
      exit(1);
    }
  };
  println!("DEBUG: selected codec format = {:?}", codec_fmt);

  // --- Load & convert image ---
  let img = image::open(&input).unwrap_or_else(|e| {
    eprintln!("ERROR: Failed to open '{}': {}", input, e);
    exit(1);
  });
  let rgb = img.to_rgb8();
  let (width, height) = rgb.dimensions();
  let pixels = rgb.into_raw();
  println!("DEBUG: image loaded, {}x{}", width, height);

  // --- Prepare component parameters ---
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
  println!("DEBUG: component parameters prepared");

  // --- Create JP2 image container ---
  let mut jp2_image = opj_image::create(&cmptparms, OPJ_CLRSPC_SRGB);
  // Set image boundaries (required for encoding)
  jp2_image.x0 = 0;
  jp2_image.y0 = 0;
  jp2_image.x1 = width;
  jp2_image.y1 = height;
  println!(
    "DEBUG: openjp2 image container created (bounds = {}..{} x {}..{})",
    jp2_image.x0, jp2_image.x1, jp2_image.y0, jp2_image.y1
  );

  // --- Copy pixel data to components ---
  if let Some(chans) = jp2_image.comps_data_mut_iter() {
    for (c, buf) in chans.enumerate() {
      for (i, pix) in buf.iter_mut().enumerate() {
        *pix = pixels[3 * i + c] as i32;
      }
    }
    println!("DEBUG: pixel data copied");
  } else {
    eprintln!("ERROR: Failed to access image component buffers");
    exit(1);
  }

  // --- Open output stream ---
  let path = Path::new(&output);
  let mut stream = Stream::new_file(path, 1 << 20, false).unwrap_or_else(|e| {
    eprintln!("ERROR: Failed to open '{}': {}", output, e);
    exit(1);
  });
  println!("DEBUG: output stream opened");

  // --- Instantiate encoder ---
  let mut codec = Codec::new_encoder(codec_fmt).unwrap_or_else(|| {
    eprintln!("ERROR: Failed to create encoder for {:?}", codec_fmt);
    exit(1);
  });
  println!("DEBUG: codec instantiated");

  // --- Default & tweak encoder params ---
  let mut params = opj_cparameters_t::default();
  unsafe { opj_set_default_encoder_parameters(&mut params) };
  println!("DEBUG: encoder parameters defaulted");

  if !(0.0..=100.0).contains(&quality) {
    eprintln!("ERROR: Quality must be 0–100");
    exit(1);
  }
  params.tcp_numlayers = 1;
  params.tcp_rates[0] = 100.0 - quality;
  println!("DEBUG: quality → tcp_rates[0] = {}", params.tcp_rates[0]);

  // --- Setup compression ---
  println!("DEBUG: calling setup_encoder");
  if codec.setup_encoder(&mut params, &mut jp2_image) == 0 {
    eprintln!("ERROR: setup_encoder failed");
    exit(1);
  }
  println!("DEBUG: setup_encoder succeeded");

  // --- Start compress ---
  println!("DEBUG: calling start_compress");
  if codec.start_compress(&mut jp2_image, &mut stream) == 0 {
    eprintln!("ERROR: start_compress failed");
    exit(1);
  }
  println!("DEBUG: start_compress succeeded");

  // --- Write data ---
  println!("DEBUG: calling encode");
  if codec.encode(&mut stream) == 0 {
    eprintln!("ERROR: encode failed");
    exit(1);
  }
  println!("DEBUG: encode succeeded");

  // --- Finish up ---
  println!("DEBUG: calling end_compress");
  if codec.end_compress(&mut stream) == 0 {
    eprintln!("ERROR: end_compress failed");
    exit(1);
  }
  println!("DEBUG: end_compress succeeded");

  // --- Flush to disk ---
  stream.flush().unwrap_or_else(|e| {
    eprintln!("ERROR: flush failed: {}", e);
    exit(1);
  });
  println!("DEBUG: flushed");

  println!("SUCCESS: wrote '{}'", output);
}
