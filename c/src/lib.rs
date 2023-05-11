use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::mem::ManuallyDrop;
use std::os::raw::c_char;
use std::sync::{Arc, RwLock};

use lightningcss::css_modules::PatternParseError;
use lightningcss::error::{Error, MinifyErrorKind, ParserError, PrinterError};
use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
use lightningcss::targets::Browsers;
use parcel_sourcemap::SourceMap;

pub struct StyleSheetWrapper<'i, 'o> {
  stylesheet: StyleSheet<'i, 'o>,
  source: &'i str,
  warnings: Vec<CssError<'i>>,
}

pub struct CssError<'i> {
  kind: ErrorKind<'i>,
  message: Option<CString>,
}

impl<'i> CssError<'i> {
  fn message(&mut self) -> *const c_char {
    if let Some(message) = &self.message {
      return message.as_ptr();
    }

    let string: String = match &self.kind {
      ErrorKind::ParserError(err) => err.to_string().into(),
      ErrorKind::MinifyError(err) => err.to_string().into(),
      ErrorKind::PrinterError(err) => err.to_string().into(),
      ErrorKind::PatternParseError(err) => err.to_string().into(),
      ErrorKind::BrowserslistError(err) => err.to_string().into(),
      ErrorKind::SourceMapError(err) => err.to_string().into(),
    };

    self.message = Some(CString::new(string).unwrap());
    self.message.as_ref().unwrap().as_ptr()
  }
}

pub enum ErrorKind<'i> {
  ParserError(Error<ParserError<'i>>),
  MinifyError(Error<MinifyErrorKind>),
  PrinterError(PrinterError),
  PatternParseError(PatternParseError),
  BrowserslistError(browserslist::Error),
  SourceMapError(parcel_sourcemap::SourceMapError),
}

macro_rules! impl_from {
  ($name: ident, $t: ty) => {
    impl<'i> From<$t> for CssError<'i> {
      fn from(err: $t) -> Self {
        CssError {
          kind: ErrorKind::$name(err),
          message: None,
        }
      }
    }
  };
}

