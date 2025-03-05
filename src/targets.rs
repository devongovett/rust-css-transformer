//! Browser target options.

#![allow(missing_docs)]

use crate::vendor_prefix::VendorPrefix;
use bitflags::bitflags;
#[cfg(any(feature = "serde", feature = "nodejs"))]
use serde::{Deserialize, Serialize};

/// Browser versions to compile CSS for.
///
/// Versions are represented as a single 24-bit integer, with one byte
/// per `major.minor.patch` component.
///
/// # Example
///
/// This example represents a target of Safari 13.2.0.
///
/// ```
/// use lightningcss::targets::Browsers;
///
/// let targets = Browsers {
///   safari: Some((13 << 16) | (2 << 8)),
///   ..Browsers::default()
/// };
/// ```
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(any(feature = "serde", feature = "nodejs"), derive(Serialize, Deserialize))]
#[allow(missing_docs)]
pub struct Browsers {
  pub android: Option<u32>,
  pub chrome: Option<u32>,
  pub edge: Option<u32>,
  pub firefox: Option<u32>,
  pub ie: Option<u32>,
  pub ios_saf: Option<u32>,
  pub opera: Option<u32>,
  pub safari: Option<u32>,
  pub samsung: Option<u32>,
}

#[cfg(feature = "browserslist")]
#[cfg_attr(docsrs, doc(cfg(feature = "browserslist")))]
impl Browsers {
  /// Parses a list of browserslist queries into Lightning CSS targets.
  pub fn from_browserslist<S: AsRef<str>, I: IntoIterator<Item = S>>(
    query: I,
  ) -> Result<Option<Browsers>, browserslist::Error> {
    use browserslist::{resolve, Opts};

    Self::from_distribs(resolve(query, &Opts::default())?)
  }

  #[cfg(not(target_arch = "wasm32"))]
  /// Finds browserslist configuration, selects queries by environment and loads the resulting queries into LightningCSS targets.
  ///
  /// Configuration resolution is modeled after the original `browserslist` nodeJS package.
  /// The configuration is resolved in the following order:
  ///
  /// - If a `BROWSERSLIST` environment variable is present, then load targets from its value. This is analog to the `--targets` CLI option.
  ///   Example: `BROWSERSLIST="firefox ESR" lightningcss [OPTIONS] <INPUT_FILE>`
  /// - If a `BROWSERSLIST_CONFIG` environment variable is present, then resolve the file at the provided path.
  ///   Then parse and use targets from `package.json` or any browserslist configuration file pointed to by the environment variable.
  ///   Example: `BROWSERSLIST_CONFIG="../config/browserslist" lightningcss [OPTIONS] <INPUT_FILE>`
  /// - If none of the above apply, then find, parse and use targets from the first `browserslist`, `.browserslistrc`
  ///   or `package.json` configuration file in any parent directory.
  ///
  /// When using parsed configuration from `browserslist`, `.browserslistrc` or `package.json` configuration files,
  /// the environment determined by:
  ///
  /// - the `BROWSERSLIST_ENV` environment variable if present,
  /// - otherwise the `NODE_ENV` environment varialbe if present,
  /// - otherwise `production` is used.
  ///
  /// If no targets are found for the resulting environment, then the `defaults` configuration section is used.
  pub fn load_browserslist() -> Result<Option<Browsers>, browserslist::Error> {
    use browserslist::{execute, Opts};

    Self::from_distribs(execute(&Opts::default())?)
  }

  fn from_distribs(distribs: Vec<browserslist::Distrib>) -> Result<Option<Browsers>, browserslist::Error> {
    let mut browsers = Browsers::default();
    let mut has_any = false;
    for distrib in distribs {
      macro_rules! browser {
        ($browser: ident) => {{
          if let Some(v) = parse_version(distrib.version()) {
            if browsers.$browser.is_none() || v < browsers.$browser.unwrap() {
              browsers.$browser = Some(v);
              has_any = true;
            }
          }
        }};
      }

      match distrib.name() {
        "android" => browser!(android),
        "chrome" | "and_chr" => browser!(chrome),
        "edge" => browser!(edge),
        "firefox" | "and_ff" => browser!(firefox),
        "ie" => browser!(ie),
        "ios_saf" => browser!(ios_saf),
        "opera" | "op_mob" => browser!(opera),
        "safari" => browser!(safari),
        "samsung" => browser!(samsung),
        _ => {}
      }
    }

    if !has_any {
      return Ok(None);
    }

    Ok(Some(browsers))
  }
}

