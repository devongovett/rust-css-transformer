use atty::Stream;
use clap::{ArgGroup, Parser};
use lightningcss::bundler::{Bundler, FileProvider};
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::targets::Browsers;
use parcel_sourcemap::SourceMap;
use serde::Serialize;
use std::borrow::Cow;
use std::sync::{Arc, RwLock};
use std::{ffi, fs, io, path::Path};

#[cfg(target_os = "macos")]
#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(group(
  ArgGroup::new("targets-resolution")
      .args(&["targets", "browserslist"]),
))]
struct CliArgs {
  /// Target CSS file (default: stdin)
  #[clap(value_parser)]
  input_file: Vec<String>,
  /// Destination file for the output
  #[clap(short, long, group = "output_file", value_parser)]
  output_file: Option<String>,
  /// Destination directory to output into.
  #[clap(short = 'd', long, group = "output_file", value_parser)]
  output_dir: Option<String>,
  /// Minify the output
  #[clap(short, long, value_parser)]
  minify: bool,
  /// Enable parsing CSS nesting
  #[clap(long, value_parser)]
  nesting: bool,
  /// Enable parsing custom media queries
  #[clap(long, value_parser)]
  custom_media: bool,
  /// Enable parsing @scope
  #[clap(long, value_parser)]
  scope: bool,
  /// Enable CSS modules in output.
  /// If no filename is provided, <output_file>.json will be used.
  /// If no --output-file is specified, code and exports will be printed to stdout as JSON.
  #[clap(long, group = "css_modules", value_parser)]
  css_modules: Option<Option<String>>,
  #[clap(long, requires = "css_modules", value_parser)]
  css_modules_pattern: Option<String>,
  #[clap(long, requires = "css_modules", value_parser)]
  css_modules_dashed_idents: bool,
  /// Enable sourcemap, at <output_file>.map
  #[clap(long, requires = "output_file", value_parser)]
  sourcemap: bool,
  #[clap(long, value_parser)]
  bundle: bool,
  #[clap(short, long, value_parser)]
  targets: Vec<String>,
  #[clap(long, value_parser)]
  browserslist: bool,
  #[clap(long, value_parser)]
  error_recovery: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceMapJson<'a> {
  version: u8,
  mappings: String,
  sources: &'a Vec<String>,
  sources_content: &'a Vec<String>,
  names: &'a Vec<String>,
}