impl_from!(ParserError, Error<ParserError<'i>>);
impl_from!(MinifyError, Error<MinifyErrorKind>);
impl_from!(PrinterError, PrinterError);
impl_from!(PatternParseError, PatternParseError);
impl_from!(BrowserslistError, browserslist::Error);
impl_from!(SourceMapError, parcel_sourcemap::SourceMapError);

#[repr(C)]
pub struct ParseOptions {
  filename: *const c_char,
  nesting: bool,
  custom_media: bool,
  scope: bool,
  css_modules: bool,
  css_modules_pattern: *const c_char,
  css_modules_dashed_idents: bool,
  error_recovery: bool,
}

#[repr(C)]
#[derive(Default, PartialEq)]
pub struct Targets {
  android: u32,
  chrome: u32,
  edge: u32,
  firefox: u32,
  ie: u32,
  ios_saf: u32,
  opera: u32,
  safari: u32,
  samsung: u32,
}

impl Into<Browsers> for Targets {
  fn into(self) -> Browsers {
    macro_rules! browser {
      ($val: expr) => {
        if $val > 0 {
          Some($val)
        } else {
          None
        }
      };
    }

    Browsers {
      android: browser!(self.android),
      chrome: browser!(self.chrome),
      edge: browser!(self.edge),
      firefox: browser!(self.firefox),
      ie: browser!(self.ie),
      ios_saf: browser!(self.ios_saf),
      opera: browser!(self.opera),
      safari: browser!(self.safari),
      samsung: browser!(self.samsung),
    }
  }
}

macro_rules! unwrap {
  ($result: expr, $error: ident, $ret: expr) => {
    match $result {
      Ok(v) => v,
      Err(err) => unsafe {
        *$error = Box::into_raw(Box::new(err.into()));
        return $ret;
      },
    }
  };
}

#[no_mangle]
pub extern "C" fn lightningcss_browserslist_to_targets(
  query: *const c_char,
  targets: *mut Targets,
  error: *mut *mut CssError,
) -> bool {
  let string = unsafe { std::str::from_utf8_unchecked(CStr::from_ptr(query).to_bytes()) };
  match Browsers::from_browserslist([string]) {
    Ok(Some(browsers)) => {
      let targets = unsafe { &mut *targets };
      targets.android = browsers.android.unwrap_or_default();
      targets.chrome = browsers.chrome.unwrap_or_default();
      targets.edge = browsers.edge.unwrap_or_default();
      targets.firefox = browsers.firefox.unwrap_or_default();
      targets.ie = browsers.ie.unwrap_or_default();
      targets.ios_saf = browsers.ios_saf.unwrap_or_default();
      targets.opera = browsers.opera.unwrap_or_default();
      targets.safari = browsers.safari.unwrap_or_default();
      targets.samsung = browsers.samsung.unwrap_or_default();
      true
    }
    Ok(None) => true,
    Err(err) => unsafe {
      *error = Box::into_raw(Box::new(err.into()));
      false
    },
  }
}

#[repr(C)]
pub struct TransformOptions {
  targets: Targets,
  unused_symbols: *mut *mut c_char,
  unused_symbols_len: usize,
}

impl Into<MinifyOptions> for TransformOptions {
  fn into(self) -> MinifyOptions {
    let mut unused_symbols = HashSet::new();
    let slice = unsafe { std::slice::from_raw_parts(self.unused_symbols, self.unused_symbols_len) };
    for symbol in slice {
      let string = unsafe { std::str::from_utf8_unchecked(CStr::from_ptr(*symbol).to_bytes()).to_owned() };
      unused_symbols.insert(string);
    }

    MinifyOptions {
      targets: if self.targets != Targets::default() {
        Some(self.targets.into())
      } else {
        None
      },
      unused_symbols,
    }
  }
}

#[repr(C)]
pub struct ToCssOptions {
  minify: bool,
  source_map: bool,
  input_source_map: *const c_char,
  input_source_map_len: usize,
  project_root: *const c_char,
  targets: Targets,
  analyze_dependencies: bool,
  pseudo_classes: PseudoClasses,
}

#[derive(PartialEq)]
#[repr(C)]
pub struct PseudoClasses {
  hover: *const c_char,
  active: *const c_char,
  focus: *const c_char,
  focus_visible: *const c_char,
  focus_within: *const c_char,
}

impl Default for PseudoClasses {
  fn default() -> Self {
    PseudoClasses {
      hover: std::ptr::null(),
      active: std::ptr::null(),
      focus: std::ptr::null(),
      focus_visible: std::ptr::null(),
      focus_within: std::ptr::null(),
    }
  }
}

impl<'a> Into<lightningcss::printer::PseudoClasses<'a>> for PseudoClasses {
  fn into(self) -> lightningcss::printer::PseudoClasses<'a> {
    macro_rules! pc {
      ($ptr: expr) => {
        if $ptr.is_null() {
          None
        } else {
          Some(unsafe { std::str::from_utf8_unchecked(CStr::from_ptr($ptr).to_bytes()) })
        }
      };
    }

    lightningcss::printer::PseudoClasses {
      hover: pc!(self.hover),
      active: pc!(self.active),
      focus: pc!(self.focus),
      focus_visible: pc!(self.focus_visible),
      focus_within: pc!(self.focus_within),
    }
  }
}

#[no_mangle]
pub extern "C" fn lightningcss_stylesheet_parse(
  source: *const c_char,
  len: usize,
  options: ParseOptions,
  error: *mut *mut CssError,
) -> *mut StyleSheetWrapper {
  let slice = unsafe { std::slice::from_raw_parts(source as *const u8, len) };
  let code = unsafe { std::str::from_utf8_unchecked(slice) };
  let warnings = Arc::new(RwLock::new(Vec::new()));
  let opts = ParserOptions {
    filename: if options.filename.is_null() {
      String::new()
    } else {
      unsafe { std::str::from_utf8_unchecked(CStr::from_ptr(options.filename).to_bytes()).to_owned() }
    },
    nesting: options.nesting,
    custom_media: options.custom_media,
    scope: options.scope,
    css_modules: if options.css_modules {
      let pattern = if !options.css_modules_pattern.is_null() {
        let pattern =
          unsafe { std::str::from_utf8_unchecked(CStr::from_ptr(options.css_modules_pattern).to_bytes()) };
        unwrap!(
          lightningcss::css_modules::Pattern::parse(pattern),
          error,
          std::ptr::null_mut()
        )
      } else {
        lightningcss::css_modules::Pattern::default()
      };
      Some(lightningcss::css_modules::Config {
        pattern,
        dashed_idents: options.css_modules_dashed_idents,
      })
    } else {
      None
    },
    error_recovery: options.error_recovery,
    source_index: 0,
    warnings: Some(warnings.clone()),
  };

  let stylesheet = unwrap!(StyleSheet::parse(code, opts), error, std::ptr::null_mut());
  Box::into_raw(Box::new(StyleSheetWrapper {
    stylesheet,
    source: code,
    warnings: warnings.clone().read().unwrap().iter().map(|w| w.clone().into()).collect(),
  }))
}