#[cfg(feature = "browserslist")]
fn parse_version(version: &str) -> Option<u32> {
  let version = version.split('-').next();
  if version.is_none() {
    return None;
  }

  let mut version = version.unwrap().split('.');
  let major = version.next().and_then(|v| v.parse::<u32>().ok());
  if let Some(major) = major {
    let minor = version.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let patch = version.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let v: u32 = (major & 0xff) << 16 | (minor & 0xff) << 8 | (patch & 0xff);
    return Some(v);
  }

  None
}

bitflags! {
  /// Features to explicitly enable or disable.
  #[derive(Debug, Default, Clone, Copy)]
  pub struct Features: u32 {
    const Nesting = 1 << 0;
    const NotSelectorList = 1 << 1;
    const DirSelector = 1 << 2;
    const LangSelectorList = 1 << 3;
    const IsSelector = 1 << 4;
    const TextDecorationThicknessPercent = 1 << 5;
    const MediaIntervalSyntax = 1 << 6;
    const MediaRangeSyntax = 1 << 7;
    const CustomMediaQueries = 1 << 8;
    const ClampFunction = 1 << 9;
    const ColorFunction = 1 << 10;
    const OklabColors = 1 << 11;
    const LabColors = 1 << 12;
    const P3Colors = 1 << 13;
    const HexAlphaColors = 1 << 14;
    const SpaceSeparatedColorNotation = 1 << 15;
    const FontFamilySystemUi = 1 << 16;
    const DoublePositionGradients = 1 << 17;
    const VendorPrefixes = 1 << 18;
    const LogicalProperties = 1 << 19;
    const LightDark = 1 << 20;
    const Selectors = Self::Nesting.bits() | Self::NotSelectorList.bits() | Self::DirSelector.bits() | Self::LangSelectorList.bits() | Self::IsSelector.bits();
    const MediaQueries = Self::MediaIntervalSyntax.bits() | Self::MediaRangeSyntax.bits() | Self::CustomMediaQueries.bits();
    const Colors = Self::ColorFunction.bits() | Self::OklabColors.bits() | Self::LabColors.bits() | Self::P3Colors.bits() | Self::HexAlphaColors.bits() | Self::SpaceSeparatedColorNotation.bits() | Self::LightDark.bits();
  }
}

/// Target browsers and features to compile.
#[derive(Debug, Clone, Copy, Default)]
pub struct Targets {
  /// Browser targets to compile the CSS for.
  pub browsers: Option<Browsers>,
  /// Features that should always be compiled, even when supported by targets.
  pub include: Features,
  /// Features that should never be compiled, even when unsupported by targets.
  pub exclude: Features,
}

impl From<Browsers> for Targets {
  fn from(browsers: Browsers) -> Self {
    Self {
      browsers: Some(browsers),
      ..Default::default()
    }
  }
}

impl From<Option<Browsers>> for Targets {
  fn from(browsers: Option<Browsers>) -> Self {
    Self {
      browsers,
      ..Default::default()
    }
  }
}

impl Targets {
  pub(crate) fn is_compatible(&self, feature: crate::compat::Feature) -> bool {
    self.browsers.map(|targets| feature.is_compatible(targets)).unwrap_or(true)
  }

  pub(crate) fn should_compile(&self, feature: crate::compat::Feature, flag: Features) -> bool {
    self.include.contains(flag) || (!self.exclude.contains(flag) && !self.is_compatible(feature))
  }

  pub(crate) fn should_compile_logical(&self, feature: crate::compat::Feature) -> bool {
    self.should_compile(feature, Features::LogicalProperties)
  }

  pub(crate) fn should_compile_selectors(&self) -> bool {
    self.include.intersects(Features::Selectors)
      || (!self.exclude.intersects(Features::Selectors) && self.browsers.is_some())
  }

  pub(crate) fn prefixes(&self, prefix: VendorPrefix, feature: crate::prefixes::Feature) -> VendorPrefix {
    if prefix.contains(VendorPrefix::None) && !self.exclude.contains(Features::VendorPrefixes) {
      if self.include.contains(Features::VendorPrefixes) {
        VendorPrefix::all()
      } else {
        self.browsers.map(|browsers| feature.prefixes_for(browsers)).unwrap_or(prefix)
      }
    } else {
      prefix
    }
  }
}

macro_rules! should_compile {
  ($targets: expr, $feature: ident) => {
    $targets.should_compile(
      crate::compat::Feature::$feature,
      crate::targets::Features::$feature,
    )
  };
}

pub(crate) use should_compile;