pub fn main() -> Result<(), std::io::Error> {
  let cli_args = CliArgs::parse();
  let project_root = std::env::current_dir()?;

  // If we're given an input file, read from it and adjust its name.
  //
  // If we're not given an input file and stdin was redirected, read
  // from it and create a fake name. Return an error if stdin was not
  // redirected (otherwise the program will hang waiting for input).
  //
  let inputs = if !cli_args.input_file.is_empty() {
    if cli_args.input_file.len() > 1 && cli_args.output_file.is_some() {
      eprintln!("Cannot use the --output-file option with multiple inputs. Use --output-dir instead.");
      std::process::exit(1);
    }

    if cli_args.input_file.len() > 1 && cli_args.output_file.is_none() && cli_args.output_dir.is_none() {
      eprintln!("Cannot output to stdout with multiple inputs. Use --output-dir instead.");
      std::process::exit(1);
    }

    cli_args
      .input_file
      .into_iter()
      .map(|ref f| -> Result<_, std::io::Error> {
        let absolute_path = fs::canonicalize(f)?;
        let filename = pathdiff::diff_paths(absolute_path, &project_root).unwrap();
        let filename = filename.to_string_lossy().into_owned();
        let contents = fs::read_to_string(f)?;
        Ok((filename, contents))
      })
      .collect::<Result<_, _>>()?
  } else {
    // Don't silently wait for input if stdin was not redirected.
    if atty::is(Stream::Stdin) {
      return Err(io::Error::new(
        io::ErrorKind::Other,
        "Not reading from stdin as it was not redirected",
      ));
    }
    let filename = format!("stdin-{}", std::process::id());
    let contents = io::read_to_string(io::stdin())?;
    vec![(filename, contents)]
  };

  let css_modules = if let Some(_) = cli_args.css_modules {
    let pattern = if let Some(pattern) = cli_args.css_modules_pattern.as_ref() {
      match lightningcss::css_modules::Pattern::parse(pattern) {
        Ok(p) => p,
        Err(e) => {
          eprintln!("{}", e);
          std::process::exit(1);
        }
      }
    } else {
      Default::default()
    };

    Some(lightningcss::css_modules::Config {
      pattern,
      dashed_idents: cli_args.css_modules_dashed_idents,
      ..Default::default()
    })
  } else {
    cli_args.css_modules.as_ref().map(|_| Default::default())
  };

  let fs = FileProvider::new();

  for (filename, source) in inputs {
    let warnings = if cli_args.error_recovery {
      Some(Arc::new(RwLock::new(Vec::new())))
    } else {
      None
    };

    let mut source_map = if cli_args.sourcemap {
      Some(SourceMap::new(&project_root.to_string_lossy()))
    } else {
      None
    };

    let output_file = if let Some(output_file) = &cli_args.output_file {
      Some(Cow::Borrowed(Path::new(output_file)))
    } else if let Some(dir) = &cli_args.output_dir {
      Some(Cow::Owned(
        Path::new(dir).join(Path::new(&filename).file_name().unwrap()),
      ))
    } else {
      None
    };

    let res = {
      let mut options = ParserOptions {
        nesting: cli_args.nesting,
        css_modules: css_modules.clone(),
        custom_media: cli_args.custom_media,
        error_recovery: cli_args.error_recovery,
        warnings: warnings.clone(),
        ..ParserOptions::default()
      };

      let mut stylesheet = if cli_args.bundle {
        let mut bundler = Bundler::new(&fs, source_map.as_mut(), options);
        bundler.bundle(Path::new(&filename)).unwrap()
      } else {
        if let Some(sm) = &mut source_map {
          sm.add_source(&filename);
          let _ = sm.set_source_content(0, &source);
        }
        options.filename = filename;
        StyleSheet::parse(&source, options).unwrap()
      };

      let targets = if !cli_args.targets.is_empty() {
        Browsers::from_browserslist(&cli_args.targets).unwrap()
      } else if cli_args.browserslist {
        Browsers::load_browserslist().unwrap()
      } else {
        None
      };

      stylesheet
        .minify(MinifyOptions {
          targets,
          ..MinifyOptions::default()
        })
        .unwrap();

      stylesheet
        .to_css(PrinterOptions {
          minify: cli_args.minify,
          source_map: source_map.as_mut(),
          project_root: Some(&project_root.to_string_lossy()),
          targets,
          ..PrinterOptions::default()
        })
        .unwrap()
    };

    let map = if let Some(ref mut source_map) = source_map {
      let mut vlq_output: Vec<u8> = Vec::new();
      source_map
        .write_vlq(&mut vlq_output)
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "Error writing sourcemap vlq"))?;

      let sm = SourceMapJson {
        version: 3,
        mappings: unsafe { String::from_utf8_unchecked(vlq_output) },
        sources: source_map.get_sources(),
        sources_content: source_map.get_sources_content(),
        names: source_map.get_names(),
      };

      serde_json::to_vec(&sm).ok()
    } else {
      None
    };

    if let Some(warnings) = warnings {
      let warnings = Arc::try_unwrap(warnings).unwrap().into_inner().unwrap();
      for warning in warnings {
        eprintln!("{}", warning);
      }
    }

    if let Some(output_file) = &output_file {
      let mut code = res.code;
      if cli_args.sourcemap {
        if let Some(map_buf) = map {
          let map_filename = output_file.to_string_lossy() + ".map";
          code += &format!("\n/*# sourceMappingURL={} */\n", map_filename);
          fs::write(map_filename.as_ref(), map_buf)?;
        }
      }

      if let Some(p) = output_file.parent() {
        fs::create_dir_all(p)?
      };
      fs::write(output_file, code.as_bytes())?;

      if let Some(css_modules) = &cli_args.css_modules {
        let css_modules_filename = if let Some(name) = css_modules {
          Cow::Borrowed(name)
        } else {
          Cow::Owned(infer_css_modules_filename(output_file.as_ref())?)
        };
        if let Some(exports) = res.exports {
          let css_modules_json = serde_json::to_string(&exports)?;
          fs::write(css_modules_filename.as_ref(), css_modules_json)?;
        }
      }
    } else {
      if let Some(exports) = res.exports {
        println!(
          "{}",
          serde_json::json!({
            "code": res.code,
            "exports": exports
          })
        );
      } else {
        println!("{}", res.code);
      }
    }
  }

  Ok(())
}

fn infer_css_modules_filename(path: &Path) -> Result<String, std::io::Error> {
  if path.extension() == Some(ffi::OsStr::new("json")) {
    Err(io::Error::new(
      io::ErrorKind::Other,
      "Cannot infer a css modules json filename, since the output file extension is '.json'",
    ))
  } else {
    // unwrap: the filename option is a String from clap, so is valid utf-8
    Ok(path.with_extension("json").to_str().unwrap().into())
  }
}