#[no_mangle]
pub extern "C" fn lightningcss_stylesheet_transform(
  stylesheet: *mut StyleSheetWrapper,
  options: TransformOptions,
  error: *mut *mut CssError,
) -> bool {
  let wrapper = unsafe { stylesheet.as_mut() }.unwrap();
  unwrap!(wrapper.stylesheet.minify(options.into()), error, false);
  true
}

#[no_mangle]
pub extern "C" fn lightningcss_stylesheet_to_css(
  stylesheet: *mut StyleSheetWrapper,
  options: ToCssOptions,
  error: *mut *mut CssError,
) -> ToCssResult {
  let wrapper = unsafe { stylesheet.as_mut() }.unwrap();
  let mut source_map = if options.source_map {
    let mut sm = SourceMap::new("/");
    sm.add_source(&wrapper.stylesheet.sources[0]);
    unwrap!(sm.set_source_content(0, wrapper.source), error, ToCssResult::default());
    Some(sm)
  } else {
    None
  };

  let opts = PrinterOptions {
    minify: options.minify,
    project_root: if options.project_root.is_null() {
      None
    } else {
      Some(unsafe { std::str::from_utf8_unchecked(CStr::from_ptr(options.project_root).to_bytes()) })
    },
    source_map: source_map.as_mut(),
    targets: if options.targets != Targets::default() {
      Some(options.targets.into())
    } else {
      None
    },
    analyze_dependencies: if options.analyze_dependencies {
      Some(Default::default())
    } else {
      None
    },
    pseudo_classes: if options.pseudo_classes != PseudoClasses::default() {
      Some(options.pseudo_classes.into())
    } else {
      None
    },
  };

  let res = unwrap!(wrapper.stylesheet.to_css(opts), error, ToCssResult::default());

  let map = if let Some(mut source_map) = source_map {
    if !options.input_source_map.is_null() {
      let slice =
        unsafe { std::slice::from_raw_parts(options.input_source_map as *const u8, options.input_source_map_len) };
      let input_source_map = unsafe { std::str::from_utf8_unchecked(slice) };
      let mut sm = unwrap!(
        SourceMap::from_json("/", input_source_map),
        error,
        ToCssResult::default()
      );
      unwrap!(source_map.extends(&mut sm), error, ToCssResult::default());
    }

    unwrap!(source_map.to_json(None), error, ToCssResult::default()).into()
  } else {
    RawString::default()
  };

  let (exports, exports_len) = if let Some(exports) = res.exports {
    let exports: Vec<CssModuleExport> = exports
      .into_iter()
      .map(|(k, v)| {
        let composes_len = v.composes.len();
        let composes = if !v.composes.is_empty() {
          let composes: Vec<CssModuleReference> = v.composes.into_iter().map(|composes| composes.into()).collect();
          ManuallyDrop::new(composes).as_mut_ptr()
        } else {
          std::ptr::null_mut()
        };

        CssModuleExport {
          exported: k.into(),
          local: v.name.into(),
          is_referenced: v.is_referenced,
          composes,
          composes_len,
        }
      })
      .collect();
    let mut exports = ManuallyDrop::new(exports);
    (exports.as_mut_ptr(), exports.len())
  } else {
    (std::ptr::null_mut(), 0)
  };

  let (references, references_len) = if let Some(references) = res.references {
    let references: Vec<CssModulePlaceholder> = references
      .into_iter()
      .map(|(k, v)| CssModulePlaceholder {
        placeholder: k.into(),
        reference: v.into(),
      })
      .collect();
    let mut references = ManuallyDrop::new(references);
    (references.as_mut_ptr(), references.len())
  } else {
    (std::ptr::null_mut(), 0)
  };

  ToCssResult {
    code: res.code.into(),
    map,
    exports,
    exports_len,
    references,
    references_len,
  }
}

#[no_mangle]
pub extern "C" fn lightningcss_stylesheet_free(stylesheet: *mut StyleSheetWrapper) {
  if !stylesheet.is_null() {
    drop(unsafe { Box::from_raw(stylesheet) })
  }
}

#[repr(C)]
pub struct ToCssResult {
  code: RawString,
  map: RawString,
  exports: *mut CssModuleExport,
  exports_len: usize,
  references: *mut CssModulePlaceholder,
  references_len: usize,
}

impl Default for ToCssResult {
  fn default() -> Self {
    ToCssResult {
      code: RawString::default(),
      map: RawString::default(),
      exports: std::ptr::null_mut(),
      exports_len: 0,
      references: std::ptr::null_mut(),
      references_len: 0,
    }
  }
}

impl Drop for ToCssResult {
  fn drop(&mut self) {
    if !self.exports.is_null() {
      let exports = unsafe { Vec::from_raw_parts(self.exports, self.exports_len, self.exports_len) };
      drop(exports);
      self.exports = std::ptr::null_mut();
    }

    if !self.references.is_null() {
      let references = unsafe { Vec::from_raw_parts(self.references, self.references_len, self.references_len) };
      drop(references);
      self.references = std::ptr::null_mut();
    }
  }
}

#[no_mangle]
pub extern "C" fn lightningcss_to_css_result_free(result: ToCssResult) {
  drop(result)
}

#[repr(C)]
pub struct CssModuleExport {
  exported: RawString,
  local: RawString,
  is_referenced: bool,
  composes: *mut CssModuleReference,
  composes_len: usize,
}

impl Drop for CssModuleExport {
  fn drop(&mut self) {
    if !self.composes.is_null() {
      let composes = unsafe { Vec::from_raw_parts(self.composes, self.composes_len, self.composes_len) };
      drop(composes);
      self.composes = std::ptr::null_mut();
    }
  }
}

#[repr(C)]
pub enum CssModuleReference {
  /// A local reference.
  Local {
    /// The local (compiled) name for the reference.
    name: RawString,
  },
  /// A global reference.
  Global {
    /// The referenced global name.
    name: RawString,
  },
  /// A reference to an export in a different file.
  Dependency {
    /// The name to reference within the dependency.
    name: RawString,
    /// The dependency specifier for the referenced file.
    specifier: RawString,
  },
}

impl From<lightningcss::css_modules::CssModuleReference> for CssModuleReference {
  fn from(reference: lightningcss::css_modules::CssModuleReference) -> Self {
    use lightningcss::css_modules::CssModuleReference::*;
    match reference {
      Local { name } => CssModuleReference::Local { name: name.into() },
      Global { name } => CssModuleReference::Global { name: name.into() },
      Dependency { name, specifier } => CssModuleReference::Dependency {
        name: name.into(),
        specifier: specifier.into(),
      },
    }
  }
}

#[repr(C)]
pub struct CssModulePlaceholder {
  placeholder: RawString,
  reference: CssModuleReference,
}

#[repr(C)]
pub struct RawString {
  text: *mut c_char,
  len: usize,
}

impl Default for RawString {
  fn default() -> Self {
    RawString {
      text: std::ptr::null_mut(),
      len: 0,
    }
  }
}

impl From<String> for RawString {
  fn from(string: String) -> RawString {
    RawString {
      len: string.len(),
      text: Box::into_raw(string.into_boxed_str()) as *mut c_char,
    }
  }
}

impl Drop for RawString {
  fn drop(&mut self) {
    if self.text.is_null() {
      return;
    }
    drop(unsafe { Box::from_raw(self.text) });
    self.text = std::ptr::null_mut();
  }
}

#[no_mangle]
pub extern "C" fn lightningcss_error_message(error: *mut CssError) -> *const c_char {
  match unsafe { error.as_mut() } {
    Some(err) => err.message(),
    None => std::ptr::null(),
  }
}

#[no_mangle]
pub extern "C" fn lightningcss_error_free(error: *mut CssError) {
  if !error.is_null() {
    drop(unsafe { Box::from_raw(error) })
  }
}

#[no_mangle]
pub extern "C" fn lightningcss_stylesheet_get_warning_count<'i>(
  stylesheet: *mut StyleSheetWrapper<'i, '_>,
) -> usize {
  match unsafe { stylesheet.as_mut() } {
    Some(s) => s.warnings.len(),
    None => 0,
  }
}

#[no_mangle]
pub extern "C" fn lightningcss_stylesheet_get_warning<'i>(
  stylesheet: *mut StyleSheetWrapper<'i, '_>,
  index: usize,
) -> *const c_char {
  let stylesheet = match unsafe { stylesheet.as_mut() } {
    Some(s) => s,
    None => return std::ptr::null(),
  };

  match stylesheet.warnings.get_mut(index) {
    Some(w) => w.message(),
    None => std::ptr::null(),
  }
}
