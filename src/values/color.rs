//! CSS color values.

#![allow(non_upper_case_globals)]

use super::angle::Angle;
use super::calc::Calc;
use super::number::CSSNumber;
use super::percentage::Percentage;
use crate::compat::Feature;
use crate::error::{ParserError, PrinterError};
use crate::macros::enum_property;
use crate::printer::Printer;
use crate::properties::PropertyId;
use crate::rules::supports::SupportsCondition;
use crate::targets::{should_compile, Browsers, Targets};
use crate::traits::{FallbackValues, IsCompatible, Parse, ToCss};
#[cfg(feature = "visitor")]
use crate::visitor::{Visit, VisitTypes, Visitor};
use bitflags::bitflags;
use cssparser::color::{parse_hash_color, parse_named_color};
use cssparser::*;
use cssparser_color::{hsl_to_rgb, AngleOrNumber, ColorParser, NumberOrPercentage};
use std::any::TypeId;
use std::f32::consts::PI;
use std::fmt::Write;

/// A CSS [`<color>`](https://www.w3.org/TR/css-color-4/#color-type) value.
///
/// CSS supports many different color spaces to represent colors. The most common values
/// are stored as RGBA using a single byte per component. Less common values are stored
/// using a `Box` to reduce the amount of memory used per color.
///
/// Each color space is represented as a struct that implements the `From` and `Into` traits
/// for all other color spaces, so it is possible to convert between color spaces easily.
/// In addition, colors support [interpolation](#method.interpolate) as in the `color-mix()` function.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "visitor", visit(visit_color, COLORS))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(untagged, rename_all = "lowercase")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub enum CssColor {
  /// The [`currentColor`](https://www.w3.org/TR/css-color-4/#currentcolor-color) keyword.
  #[cfg_attr(feature = "serde", serde(with = "CurrentColor"))]
  CurrentColor,
  /// An value in the RGB color space, including values parsed as hex colors, or the `rgb()`, `hsl()`, and `hwb()` functions.
  #[cfg_attr(
    feature = "serde",
    serde(serialize_with = "serialize_rgba", deserialize_with = "deserialize_rgba")
  )]
  #[cfg_attr(feature = "jsonschema", schemars(with = "RGBColor"))]
  RGBA(RGBA),
  /// A value in a LAB color space, including the `lab()`, `lch()`, `oklab()`, and `oklch()` functions.
  LAB(Box<LABColor>),
  /// A value in a predefined color space, e.g. `display-p3`.
  Predefined(Box<PredefinedColor>),
  /// A floating point representation of an RGB, HSL, or HWB color when it contains `none` components.
  Float(Box<FloatColor>),
  /// The [`light-dark()`](https://drafts.csswg.org/css-color-5/#light-dark) function.
  #[cfg_attr(feature = "visitor", skip_type)]
  #[cfg_attr(feature = "serde", serde(with = "LightDark"))]
  LightDark(Box<CssColor>, Box<CssColor>),
  /// A [system color](https://drafts.csswg.org/css-color/#css-system-colors) keyword.
  System(SystemColor),
}

#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
enum CurrentColor {
  CurrentColor,
}

#[cfg(feature = "serde")]
impl CurrentColor {
  fn serialize<S>(serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    serde::Serialize::serialize(&CurrentColor::CurrentColor, serializer)
  }

  fn deserialize<'de, D>(deserializer: D) -> Result<(), D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    use serde::Deserialize;
    let _: CurrentColor = Deserialize::deserialize(deserializer)?;
    Ok(())
  }
}

// Convert RGBA to SRGB to serialize so we get a tagged struct.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
enum RGBColor {
  RGB(SRGB),
}

#[cfg(feature = "serde")]
fn serialize_rgba<S>(rgba: &RGBA, serializer: S) -> Result<S::Ok, S::Error>
where
  S: serde::Serializer,
{
  use serde::Serialize;
  RGBColor::RGB(rgba.into()).serialize(serializer)
}

#[cfg(feature = "serde")]
fn deserialize_rgba<'de, D>(deserializer: D) -> Result<RGBA, D::Error>
where
  D: serde::Deserializer<'de>,
{
  use serde::Deserialize;
  match RGBColor::deserialize(deserializer)? {
    RGBColor::RGB(srgb) => Ok(srgb.into()),
  }
}

// For AST serialization.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
enum LightDark {
  LightDark { light: CssColor, dark: CssColor },
}

#[cfg(feature = "serde")]
impl<'de> LightDark {
  pub fn serialize<S>(light: &Box<CssColor>, dark: &Box<CssColor>, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    let wrapper = LightDark::LightDark {
      light: (**light).clone(),
      dark: (**dark).clone(),
    };
    serde::Serialize::serialize(&wrapper, serializer)
  }

  pub fn deserialize<D>(deserializer: D) -> Result<(Box<CssColor>, Box<CssColor>), D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let v: LightDark = serde::Deserialize::deserialize(deserializer)?;
    match v {
      LightDark::LightDark { light, dark } => Ok((Box::new(light), Box::new(dark))),
    }
  }
}

/// A color in a LAB color space, including the `lab()`, `lch()`, `oklab()`, and `oklch()` functions.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", rename_all = "lowercase")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum LABColor {
  /// A `lab()` color.
  LAB(LAB),
  /// An `lch()` color.
  LCH(LCH),
  /// An `oklab()` color.
  OKLAB(OKLAB),
  /// An `oklch()` color.
  OKLCH(OKLCH),
}

/// A color in a predefined color space, e.g. `display-p3`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(tag = "type"))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum PredefinedColor {
  /// A color in the `srgb` color space.
  #[cfg_attr(feature = "serde", serde(rename = "srgb"))]
  SRGB(SRGB),
  /// A color in the `srgb-linear` color space.
  #[cfg_attr(feature = "serde", serde(rename = "srgb-linear"))]
  SRGBLinear(SRGBLinear),
  /// A color in the `display-p3` color space.
  #[cfg_attr(feature = "serde", serde(rename = "display-p3"))]
  DisplayP3(P3),
  /// A color in the `a98-rgb` color space.
  #[cfg_attr(feature = "serde", serde(rename = "a98-rgb"))]
  A98(A98),
  /// A color in the `prophoto-rgb` color space.
  #[cfg_attr(feature = "serde", serde(rename = "prophoto-rgb"))]
  ProPhoto(ProPhoto),
  /// A color in the `rec2020` color space.
  #[cfg_attr(feature = "serde", serde(rename = "rec2020"))]
  Rec2020(Rec2020),
  /// A color in the `xyz-d50` color space.
  #[cfg_attr(feature = "serde", serde(rename = "xyz-d50"))]
  XYZd50(XYZd50),
  /// A color in the `xyz-d65` color space.
  #[cfg_attr(feature = "serde", serde(rename = "xyz-d65"))]
  XYZd65(XYZd65),
}

/// A floating point representation of color types that
/// are usually stored as RGBA. These are used when there
/// are any `none` components, which are represented as NaN.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", rename_all = "lowercase")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum FloatColor {
  /// An RGB color.
  RGB(SRGB),
  /// An HSL color.
  HSL(HSL),
  /// An HWB color.
  HWB(HWB),
}

bitflags! {
  /// A color type that is used as a fallback when compiling colors for older browsers.
  #[derive(Debug, PartialEq, Eq, Clone, Copy)]
  pub struct ColorFallbackKind: u8 {
    /// An RGB color fallback.
    const RGB    = 0b01;
    /// A P3 color fallback.
    const P3     = 0b10;
    /// A LAB color fallback.
    const LAB    = 0b100;
    /// An OKLAB color fallback.
    const OKLAB  = 0b1000;
  }
}

enum_property! {
  /// A [color space](https://www.w3.org/TR/css-color-4/#interpolation-space) keyword
  /// used in interpolation functions such as `color-mix()`.
  enum ColorSpaceName {
    "srgb": SRGB,
    "srgb-linear": SRGBLinear,
    "lab": LAB,
    "oklab": OKLAB,
    "xyz": XYZ,
    "xyz-d50": XYZd50,
    "xyz-d65": XYZd65,
    "hsl": Hsl,
    "hwb": Hwb,
    "lch": LCH,
    "oklch": OKLCH,
  }
}

enum_property! {
  /// A hue [interpolation method](https://www.w3.org/TR/css-color-4/#typedef-hue-interpolation-method)
  /// used in interpolation functions such as `color-mix()`.
  pub enum HueInterpolationMethod {
    /// Angles are adjusted so that θ₂ - θ₁ ∈ [-180, 180].
    Shorter,
    /// Angles are adjusted so that θ₂ - θ₁ ∈ {0, [180, 360)}.
    Longer,
    /// Angles are adjusted so that θ₂ - θ₁ ∈ [0, 360).
    Increasing,
    /// Angles are adjusted so that θ₂ - θ₁ ∈ (-360, 0].
    Decreasing,
    /// No fixup is performed. Angles are interpolated in the same way as every other component.
    Specified,
  }
}

impl ColorFallbackKind {
  pub(crate) fn lowest(&self) -> ColorFallbackKind {
    // This finds the lowest set bit.
    *self & ColorFallbackKind::from_bits_truncate(self.bits().wrapping_neg())
  }

  pub(crate) fn highest(&self) -> ColorFallbackKind {
    // This finds the highest set bit.
    if self.is_empty() {
      return ColorFallbackKind::empty();
    }

    let zeros = 7 - self.bits().leading_zeros();
    ColorFallbackKind::from_bits_truncate(1 << zeros)
  }

  pub(crate) fn and_below(&self) -> ColorFallbackKind {
    if self.is_empty() {
      return ColorFallbackKind::empty();
    }

    *self | ColorFallbackKind::from_bits_truncate(self.bits() - 1)
  }

  pub(crate) const fn supports_condition_value(&self) -> &'static str {
    match *self {
      ColorFallbackKind::P3 => "color(display-p3 0 0 0)",
      ColorFallbackKind::LAB => "lab(0% 0 0)",
      _ => unreachable!(),
    }
  }

  pub(crate) fn supports_condition<'i>(&self) -> SupportsCondition<'i> {
    let s = self.supports_condition_value();
    SupportsCondition::Declaration {
      property_id: PropertyId::Color,
      value: s.into(),
    }
  }
}

impl CssColor {
  /// Returns the `currentColor` keyword.
  pub fn current_color() -> CssColor {
    CssColor::CurrentColor
  }

  /// Returns the `transparent` keyword.
  pub fn transparent() -> CssColor {
    CssColor::RGBA(RGBA::transparent())
  }

  /// Converts the color to RGBA.
  pub fn to_rgb(&self) -> Result<CssColor, ()> {
    match self {
      CssColor::LightDark(light, dark) => {
        Ok(CssColor::LightDark(Box::new(light.to_rgb()?), Box::new(dark.to_rgb()?)))
      }
      _ => Ok(RGBA::try_from(self)?.into()),
    }
  }

  /// Converts the color to the LAB color space.
  pub fn to_lab(&self) -> Result<CssColor, ()> {
    match self {
      CssColor::LightDark(light, dark) => {
        Ok(CssColor::LightDark(Box::new(light.to_lab()?), Box::new(dark.to_lab()?)))
      }
      _ => Ok(LAB::try_from(self)?.into()),
    }
  }

  /// Converts the color to the P3 color space.
  pub fn to_p3(&self) -> Result<CssColor, ()> {
    match self {
      CssColor::LightDark(light, dark) => {
        Ok(CssColor::LightDark(Box::new(light.to_p3()?), Box::new(dark.to_p3()?)))
      }
      _ => Ok(P3::try_from(self)?.into()),
    }
  }

  pub(crate) fn get_possible_fallbacks(&self, targets: Targets) -> ColorFallbackKind {
    // Fallbacks occur in levels: Oklab -> Lab -> P3 -> RGB. We start with all levels
    // below and including the authored color space, and remove the ones that aren't
    // compatible with our browser targets.
    let mut fallbacks = match self {
      CssColor::CurrentColor | CssColor::RGBA(_) | CssColor::Float(..) | CssColor::System(..) => {
        return ColorFallbackKind::empty()
      }
      CssColor::LAB(lab) => match &**lab {
        LABColor::LAB(..) | LABColor::LCH(..) if should_compile!(targets, LabColors) => {
          ColorFallbackKind::LAB.and_below()
        }
        LABColor::OKLAB(..) | LABColor::OKLCH(..) if should_compile!(targets, OklabColors) => {
          ColorFallbackKind::OKLAB.and_below()
        }
        _ => return ColorFallbackKind::empty(),
      },
      CssColor::Predefined(predefined) => match &**predefined {
        PredefinedColor::DisplayP3(..) if should_compile!(targets, P3Colors) => ColorFallbackKind::P3.and_below(),
        _ if should_compile!(targets, ColorFunction) => ColorFallbackKind::LAB.and_below(),
        _ => return ColorFallbackKind::empty(),
      },
      CssColor::LightDark(light, dark) => {
        return light.get_possible_fallbacks(targets) | dark.get_possible_fallbacks(targets);
      }
    };

    if fallbacks.contains(ColorFallbackKind::OKLAB) {
      if !should_compile!(targets, OklabColors) {
        fallbacks.remove(ColorFallbackKind::LAB.and_below());
      }
    }

    if fallbacks.contains(ColorFallbackKind::LAB) {
      if !should_compile!(targets, LabColors) {
        fallbacks.remove(ColorFallbackKind::P3.and_below());
      } else if targets
        .browsers
        .map(|targets| Feature::LabColors.is_partially_compatible(targets))
        .unwrap_or(false)
      {
        // We don't need P3 if Lab is supported by some of our targets.
        // No browser implements Lab but not P3.
        fallbacks.remove(ColorFallbackKind::P3);
      }
    }

    if fallbacks.contains(ColorFallbackKind::P3) {
      if !should_compile!(targets, P3Colors) {
        fallbacks.remove(ColorFallbackKind::RGB);
      } else if fallbacks.highest() != ColorFallbackKind::P3
        && !targets
          .browsers
          .map(|targets| Feature::P3Colors.is_partially_compatible(targets))
          .unwrap_or(false)
      {
        // Remove P3 if it isn't supported by any targets, and wasn't the
        // original authored color.
        fallbacks.remove(ColorFallbackKind::P3);
      }
    }

    fallbacks
  }

  /// Returns the color fallback types needed for the given browser targets.
  pub fn get_necessary_fallbacks(&self, targets: Targets) -> ColorFallbackKind {
    // Get the full set of possible fallbacks, and remove the highest one, which
    // will replace the original declaration. The remaining fallbacks need to be added.
    let fallbacks = self.get_possible_fallbacks(targets);
    fallbacks - fallbacks.highest()
  }

  /// Returns a fallback color for the given fallback type.
  pub fn get_fallback(&self, kind: ColorFallbackKind) -> CssColor {
    if matches!(self, CssColor::RGBA(_)) {
      return self.clone();
    }

    match kind {
      ColorFallbackKind::RGB => self.to_rgb().unwrap(),
      ColorFallbackKind::P3 => self.to_p3().unwrap(),
      ColorFallbackKind::LAB => self.to_lab().unwrap(),
      _ => unreachable!(),
    }
  }
}

impl IsCompatible for CssColor {
  fn is_compatible(&self, browsers: Browsers) -> bool {
    match self {
      CssColor::CurrentColor | CssColor::RGBA(_) | CssColor::Float(..) => true,
      CssColor::LAB(lab) => match &**lab {
        LABColor::LAB(..) | LABColor::LCH(..) => Feature::LabColors.is_compatible(browsers),
        LABColor::OKLAB(..) | LABColor::OKLCH(..) => Feature::OklabColors.is_compatible(browsers),
      },
      CssColor::Predefined(predefined) => match &**predefined {
        PredefinedColor::DisplayP3(..) => Feature::P3Colors.is_compatible(browsers),
        _ => Feature::ColorFunction.is_compatible(browsers),
      },
      CssColor::LightDark(light, dark) => {
        Feature::LightDark.is_compatible(browsers) && light.is_compatible(browsers) && dark.is_compatible(browsers)
      }
      CssColor::System(system) => system.is_compatible(browsers),
    }
  }
}

impl FallbackValues for CssColor {
  fn get_fallbacks(&mut self, targets: Targets) -> Vec<CssColor> {
    let fallbacks = self.get_necessary_fallbacks(targets);

    let mut res = Vec::new();
    if fallbacks.contains(ColorFallbackKind::RGB) {
      res.push(self.to_rgb().unwrap());
    }

    if fallbacks.contains(ColorFallbackKind::P3) {
      res.push(self.to_p3().unwrap());
    }

    if fallbacks.contains(ColorFallbackKind::LAB) {
      *self = self.to_lab().unwrap();
    }

    res
  }
}

impl Default for CssColor {
  fn default() -> CssColor {
    CssColor::transparent()
  }
}

impl<'i> Parse<'i> for CssColor {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let location = input.current_source_location();
    let token = input.next()?;
    match *token {
      Token::Hash(ref value) | Token::IDHash(ref value) => parse_hash_color(value.as_bytes())
        .map(|(r, g, b, a)| CssColor::RGBA(RGBA::new(r, g, b, a)))
        .map_err(|_| location.new_unexpected_token_error(token.clone())),
      Token::Ident(ref value) => Ok(match_ignore_ascii_case! { value,
        "currentcolor" => CssColor::CurrentColor,
        "transparent" => CssColor::RGBA(RGBA::transparent()),
        _ => {
          if let Ok((r, g, b)) = parse_named_color(value) {
            CssColor::RGBA(RGBA { red: r, green: g, blue: b, alpha: 255 })
          } else if let Ok(system_color) = SystemColor::parse_string(&value) {
            CssColor::System(system_color)
          } else {
            return Err(location.new_unexpected_token_error(token.clone()))
          }
        }
      }),
      Token::Function(ref name) => parse_color_function(location, name.clone(), input),
      _ => Err(location.new_unexpected_token_error(token.clone())),
    }
  }
}

impl ToCss for CssColor {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self {
      CssColor::CurrentColor => dest.write_str("currentColor"),
      CssColor::RGBA(color) => {
        if color.alpha == 255 {
          let hex: u32 = ((color.red as u32) << 16) | ((color.green as u32) << 8) | (color.blue as u32);
          if let Some(name) = short_color_name(hex) {
            return dest.write_str(name);
          }

          let compact = compact_hex(hex);
          if hex == expand_hex(compact) {
            write!(dest, "#{:03x}", compact)?;
          } else {
            write!(dest, "#{:06x}", hex)?;
          }
        } else {
          // If the #rrggbbaa syntax is not supported by the browser targets, output rgba()
          if should_compile!(dest.targets, HexAlphaColors) {
            // If the browser doesn't support `#rrggbbaa` color syntax, it is converted to `transparent` when compressed(minify = true).
            // https://www.w3.org/TR/css-color-4/#transparent-black
            if dest.minify && color.red == 0 && color.green == 0 && color.blue == 0 && color.alpha == 0 {
              return dest.write_str("transparent");
            } else {
              dest.write_str("rgba(")?;
              write!(dest, "{}", color.red)?;
              dest.delim(',', false)?;
              write!(dest, "{}", color.green)?;
              dest.delim(',', false)?;
              write!(dest, "{}", color.blue)?;
              dest.delim(',', false)?;

              // Try first with two decimal places, then with three.
              let mut rounded_alpha = (color.alpha_f32() * 100.0).round() / 100.0;
              let clamped = (rounded_alpha * 255.0).round().max(0.).min(255.0) as u8;
              if clamped != color.alpha {
                rounded_alpha = (color.alpha_f32() * 1000.).round() / 1000.;
              }

              rounded_alpha.to_css(dest)?;
              dest.write_char(')')?;
              return Ok(());
            }
          }

          let hex: u32 = ((color.red as u32) << 24)
            | ((color.green as u32) << 16)
            | ((color.blue as u32) << 8)
            | (color.alpha as u32);
          let compact = compact_hex(hex);
          if hex == expand_hex(compact) {
            write!(dest, "#{:04x}", compact)?;
          } else {
            write!(dest, "#{:08x}", hex)?;
          }
        }
        Ok(())
      }
      CssColor::LAB(lab) => match &**lab {
        LABColor::LAB(lab) => write_components("lab", lab.l, lab.a, lab.b, lab.alpha, dest),
        LABColor::LCH(lch) => write_components("lch", lch.l, lch.c, lch.h, lch.alpha, dest),
        LABColor::OKLAB(lab) => write_components("oklab", lab.l, lab.a, lab.b, lab.alpha, dest),
        LABColor::OKLCH(lch) => write_components("oklch", lch.l, lch.c, lch.h, lch.alpha, dest),
      },
      CssColor::Predefined(predefined) => write_predefined(predefined, dest),
      CssColor::Float(float) => {
        // Serialize as hex.
        let srgb = SRGB::from(**float);
        CssColor::from(srgb).to_css(dest)
      }
      CssColor::LightDark(light, dark) => {
        if should_compile!(dest.targets, LightDark) {
          dest.write_str("var(--lightningcss-light")?;
          dest.delim(',', false)?;
          light.to_css(dest)?;
          dest.write_char(')')?;
          dest.whitespace()?;
          dest.write_str("var(--lightningcss-dark")?;
          dest.delim(',', false)?;
          dark.to_css(dest)?;
          return dest.write_char(')');
        }

        dest.write_str("light-dark(")?;
        light.to_css(dest)?;
        dest.delim(',', false)?;
        dark.to_css(dest)?;
        dest.write_char(')')
      }
      CssColor::System(system) => system.to_css(dest),
    }
  }
}

// From esbuild: https://github.com/evanw/esbuild/blob/18e13bdfdca5cd3c7a2fae1a8bd739f8f891572c/internal/css_parser/css_decls_color.go#L218
// 0xAABBCCDD => 0xABCD
fn compact_hex(v: u32) -> u32 {
  return ((v & 0x0FF00000) >> 12) | ((v & 0x00000FF0) >> 4);
}

// 0xABCD => 0xAABBCCDD
fn expand_hex(v: u32) -> u32 {
  return ((v & 0xF000) << 16) | ((v & 0xFF00) << 12) | ((v & 0x0FF0) << 8) | ((v & 0x00FF) << 4) | (v & 0x000F);
}

fn short_color_name(v: u32) -> Option<&'static str> {
  // These names are shorter than their hex codes
  let s = match v {
    0x000080 => "navy",
    0x008000 => "green",
    0x008080 => "teal",
    0x4b0082 => "indigo",
    0x800000 => "maroon",
    0x800080 => "purple",
    0x808000 => "olive",
    0x808080 => "gray",
    0xa0522d => "sienna",
    0xa52a2a => "brown",
    0xc0c0c0 => "silver",
    0xcd853f => "peru",
    0xd2b48c => "tan",
    0xda70d6 => "orchid",
    0xdda0dd => "plum",
    0xee82ee => "violet",
    0xf0e68c => "khaki",
    0xf0ffff => "azure",
    0xf5deb3 => "wheat",
    0xf5f5dc => "beige",
    0xfa8072 => "salmon",
    0xfaf0e6 => "linen",
    0xff0000 => "red",
    0xff6347 => "tomato",
    0xff7f50 => "coral",
    0xffa500 => "orange",
    0xffc0cb => "pink",
    0xffd700 => "gold",
    0xffe4c4 => "bisque",
    0xfffafa => "snow",
    0xfffff0 => "ivory",
    _ => return None,
  };

  Some(s)
}

struct RelativeComponentParser {
  names: (&'static str, &'static str, &'static str),
  components: (f32, f32, f32, f32),
  types: (ChannelType, ChannelType, ChannelType),
}

impl RelativeComponentParser {
  fn new<T: ColorSpace>(color: &T) -> Self {
    Self {
      names: color.channels(),
      components: color.components(),
      types: color.types(),
    }
  }

  fn get_ident(&self, ident: &str, allowed_types: ChannelType) -> Option<f32> {
    if ident.eq_ignore_ascii_case(self.names.0) && allowed_types.intersects(self.types.0) {
      return Some(self.components.0);
    }

    if ident.eq_ignore_ascii_case(self.names.1) && allowed_types.intersects(self.types.1) {
      return Some(self.components.1);
    }

    if ident.eq_ignore_ascii_case(self.names.2) && allowed_types.intersects(self.types.2) {
      return Some(self.components.2);
    }

    if ident.eq_ignore_ascii_case("alpha") && allowed_types.intersects(ChannelType::Percentage) {
      return Some(self.components.3);
    }

    None
  }

  fn parse_ident<'i, 't>(
    &self,
    input: &mut Parser<'i, 't>,
    allowed_types: ChannelType,
  ) -> Result<f32, ParseError<'i, ParserError<'i>>> {
    match self.get_ident(input.expect_ident()?.as_ref(), allowed_types) {
      Some(v) => Ok(v),
      None => Err(input.new_error_for_next_token()),
    }
  }

  fn parse_calc<'i, 't>(
    &self,
    input: &mut Parser<'i, 't>,
    allowed_types: ChannelType,
  ) -> Result<f32, ParseError<'i, ParserError<'i>>> {
    match Calc::parse_with(input, |ident| self.get_ident(ident, allowed_types).map(Calc::Number)) {
      Ok(Calc::Value(v)) => Ok(*v),
      Ok(Calc::Number(n)) => Ok(n),
      _ => Err(input.new_custom_error(ParserError::InvalidValue)),
    }
  }
}

impl<'i> ColorParser<'i> for RelativeComponentParser {
  type Output = cssparser_color::Color;
  type Error = ParserError<'i>;

  fn parse_angle_or_number<'t>(
    &self,
    input: &mut Parser<'i, 't>,
  ) -> Result<AngleOrNumber, ParseError<'i, Self::Error>> {
    if let Ok(value) = input.try_parse(|input| self.parse_ident(input, ChannelType::Angle | ChannelType::Number)) {
      return Ok(AngleOrNumber::Number { value });
    }

    if let Ok(value) = input.try_parse(|input| self.parse_calc(input, ChannelType::Angle | ChannelType::Number)) {
      return Ok(AngleOrNumber::Number { value });
    }

    if let Ok(value) = input.try_parse(|input| -> Result<Angle, ParseError<'i, ParserError<'i>>> {
      match Calc::parse_with(input, |ident| {
        self
          .get_ident(ident, ChannelType::Angle | ChannelType::Number)
          .map(|v| Calc::Value(Box::new(Angle::Deg(v))))
      }) {
        Ok(Calc::Value(v)) => Ok(*v),
        _ => Err(input.new_custom_error(ParserError::InvalidValue)),
      }
    }) {
      return Ok(AngleOrNumber::Angle {
        degrees: value.to_degrees(),
      });
    }

    Err(input.new_error_for_next_token())
  }

  fn parse_number<'t>(&self, input: &mut Parser<'i, 't>) -> Result<f32, ParseError<'i, Self::Error>> {
    if let Ok(value) = input.try_parse(|input| self.parse_ident(input, ChannelType::Number)) {
      return Ok(value);
    }

    if let Ok(value) = input.try_parse(|input| self.parse_calc(input, ChannelType::Number)) {
      return Ok(value);
    }

    Err(input.new_error_for_next_token())
  }

  fn parse_percentage<'t>(&self, input: &mut Parser<'i, 't>) -> Result<f32, ParseError<'i, Self::Error>> {
    if let Ok(value) = input.try_parse(|input| self.parse_ident(input, ChannelType::Percentage)) {
      return Ok(value);
    }

    if let Ok(value) = input.try_parse(|input| -> Result<Percentage, ParseError<'i, ParserError<'i>>> {
      match Calc::parse_with(input, |ident| {
        self
          .get_ident(ident, ChannelType::Percentage)
          .map(|v| Calc::Value(Box::new(Percentage(v))))
      }) {
        Ok(Calc::Value(v)) => Ok(*v),
        _ => Err(input.new_custom_error(ParserError::InvalidValue)),
      }
    }) {
      return Ok(value.0);
    }

    Err(input.new_error_for_next_token())
  }

  fn parse_number_or_percentage<'t>(
    &self,
    input: &mut Parser<'i, 't>,
  ) -> Result<NumberOrPercentage, ParseError<'i, Self::Error>> {
    if let Ok(value) =
      input.try_parse(|input| self.parse_ident(input, ChannelType::Percentage | ChannelType::Number))
    {
      return Ok(NumberOrPercentage::Percentage { unit_value: value });
    }

    if let Ok(value) =
      input.try_parse(|input| self.parse_calc(input, ChannelType::Percentage | ChannelType::Number))
    {
      return Ok(NumberOrPercentage::Percentage { unit_value: value });
    }

    if let Ok(value) = input.try_parse(|input| -> Result<Percentage, ParseError<'i, ParserError<'i>>> {
      match Calc::parse_with(input, |ident| {
        self
          .get_ident(ident, ChannelType::Percentage | ChannelType::Number)
          .map(|v| Calc::Value(Box::new(Percentage(v))))
      }) {
        Ok(Calc::Value(v)) => Ok(*v),
        _ => Err(input.new_custom_error(ParserError::InvalidValue)),
      }
    }) {
      return Ok(NumberOrPercentage::Percentage { unit_value: value.0 });
    }

    Err(input.new_error_for_next_token())
  }
}

pub(crate) trait LightDarkColor {
  fn light_dark(light: Self, dark: Self) -> Self;
}

impl LightDarkColor for CssColor {
  #[inline]
  fn light_dark(light: Self, dark: Self) -> Self {
    CssColor::LightDark(Box::new(light), Box::new(dark))
  }
}

pub(crate) struct ComponentParser {
  pub allow_none: bool,
  from: Option<RelativeComponentParser>,
}

impl ComponentParser {
  pub fn new(allow_none: bool) -> Self {
    Self { allow_none, from: None }
  }

  pub fn parse_relative<
    'i,
    't,
    T: TryFrom<CssColor> + ColorSpace,
    C: LightDarkColor,
    P: Fn(&mut Parser<'i, 't>, &mut Self) -> Result<C, ParseError<'i, ParserError<'i>>>,
  >(
    &mut self,
    input: &mut Parser<'i, 't>,
    parse: P,
  ) -> Result<C, ParseError<'i, ParserError<'i>>> {
    if input.try_parse(|input| input.expect_ident_matching("from")).is_ok() {
      let from = CssColor::parse(input)?;
      return self.parse_from::<T, C, P>(from, input, &parse);
    }

    parse(input, self)
  }

  fn parse_from<
    'i,
    't,
    T: TryFrom<CssColor> + ColorSpace,
    C: LightDarkColor,
    P: Fn(&mut Parser<'i, 't>, &mut Self) -> Result<C, ParseError<'i, ParserError<'i>>>,
  >(
    &mut self,
    from: CssColor,
    input: &mut Parser<'i, 't>,
    parse: &P,
  ) -> Result<C, ParseError<'i, ParserError<'i>>> {
    if let CssColor::LightDark(light, dark) = from {
      let state = input.state();
      let light = self.parse_from::<T, C, P>(*light, input, parse)?;
      input.reset(&state);
      let dark = self.parse_from::<T, C, P>(*dark, input, parse)?;
      return Ok(C::light_dark(light, dark));
    }

    let from = T::try_from(from)
      .map_err(|_| input.new_custom_error(ParserError::InvalidValue))?
      .resolve();
    self.from = Some(RelativeComponentParser::new(&from));

    parse(input, self)
  }
}

impl<'i> ColorParser<'i> for ComponentParser {
  type Output = cssparser_color::Color;
  type Error = ParserError<'i>;

  fn parse_angle_or_number<'t>(
    &self,
    input: &mut Parser<'i, 't>,
  ) -> Result<AngleOrNumber, ParseError<'i, Self::Error>> {
    if let Some(from) = &self.from {
      if let Ok(res) = input.try_parse(|input| from.parse_angle_or_number(input)) {
        return Ok(res);
      }
    }

    if let Ok(angle) = input.try_parse(Angle::parse) {
      Ok(AngleOrNumber::Angle {
        degrees: angle.to_degrees(),
      })
    } else if let Ok(value) = input.try_parse(CSSNumber::parse) {
      Ok(AngleOrNumber::Number { value })
    } else if self.allow_none {
      input.expect_ident_matching("none")?;
      Ok(AngleOrNumber::Number { value: f32::NAN })
    } else {
      Err(input.new_custom_error(ParserError::InvalidValue))
    }
  }

  fn parse_number<'t>(&self, input: &mut Parser<'i, 't>) -> Result<f32, ParseError<'i, Self::Error>> {
    if let Some(from) = &self.from {
      if let Ok(res) = input.try_parse(|input| from.parse_number(input)) {
        return Ok(res);
      }
    }

    if let Ok(val) = input.try_parse(CSSNumber::parse) {
      return Ok(val);
    } else if self.allow_none {
      input.expect_ident_matching("none")?;
      Ok(f32::NAN)
    } else {
      Err(input.new_custom_error(ParserError::InvalidValue))
    }
  }

  fn parse_percentage<'t>(&self, input: &mut Parser<'i, 't>) -> Result<f32, ParseError<'i, Self::Error>> {
    if let Some(from) = &self.from {
      if let Ok(res) = input.try_parse(|input| from.parse_percentage(input)) {
        return Ok(res);
      }
    }

    if let Ok(val) = input.try_parse(Percentage::parse) {
      return Ok(val.0);
    } else if self.allow_none {
      input.expect_ident_matching("none")?;
      Ok(f32::NAN)
    } else {
      Err(input.new_custom_error(ParserError::InvalidValue))
    }
  }

  fn parse_number_or_percentage<'t>(
    &self,
    input: &mut Parser<'i, 't>,
  ) -> Result<NumberOrPercentage, ParseError<'i, Self::Error>> {
    if let Some(from) = &self.from {
      if let Ok(res) = input.try_parse(|input| from.parse_number_or_percentage(input)) {
        return Ok(res);
      }
    }

    if let Ok(value) = input.try_parse(CSSNumber::parse) {
      Ok(NumberOrPercentage::Number { value })
    } else if let Ok(value) = input.try_parse(Percentage::parse) {
      Ok(NumberOrPercentage::Percentage { unit_value: value.0 })
    } else if self.allow_none {
      input.expect_ident_matching("none")?;
      Ok(NumberOrPercentage::Number { value: f32::NAN })
    } else {
      Err(input.new_custom_error(ParserError::InvalidValue))
    }
  }
}

// https://www.w3.org/TR/css-color-4/#lab-colors
fn parse_color_function<'i, 't>(
  location: SourceLocation,
  function: CowRcStr<'i>,
  input: &mut Parser<'i, 't>,
) -> Result<CssColor, ParseError<'i, ParserError<'i>>> {
  let mut parser = ComponentParser::new(true);

  match_ignore_ascii_case! {&*function,
    "lab" => {
      parse_lab::<LAB, _>(input, &mut parser, |l, a, b, alpha| {
        LABColor::LAB(LAB { l, a, b, alpha })
      })
    },
    "oklab" => {
      parse_lab::<OKLAB, _>(input, &mut parser, |l, a, b, alpha| {
        LABColor::OKLAB(OKLAB { l, a, b, alpha })
      })
    },
    "lch" => {
      parse_lch::<LCH, _>(input, &mut parser, |l, c, h, alpha| {
        LABColor::LCH(LCH { l, c, h, alpha })
      })
    },
    "oklch" => {
      parse_lch::<OKLCH, _>(input, &mut parser, |l, c, h, alpha| {
        LABColor::OKLCH(OKLCH { l, c, h, alpha })
      })
    },
    "color" => {
      let predefined = parse_predefined(input, &mut parser)?;
      Ok(predefined)
    },
    "hsl" | "hsla" => {
      parse_hsl_hwb::<HSL, _>(input, &mut parser, true, |h, s, l, a| {
        let hsl = HSL { h, s, l, alpha: a };
        if !h.is_nan() && !s.is_nan() && !l.is_nan() && !a.is_nan() {
          CssColor::RGBA(hsl.into())
        } else {
          CssColor::Float(Box::new(FloatColor::HSL(hsl)))
        }
      })
    },
    "hwb" => {
      parse_hsl_hwb::<HWB, _>(input, &mut parser, false, |h, w, b, a| {
        let hwb = HWB { h, w, b, alpha: a };
        if !h.is_nan() && !w.is_nan() && !b.is_nan() && !a.is_nan() {
          CssColor::RGBA(hwb.into())
        } else {
          CssColor::Float(Box::new(FloatColor::HWB(hwb)))
        }
      })
    },
    "rgb" | "rgba" => {
       parse_rgb(input, &mut parser)
    },
    "color-mix" => {
      input.parse_nested_block(parse_color_mix)
    },
    "light-dark" => {
      input.parse_nested_block(|input| {
        let light = match CssColor::parse(input)? {
          CssColor::LightDark(light, _) => light,
          light => Box::new(light)
        };
        input.expect_comma()?;
        let dark = match CssColor::parse(input)? {
          CssColor::LightDark(_, dark) => dark,
          dark => Box::new(dark)
        };
        Ok(CssColor::LightDark(light, dark))
      })
    },
    _ => Err(location.new_unexpected_token_error(
      cssparser::Token::Ident(function.clone())
    ))
  }
}

/// Parses the lab() and oklab() functions.
#[inline]
fn parse_lab<'i, 't, T: TryFrom<CssColor> + ColorSpace, F: Fn(f32, f32, f32, f32) -> LABColor>(
  input: &mut Parser<'i, 't>,
  parser: &mut ComponentParser,
  f: F,
) -> Result<CssColor, ParseError<'i, ParserError<'i>>> {
  // https://www.w3.org/TR/css-color-4/#funcdef-lab
  input.parse_nested_block(|input| {
    parser.parse_relative::<T, _, _>(input, |input, parser| {
      // f32::max() does not propagate NaN, so use clamp for now until f32::maximum() is stable.
      let l = parser.parse_percentage(input)?.clamp(0.0, f32::MAX);
      let a = parser.parse_number(input)?;
      let b = parser.parse_number(input)?;
      let alpha = parse_alpha(input, parser)?;
      let lab = f(l, a, b, alpha);

      Ok(CssColor::LAB(Box::new(lab)))
    })
  })
}

/// Parses the lch() and oklch() functions.
#[inline]
fn parse_lch<'i, 't, T: TryFrom<CssColor> + ColorSpace, F: Fn(f32, f32, f32, f32) -> LABColor>(
  input: &mut Parser<'i, 't>,
  parser: &mut ComponentParser,
  f: F,
) -> Result<CssColor, ParseError<'i, ParserError<'i>>> {
  // https://www.w3.org/TR/css-color-4/#funcdef-lch
  input.parse_nested_block(|input| {
    parser.parse_relative::<T, _, _>(input, |input, parser| {
      if let Some(from) = &mut parser.from {
        // Relative angles should be normalized.
        // https://www.w3.org/TR/css-color-5/#relative-LCH
        from.components.2 %= 360.0;
        if from.components.2 < 0.0 {
          from.components.2 += 360.0;
        }
      }

      let l = parser.parse_percentage(input)?.clamp(0.0, f32::MAX);
      let c = parser.parse_number(input)?.clamp(0.0, f32::MAX);
      let h = parse_angle_or_number(input, parser)?;
      let alpha = parse_alpha(input, parser)?;
      let lab = f(l, c, h, alpha);

      Ok(CssColor::LAB(Box::new(lab)))
    })
  })
}

#[inline]
fn parse_predefined<'i, 't>(
  input: &mut Parser<'i, 't>,
  parser: &mut ComponentParser,
) -> Result<CssColor, ParseError<'i, ParserError<'i>>> {
  // https://www.w3.org/TR/css-color-4/#color-function
  let res = input.parse_nested_block(|input| {
    let from = if input.try_parse(|input| input.expect_ident_matching("from")).is_ok() {
      Some(CssColor::parse(input)?)
    } else {
      None
    };

    let colorspace = input.expect_ident_cloned()?;

    if let Some(CssColor::LightDark(light, dark)) = from {
      let state = input.state();
      let light = parse_predefined_relative(input, parser, &colorspace, Some(&*light))?;
      input.reset(&state);
      let dark = parse_predefined_relative(input, parser, &colorspace, Some(&*dark))?;
      return Ok(CssColor::LightDark(Box::new(light), Box::new(dark)));
    }

    parse_predefined_relative(input, parser, &colorspace, from.as_ref())
  })?;

  Ok(res)
}

#[inline]
fn parse_predefined_relative<'i, 't>(
  input: &mut Parser<'i, 't>,
  parser: &mut ComponentParser,
  colorspace: &CowRcStr<'i>,
  from: Option<&CssColor>,
) -> Result<CssColor, ParseError<'i, ParserError<'i>>> {
  let location = input.current_source_location();
  if let Some(from) = from {
    let handle_error = |_| input.new_custom_error(ParserError::InvalidValue);
    parser.from = Some(match_ignore_ascii_case! { &*&colorspace,
      "srgb" => RelativeComponentParser::new(&SRGB::try_from(from).map_err(handle_error)?.resolve_missing()),
      "srgb-linear" => RelativeComponentParser::new(&SRGBLinear::try_from(from).map_err(handle_error)?.resolve_missing()),
      "display-p3" => RelativeComponentParser::new(&P3::try_from(from).map_err(handle_error)?.resolve_missing()),
      "a98-rgb" => RelativeComponentParser::new(&A98::try_from(from).map_err(handle_error)?.resolve_missing()),
      "prophoto-rgb" => RelativeComponentParser::new(&ProPhoto::try_from(from).map_err(handle_error)?.resolve_missing()),
      "rec2020" => RelativeComponentParser::new(&Rec2020::try_from(from).map_err(handle_error)?.resolve_missing()),
      "xyz-d50" => RelativeComponentParser::new(&XYZd50::try_from(from).map_err(handle_error)?.resolve_missing()),
      "xyz" | "xyz-d65" => RelativeComponentParser::new(&XYZd65::try_from(from).map_err(handle_error)?.resolve_missing()),
      _ => return Err(location.new_unexpected_token_error(
        cssparser::Token::Ident(colorspace.clone())
      ))
    });
  }

  // Out of gamut values should not be clamped, i.e. values < 0 or > 1 should be preserved.
  // The browser will gamut-map the color for the target device that it is rendered on.
  let a = input.try_parse(|input| parse_number_or_percentage(input, parser))?;
  let b = input.try_parse(|input| parse_number_or_percentage(input, parser))?;
  let c = input.try_parse(|input| parse_number_or_percentage(input, parser))?;
  let alpha = parse_alpha(input, parser)?;

  let res = match_ignore_ascii_case! { &*&colorspace,
    "srgb" => PredefinedColor::SRGB(SRGB { r: a, g: b, b: c, alpha }),
    "srgb-linear" => PredefinedColor::SRGBLinear(SRGBLinear { r: a, g: b, b: c, alpha }),
    "display-p3" => PredefinedColor::DisplayP3(P3 { r: a, g: b, b: c, alpha }),
    "a98-rgb" => PredefinedColor::A98(A98 { r: a, g: b, b: c, alpha }),
    "prophoto-rgb" => PredefinedColor::ProPhoto(ProPhoto { r: a, g: b, b: c, alpha }),
    "rec2020" => PredefinedColor::Rec2020(Rec2020 { r: a, g: b, b: c, alpha }),
    "xyz-d50" => PredefinedColor::XYZd50(XYZd50 { x: a, y: b, z: c, alpha}),
    "xyz" | "xyz-d65" => PredefinedColor::XYZd65(XYZd65 { x: a, y: b, z: c, alpha }),
    _ => return Err(location.new_unexpected_token_error(
      cssparser::Token::Ident(colorspace.clone())
    ))
  };

  Ok(CssColor::Predefined(Box::new(res)))
}

/// Parses the hsl() and hwb() functions.
/// The results of this function are stored as floating point if there are any `none` components.
#[inline]
fn parse_hsl_hwb<'i, 't, T: TryFrom<CssColor> + ColorSpace, F: Fn(f32, f32, f32, f32) -> CssColor>(
  input: &mut Parser<'i, 't>,
  parser: &mut ComponentParser,
  allows_legacy: bool,
  f: F,
) -> Result<CssColor, ParseError<'i, ParserError<'i>>> {
  // https://drafts.csswg.org/css-color-4/#the-hsl-notation
  input.parse_nested_block(|input| {
    parser.parse_relative::<T, _, _>(input, |input, parser| {
      let (h, a, b, is_legacy) = parse_hsl_hwb_components::<T>(input, parser, allows_legacy)?;
      let alpha = if is_legacy {
        parse_legacy_alpha(input, parser)?
      } else {
        parse_alpha(input, parser)?
      };

      Ok(f(h, a, b, alpha))
    })
  })
}

#[inline]
pub(crate) fn parse_hsl_hwb_components<'i, 't, T: TryFrom<CssColor> + ColorSpace>(
  input: &mut Parser<'i, 't>,
  parser: &mut ComponentParser,
  allows_legacy: bool,
) -> Result<(f32, f32, f32, bool), ParseError<'i, ParserError<'i>>> {
  let h = parse_angle_or_number(input, parser)?;
  let is_legacy_syntax =
    allows_legacy && parser.from.is_none() && !h.is_nan() && input.try_parse(|p| p.expect_comma()).is_ok();
  let a = parser.parse_percentage(input)?.clamp(0.0, 1.0);
  if is_legacy_syntax {
    input.expect_comma()?;
  }
  let b = parser.parse_percentage(input)?.clamp(0.0, 1.0);
  if is_legacy_syntax && (a.is_nan() || b.is_nan()) {
    return Err(input.new_custom_error(ParserError::InvalidValue));
  }
  Ok((h, a, b, is_legacy_syntax))
}

#[inline]
fn parse_rgb<'i, 't>(
  input: &mut Parser<'i, 't>,
  parser: &mut ComponentParser,
) -> Result<CssColor, ParseError<'i, ParserError<'i>>> {
  // https://drafts.csswg.org/css-color-4/#rgb-functions
  input.parse_nested_block(|input| {
    parser.parse_relative::<SRGB, _, _>(input, |input, parser| {
      let (r, g, b, is_legacy) = parse_rgb_components(input, parser)?;
      let alpha = if is_legacy {
        parse_legacy_alpha(input, parser)?
      } else {
        parse_alpha(input, parser)?
      };

      if !r.is_nan() && !g.is_nan() && !b.is_nan() && !alpha.is_nan() {
        if is_legacy {
          Ok(CssColor::RGBA(RGBA::new(r as u8, g as u8, b as u8, alpha)))
        } else {
          Ok(CssColor::RGBA(RGBA::from_floats(r, g, b, alpha)))
        }
      } else {
        Ok(CssColor::Float(Box::new(FloatColor::RGB(SRGB { r, g, b, alpha }))))
      }
    })
  })
}

#[inline]
pub(crate) fn parse_rgb_components<'i, 't>(
  input: &mut Parser<'i, 't>,
  parser: &mut ComponentParser,
) -> Result<(f32, f32, f32, bool), ParseError<'i, ParserError<'i>>> {
  let red = parser.parse_number_or_percentage(input)?;
  let is_legacy_syntax =
    parser.from.is_none() && !red.unit_value().is_nan() && input.try_parse(|p| p.expect_comma()).is_ok();
  let (r, g, b) = if is_legacy_syntax {
    match red {
      NumberOrPercentage::Number { value } => {
        let r = value.round().clamp(0.0, 255.0);
        let g = parser.parse_number(input)?.round().clamp(0.0, 255.0);
        input.expect_comma()?;
        let b = parser.parse_number(input)?.round().clamp(0.0, 255.0);
        (r, g, b)
      }
      NumberOrPercentage::Percentage { unit_value } => {
        let r = (unit_value * 255.0).round().clamp(0.0, 255.0);
        let g = (parser.parse_percentage(input)? * 255.0).round().clamp(0.0, 255.0);
        input.expect_comma()?;
        let b = (parser.parse_percentage(input)? * 255.0).round().clamp(0.0, 255.0);
        (r, g, b)
      }
    }
  } else {
    #[inline]
    fn get_component<'i, 't>(value: NumberOrPercentage) -> f32 {
      match value {
        NumberOrPercentage::Number { value } if value.is_nan() => value,
        NumberOrPercentage::Number { value } => value.round().clamp(0.0, 255.0) / 255.0,
        NumberOrPercentage::Percentage { unit_value } => unit_value.clamp(0.0, 1.0),
      }
    }

    let r = get_component(red);
    let g = get_component(parser.parse_number_or_percentage(input)?);
    let b = get_component(parser.parse_number_or_percentage(input)?);
    (r, g, b)
  };

  if is_legacy_syntax && (g.is_nan() || b.is_nan()) {
    return Err(input.new_custom_error(ParserError::InvalidValue));
  }
  Ok((r, g, b, is_legacy_syntax))
}

#[inline]
fn parse_angle_or_number<'i, 't>(
  input: &mut Parser<'i, 't>,
  parser: &ComponentParser,
) -> Result<f32, ParseError<'i, ParserError<'i>>> {
  Ok(match parser.parse_angle_or_number(input)? {
    AngleOrNumber::Number { value } => value,
    AngleOrNumber::Angle { degrees } => degrees,
  })
}

#[inline]
fn parse_number_or_percentage<'i, 't>(
  input: &mut Parser<'i, 't>,
  parser: &ComponentParser,
) -> Result<f32, ParseError<'i, ParserError<'i>>> {
  Ok(match parser.parse_number_or_percentage(input)? {
    NumberOrPercentage::Number { value } => value,
    NumberOrPercentage::Percentage { unit_value } => unit_value,
  })
}

#[inline]
fn parse_alpha<'i, 't>(
  input: &mut Parser<'i, 't>,
  parser: &ComponentParser,
) -> Result<f32, ParseError<'i, ParserError<'i>>> {
  let res = if input.try_parse(|input| input.expect_delim('/')).is_ok() {
    parse_number_or_percentage(input, parser)?.clamp(0.0, 1.0)
  } else {
    1.0
  };
  Ok(res)
}

#[inline]
fn parse_legacy_alpha<'i, 't>(
  input: &mut Parser<'i, 't>,
  parser: &ComponentParser,
) -> Result<f32, ParseError<'i, ParserError<'i>>> {
  Ok(if !input.is_exhausted() {
    input.expect_comma()?;
    parse_number_or_percentage(input, parser)?.clamp(0.0, 1.0)
  } else {
    1.0
  })
}

#[inline]
fn write_components<W>(
  name: &str,
  a: f32,
  b: f32,
  c: f32,
  alpha: f32,
  dest: &mut Printer<W>,
) -> Result<(), PrinterError>
where
  W: std::fmt::Write,
{
  dest.write_str(name)?;
  dest.write_char('(')?;
  if a.is_nan() {
    dest.write_str("none")?;
  } else {
    Percentage(a).to_css(dest)?;
  }
  dest.write_char(' ')?;
  write_component(b, dest)?;
  dest.write_char(' ')?;
  write_component(c, dest)?;
  if alpha.is_nan() || (alpha - 1.0).abs() > f32::EPSILON {
    dest.delim('/', true)?;
    write_component(alpha, dest)?;
  }

  dest.write_char(')')
}

#[inline]
fn write_component<W>(c: f32, dest: &mut Printer<W>) -> Result<(), PrinterError>
where
  W: std::fmt::Write,
{
  if c.is_nan() {
    dest.write_str("none")?;
  } else {
    c.to_css(dest)?;
  }
  Ok(())
}

#[inline]
fn write_predefined<W>(predefined: &PredefinedColor, dest: &mut Printer<W>) -> Result<(), PrinterError>
where
  W: std::fmt::Write,
{
  use PredefinedColor::*;

  let (name, a, b, c, alpha) = match predefined {
    SRGB(rgb) => ("srgb", rgb.r, rgb.g, rgb.b, rgb.alpha),
    SRGBLinear(rgb) => ("srgb-linear", rgb.r, rgb.g, rgb.b, rgb.alpha),
    DisplayP3(rgb) => ("display-p3", rgb.r, rgb.g, rgb.b, rgb.alpha),
    A98(rgb) => ("a98-rgb", rgb.r, rgb.g, rgb.b, rgb.alpha),
    ProPhoto(rgb) => ("prophoto-rgb", rgb.r, rgb.g, rgb.b, rgb.alpha),
    Rec2020(rgb) => ("rec2020", rgb.r, rgb.g, rgb.b, rgb.alpha),
    XYZd50(xyz) => ("xyz-d50", xyz.x, xyz.y, xyz.z, xyz.alpha),
    // "xyz" has better compatibility (Safari 15) than "xyz-d65", and it is shorter.
    XYZd65(xyz) => ("xyz", xyz.x, xyz.y, xyz.z, xyz.alpha),
  };

  dest.write_str("color(")?;
  dest.write_str(name)?;
  dest.write_char(' ')?;
  write_component(a, dest)?;
  dest.write_char(' ')?;
  write_component(b, dest)?;
  dest.write_char(' ')?;
  write_component(c, dest)?;

  if alpha.is_nan() || (alpha - 1.0).abs() > f32::EPSILON {
    dest.delim('/', true)?;
    write_component(alpha, dest)?;
  }

  dest.write_char(')')
}

bitflags! {
  /// A channel type for a color space.
  #[derive(PartialEq, Eq, Clone, Copy)]
  pub struct ChannelType: u8 {
    /// Channel represents a percentage.
    const Percentage = 0b001;
    /// Channel represents an angle.
    const Angle = 0b010;
    /// Channel represents a number.
    const Number = 0b100;
  }
}

/// A trait for color spaces.
pub trait ColorSpace {
  /// Returns the raw color component values.
  fn components(&self) -> (f32, f32, f32, f32);
  /// Returns the channel names for this color space.
  fn channels(&self) -> (&'static str, &'static str, &'static str);
  /// Returns the channel types for this color space.
  fn types(&self) -> (ChannelType, ChannelType, ChannelType);
  /// Resolves missing color components (e.g. `none` keywords) in the color.
  fn resolve_missing(&self) -> Self;
  /// Returns a resolved color by replacing missing (i.e. `none`) components with zero,
  /// and performing gamut mapping to ensure the color can be represented within the color space.
  fn resolve(&self) -> Self;
}

macro_rules! define_colorspace {
  (
    $(#[$outer:meta])*
    $vis:vis struct $name:ident {
      $(#[$a_meta: meta])*
      $a: ident: $at: ident,
      $(#[$b_meta: meta])*
      $b: ident: $bt: ident,
      $(#[$c_meta: meta])*
      $c: ident: $ct: ident
    }
  ) => {
    $(#[$outer])*
    #[derive(Debug, Clone, Copy, PartialEq)] #[cfg_attr(feature = "visitor", derive(Visit))]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
    pub struct $name {
      $(#[$a_meta])*
      pub $a: f32,
      $(#[$b_meta])*
      pub $b: f32,
      $(#[$c_meta])*
      pub $c: f32,
      /// The alpha component.
      pub alpha: f32,
    }

    impl ColorSpace for $name {
      fn components(&self) -> (f32, f32, f32, f32) {
        (self.$a, self.$b, self.$c, self.alpha)
      }

      fn channels(&self) -> (&'static str, &'static str, &'static str) {
        (stringify!($a), stringify!($b), stringify!($c))
      }

      fn types(&self) -> (ChannelType, ChannelType, ChannelType) {
        (ChannelType::$at, ChannelType::$bt, ChannelType::$ct)
      }

      #[inline]
      fn resolve_missing(&self) -> Self {
        Self {
          $a: if self.$a.is_nan() { 0.0 } else { self.$a },
          $b: if self.$b.is_nan() { 0.0 } else { self.$b },
          $c: if self.$c.is_nan() { 0.0 } else { self.$c },
          alpha: if self.alpha.is_nan() { 0.0 } else { self.alpha },
        }
      }

      #[inline]
      fn resolve(&self) -> Self {
        let mut resolved = self.resolve_missing();
        if !resolved.in_gamut() {
          resolved = map_gamut(resolved);
        }
        resolved
      }
    }
  };
}

define_colorspace! {
  /// A color in the [`sRGB`](https://www.w3.org/TR/css-color-4/#predefined-sRGB) color space.
  pub struct SRGB {
    /// The red component.
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_rgb_component", deserialize_with = "deserialize_rgb_component"))]
    r: Percentage,
    /// The green component.
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_rgb_component", deserialize_with = "deserialize_rgb_component"))]
    g: Percentage,
    /// The blue component.
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_rgb_component", deserialize_with = "deserialize_rgb_component"))]
    b: Percentage
  }
}

// serialize RGB components in the 0-255 range as it is more common.
#[cfg(feature = "serde")]
fn serialize_rgb_component<S>(v: &f32, serializer: S) -> Result<S::Ok, S::Error>
where
  S: serde::Serializer,
{
  let v = if !v.is_nan() {
    (v * 255.0).round().max(0.0).min(255.0)
  } else {
    *v
  };

  serializer.serialize_f32(v)
}

#[cfg(feature = "serde")]
fn deserialize_rgb_component<'de, D>(deserializer: D) -> Result<f32, D::Error>
where
  D: serde::Deserializer<'de>,
{
  let v: f32 = serde::Deserialize::deserialize(deserializer)?;
  Ok(v / 255.0)
}

// Copied from an older version of cssparser.
/// A color with red, green, blue, and alpha components, in a byte each.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct RGBA {
  /// The red component.
  pub red: u8,
  /// The green component.
  pub green: u8,
  /// The blue component.
  pub blue: u8,
  /// The alpha component.
  pub alpha: u8,
}

impl RGBA {
  /// Constructs a new RGBA value from float components. It expects the red,
  /// green, blue and alpha channels in that order, and all values will be
  /// clamped to the 0.0 ... 1.0 range.
  #[inline]
  pub fn from_floats(red: f32, green: f32, blue: f32, alpha: f32) -> Self {
    Self::new(clamp_unit_f32(red), clamp_unit_f32(green), clamp_unit_f32(blue), alpha)
  }

  /// Returns a transparent color.
  #[inline]
  pub fn transparent() -> Self {
    Self::new(0, 0, 0, 0.0)
  }

  /// Same thing, but with `u8` values instead of floats in the 0 to 1 range.
  #[inline]
  pub fn new(red: u8, green: u8, blue: u8, alpha: f32) -> Self {
    RGBA {
      red,
      green,
      blue,
      alpha: clamp_unit_f32(alpha),
    }
  }

  /// Returns the red channel in a floating point number form, from 0 to 1.
  #[inline]
  pub fn red_f32(&self) -> f32 {
    self.red as f32 / 255.0
  }

  /// Returns the green channel in a floating point number form, from 0 to 1.
  #[inline]
  pub fn green_f32(&self) -> f32 {
    self.green as f32 / 255.0
  }

  /// Returns the blue channel in a floating point number form, from 0 to 1.
  #[inline]
  pub fn blue_f32(&self) -> f32 {
    self.blue as f32 / 255.0
  }

  /// Returns the alpha channel in a floating point number form, from 0 to 1.
  #[inline]
  pub fn alpha_f32(&self) -> f32 {
    self.alpha as f32 / 255.0
  }
}

fn clamp_unit_f32(val: f32) -> u8 {
  // Whilst scaling by 256 and flooring would provide
  // an equal distribution of integers to percentage inputs,
  // this is not what Gecko does so we instead multiply by 255
  // and round (adding 0.5 and flooring is equivalent to rounding)
  //
  // Chrome does something similar for the alpha value, but not
  // the rgb values.
  //
  // See https://bugzilla.mozilla.org/show_bug.cgi?id=1340484
  //
  // Clamping to 256 and rounding after would let 1.0 map to 256, and
  // `256.0_f32 as u8` is undefined behavior:
  //
  // https://github.com/rust-lang/rust/issues/10184
  clamp_floor_256_f32(val * 255.)
}

fn clamp_floor_256_f32(val: f32) -> u8 {
  val.round().max(0.).min(255.) as u8
}

define_colorspace! {
  /// A color in the [`sRGB-linear`](https://www.w3.org/TR/css-color-4/#predefined-sRGB-linear) color space.
  pub struct SRGBLinear {
    /// The red component.
    r: Percentage,
    /// The green component.
    g: Percentage,
    /// The blue component.
    b: Percentage
  }
}

define_colorspace! {
  /// A color in the [`display-p3`](https://www.w3.org/TR/css-color-4/#predefined-display-p3) color space.
  pub struct P3 {
    /// The red component.
    r: Percentage,
    /// The green component.
    g: Percentage,
    /// The blue component.
    b: Percentage
  }
}

define_colorspace! {
  /// A color in the [`a98-rgb`](https://www.w3.org/TR/css-color-4/#predefined-a98-rgb) color space.
  pub struct A98 {
    /// The red component.
    r: Percentage,
    /// The green component.
    g: Percentage,
    /// The blue component.
    b: Percentage
  }
}

define_colorspace! {
  /// A color in the [`prophoto-rgb`](https://www.w3.org/TR/css-color-4/#predefined-prophoto-rgb) color space.
  pub struct ProPhoto {
    /// The red component.
    r: Percentage,
    /// The green component.
    g: Percentage,
    /// The blue component.
    b: Percentage
  }
}

define_colorspace! {
  /// A color in the [`rec2020`](https://www.w3.org/TR/css-color-4/#predefined-rec2020) color space.
  pub struct Rec2020 {
    /// The red component.
    r: Percentage,
    /// The green component.
    g: Percentage,
    /// The blue component.
    b: Percentage
  }
}

define_colorspace! {
  /// A color in the [CIE Lab](https://www.w3.org/TR/css-color-4/#cie-lab) color space.
  pub struct LAB {
    /// The lightness component.
    l: Percentage,
    /// The a component.
    a: Number,
    /// The b component.
    b: Number
  }
}

define_colorspace! {
  /// A color in the [CIE LCH](https://www.w3.org/TR/css-color-4/#cie-lab) color space.
  pub struct LCH {
    /// The lightness component.
    l: Percentage,
    /// The chroma component.
    c: Number,
    /// The hue component.
    h: Angle
  }
}

define_colorspace! {
  /// A color in the [OKLab](https://www.w3.org/TR/css-color-4/#ok-lab) color space.
  pub struct OKLAB {
    /// The lightness component.
    l: Percentage,
    /// The a component.
    a: Number,
    /// The b component.
    b: Number
  }
}

define_colorspace! {
  /// A color in the [OKLCH](https://www.w3.org/TR/css-color-4/#ok-lab) color space.
  pub struct OKLCH {
    /// The lightness component.
    l: Percentage,
    /// The chroma component.
    c: Number,
    /// The hue component.
    h: Angle
  }
}

define_colorspace! {
  /// A color in the [`xyz-d50`](https://www.w3.org/TR/css-color-4/#predefined-xyz) color space.
  pub struct XYZd50 {
    /// The x component.
    x: Percentage,
    /// The y component.
    y: Percentage,
    /// The z component.
    z: Percentage
  }
}

define_colorspace! {
  /// A color in the [`xyz-d65`](https://www.w3.org/TR/css-color-4/#predefined-xyz) color space.
  pub struct XYZd65 {
    /// The x component.
    x: Percentage,
    /// The y component.
    y: Percentage,
    /// The z component.
    z: Percentage
  }
}

define_colorspace! {
  /// A color in the [`hsl`](https://www.w3.org/TR/css-color-4/#the-hsl-notation) color space.
  pub struct HSL {
    /// The hue component.
    h: Angle,
    /// The saturation component.
    s: Percentage,
    /// The lightness component.
    l: Percentage
  }
}

define_colorspace! {
  /// A color in the [`hwb`](https://www.w3.org/TR/css-color-4/#the-hwb-notation) color space.
  pub struct HWB {
    /// The hue component.
    h: Angle,
    /// The whiteness component.
    w: Percentage,
    /// The blackness component.
    b: Percentage
  }
}

macro_rules! via {
  ($t: ident -> $u: ident -> $v: ident) => {
    impl From<$t> for $v {
      #[inline]
      fn from(t: $t) -> $v {
        let xyz: $u = t.into();
        xyz.into()
      }
    }

    impl From<$v> for $t {
      #[inline]
      fn from(t: $v) -> $t {
        let xyz: $u = t.into();
        xyz.into()
      }
    }
  };
}

#[inline]
fn rectangular_to_polar(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
  // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L375
  let mut h = b.atan2(a) * 180.0 / PI;
  if h < 0.0 {
    h += 360.0;
  }
  let c = (a.powi(2) + b.powi(2)).sqrt();
  h = h % 360.0;
  (l, c, h)
}

#[inline]
fn polar_to_rectangular(l: f32, c: f32, h: f32) -> (f32, f32, f32) {
  // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L385
  let a = c * (h * PI / 180.0).cos();
  let b = c * (h * PI / 180.0).sin();
  (l, a, b)
}

impl From<LCH> for LAB {
  fn from(lch: LCH) -> LAB {
    let lch = lch.resolve_missing();
    let (l, a, b) = polar_to_rectangular(lch.l, lch.c, lch.h);
    LAB {
      l,
      a,
      b,
      alpha: lch.alpha,
    }
  }
}

impl From<LAB> for LCH {
  fn from(lab: LAB) -> LCH {
    let lab = lab.resolve_missing();
    let (l, c, h) = rectangular_to_polar(lab.l, lab.a, lab.b);
    LCH {
      l,
      c,
      h,
      alpha: lab.alpha,
    }
  }
}

impl From<OKLCH> for OKLAB {
  fn from(lch: OKLCH) -> OKLAB {
    let lch = lch.resolve_missing();
    let (l, a, b) = polar_to_rectangular(lch.l, lch.c, lch.h);
    OKLAB {
      l,
      a,
      b,
      alpha: lch.alpha,
    }
  }
}

impl From<OKLAB> for OKLCH {
  fn from(lab: OKLAB) -> OKLCH {
    let lab = lab.resolve_missing();
    let (l, c, h) = rectangular_to_polar(lab.l, lab.a, lab.b);
    OKLCH {
      l,
      c,
      h,
      alpha: lab.alpha,
    }
  }
}

const D50: &[f32] = &[0.3457 / 0.3585, 1.00000, (1.0 - 0.3457 - 0.3585) / 0.3585];

impl From<LAB> for XYZd50 {
  fn from(lab: LAB) -> XYZd50 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L352
    const K: f32 = 24389.0 / 27.0; // 29^3/3^3
    const E: f32 = 216.0 / 24389.0; // 6^3/29^3

    let lab = lab.resolve_missing();
    let l = lab.l * 100.0;
    let a = lab.a;
    let b = lab.b;

    // compute f, starting with the luminance-related term
    let f1 = (l + 16.0) / 116.0;
    let f0 = a / 500.0 + f1;
    let f2 = f1 - b / 200.0;

    // compute xyz
    let x = if f0.powi(3) > E {
      f0.powi(3)
    } else {
      (116.0 * f0 - 16.0) / K
    };

    let y = if l > K * E { ((l + 16.0) / 116.0).powi(3) } else { l / K };

    let z = if f2.powi(3) > E {
      f2.powi(3)
    } else {
      (116.0 * f2 - 16.0) / K
    };

    // Compute XYZ by scaling xyz by reference white
    XYZd50 {
      x: x * D50[0],
      y: y * D50[1],
      z: z * D50[2],
      alpha: lab.alpha,
    }
  }
}

impl From<XYZd50> for XYZd65 {
  fn from(xyz: XYZd50) -> XYZd65 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L319
    const MATRIX: &[f32] = &[
      0.9554734527042182,
      -0.023098536874261423,
      0.0632593086610217,
      -0.028369706963208136,
      1.0099954580058226,
      0.021041398966943008,
      0.012314001688319899,
      -0.020507696433477912,
      1.3303659366080753,
    ];

    let xyz = xyz.resolve_missing();
    let (x, y, z) = multiply_matrix(MATRIX, xyz.x, xyz.y, xyz.z);
    XYZd65 {
      x,
      y,
      z,
      alpha: xyz.alpha,
    }
  }
}

impl From<XYZd65> for XYZd50 {
  fn from(xyz: XYZd65) -> XYZd50 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L319
    const MATRIX: &[f32] = &[
      1.0479298208405488,
      0.022946793341019088,
      -0.05019222954313557,
      0.029627815688159344,
      0.990434484573249,
      -0.01707382502938514,
      -0.009243058152591178,
      0.015055144896577895,
      0.7518742899580008,
    ];

    let xyz = xyz.resolve_missing();
    let (x, y, z) = multiply_matrix(MATRIX, xyz.x, xyz.y, xyz.z);
    XYZd50 {
      x,
      y,
      z,
      alpha: xyz.alpha,
    }
  }
}

impl From<XYZd65> for SRGBLinear {
  fn from(xyz: XYZd65) -> SRGBLinear {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L62
    const MATRIX: &[f32] = &[
      3.2409699419045226,
      -1.537383177570094,
      -0.4986107602930034,
      -0.9692436362808796,
      1.8759675015077202,
      0.04155505740717559,
      0.05563007969699366,
      -0.20397695888897652,
      1.0569715142428786,
    ];

    let xyz = xyz.resolve_missing();
    let (r, g, b) = multiply_matrix(MATRIX, xyz.x, xyz.y, xyz.z);
    SRGBLinear {
      r,
      g,
      b,
      alpha: xyz.alpha,
    }
  }
}

#[inline]
fn multiply_matrix(m: &[f32], x: f32, y: f32, z: f32) -> (f32, f32, f32) {
  let a = m[0] * x + m[1] * y + m[2] * z;
  let b = m[3] * x + m[4] * y + m[5] * z;
  let c = m[6] * x + m[7] * y + m[8] * z;
  (a, b, c)
}

impl From<SRGBLinear> for SRGB {
  #[inline]
  fn from(rgb: SRGBLinear) -> SRGB {
    let rgb = rgb.resolve_missing();
    let (r, g, b) = gam_srgb(rgb.r, rgb.g, rgb.b);
    SRGB {
      r,
      g,
      b,
      alpha: rgb.alpha,
    }
  }
}

fn gam_srgb(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
  // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L31
  // convert an array of linear-light sRGB values in the range 0.0-1.0
  // to gamma corrected form
  // https://en.wikipedia.org/wiki/SRGB
  // Extended transfer function:
  // For negative values, linear portion extends on reflection
  // of axis, then uses reflected pow below that

  #[inline]
  fn gam_srgb_component(c: f32) -> f32 {
    let abs = c.abs();
    if abs > 0.0031308 {
      let sign = if c < 0.0 { -1.0 } else { 1.0 };
      return sign * (1.055 * abs.powf(1.0 / 2.4) - 0.055);
    }

    return 12.92 * c;
  }

  let r = gam_srgb_component(r);
  let g = gam_srgb_component(g);
  let b = gam_srgb_component(b);
  (r, g, b)
}

impl From<OKLAB> for XYZd65 {
  fn from(lab: OKLAB) -> XYZd65 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L418
    const LMS_TO_XYZ: &[f32] = &[
      1.2268798733741557,
      -0.5578149965554813,
      0.28139105017721583,
      -0.04057576262431372,
      1.1122868293970594,
      -0.07171106666151701,
      -0.07637294974672142,
      -0.4214933239627914,
      1.5869240244272418,
    ];

    const OKLAB_TO_LMS: &[f32] = &[
      0.99999999845051981432,
      0.39633779217376785678,
      0.21580375806075880339,
      1.0000000088817607767,
      -0.1055613423236563494,
      -0.063854174771705903402,
      1.0000000546724109177,
      -0.089484182094965759684,
      -1.2914855378640917399,
    ];

    let lab = lab.resolve_missing();
    let (a, b, c) = multiply_matrix(OKLAB_TO_LMS, lab.l, lab.a, lab.b);
    let (x, y, z) = multiply_matrix(LMS_TO_XYZ, a.powi(3), b.powi(3), c.powi(3));
    XYZd65 {
      x,
      y,
      z,
      alpha: lab.alpha,
    }
  }
}

impl From<XYZd65> for OKLAB {
  fn from(xyz: XYZd65) -> OKLAB {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L400
    const XYZ_TO_LMS: &[f32] = &[
      0.8190224432164319,
      0.3619062562801221,
      -0.12887378261216414,
      0.0329836671980271,
      0.9292868468965546,
      0.03614466816999844,
      0.048177199566046255,
      0.26423952494422764,
      0.6335478258136937,
    ];

    const LMS_TO_OKLAB: &[f32] = &[
      0.2104542553,
      0.7936177850,
      -0.0040720468,
      1.9779984951,
      -2.4285922050,
      0.4505937099,
      0.0259040371,
      0.7827717662,
      -0.8086757660,
    ];

    let xyz = xyz.resolve_missing();
    let (a, b, c) = multiply_matrix(XYZ_TO_LMS, xyz.x, xyz.y, xyz.z);
    let (l, a, b) = multiply_matrix(LMS_TO_OKLAB, a.cbrt(), b.cbrt(), c.cbrt());
    OKLAB {
      l,
      a,
      b,
      alpha: xyz.alpha,
    }
  }
}

impl From<XYZd50> for LAB {
  fn from(xyz: XYZd50) -> LAB {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L332
    // Assuming XYZ is relative to D50, convert to CIE LAB
    // from CIE standard, which now defines these as a rational fraction
    const E: f32 = 216.0 / 24389.0; // 6^3/29^3
    const K: f32 = 24389.0 / 27.0; // 29^3/3^3

    // compute xyz, which is XYZ scaled relative to reference white
    let xyz = xyz.resolve_missing();
    let x = xyz.x / D50[0];
    let y = xyz.y / D50[1];
    let z = xyz.z / D50[2];

    // now compute f
    let f0 = if x > E { x.cbrt() } else { (K * x + 16.0) / 116.0 };

    let f1 = if y > E { y.cbrt() } else { (K * y + 16.0) / 116.0 };

    let f2 = if z > E { z.cbrt() } else { (K * z + 16.0) / 116.0 };

    let l = ((116.0 * f1) - 16.0) / 100.0;
    let a = 500.0 * (f0 - f1);
    let b = 200.0 * (f1 - f2);
    LAB {
      l,
      a,
      b,
      alpha: xyz.alpha,
    }
  }
}

impl From<SRGB> for SRGBLinear {
  fn from(rgb: SRGB) -> SRGBLinear {
    let rgb = rgb.resolve_missing();
    let (r, g, b) = lin_srgb(rgb.r, rgb.g, rgb.b);
    SRGBLinear {
      r,
      g,
      b,
      alpha: rgb.alpha,
    }
  }
}

fn lin_srgb(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
  // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L11
  // convert sRGB values where in-gamut values are in the range [0 - 1]
  // to linear light (un-companded) form.
  // https://en.wikipedia.org/wiki/SRGB
  // Extended transfer function:
  // for negative values, linear portion is extended on reflection of axis,
  // then reflected power function is used.

  #[inline]
  fn lin_srgb_component(c: f32) -> f32 {
    let abs = c.abs();
    if abs < 0.04045 {
      return c / 12.92;
    }

    let sign = if c < 0.0 { -1.0 } else { 1.0 };
    sign * ((abs + 0.055) / 1.055).powf(2.4)
  }

  let r = lin_srgb_component(r);
  let g = lin_srgb_component(g);
  let b = lin_srgb_component(b);
  (r, g, b)
}

impl From<SRGBLinear> for XYZd65 {
  fn from(rgb: SRGBLinear) -> XYZd65 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L50
    // convert an array of linear-light sRGB values to CIE XYZ
    // using sRGB's own white, D65 (no chromatic adaptation)
    const MATRIX: &[f32] = &[
      0.41239079926595934,
      0.357584339383878,
      0.1804807884018343,
      0.21263900587151027,
      0.715168678767756,
      0.07219231536073371,
      0.01933081871559182,
      0.11919477979462598,
      0.9505321522496607,
    ];

    let rgb = rgb.resolve_missing();
    let (x, y, z) = multiply_matrix(MATRIX, rgb.r, rgb.g, rgb.b);
    XYZd65 {
      x,
      y,
      z,
      alpha: rgb.alpha,
    }
  }
}

impl From<XYZd65> for P3 {
  fn from(xyz: XYZd65) -> P3 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L105
    const MATRIX: &[f32] = &[
      2.493496911941425,
      -0.9313836179191239,
      -0.40271078445071684,
      -0.8294889695615747,
      1.7626640603183463,
      0.023624685841943577,
      0.03584583024378447,
      -0.07617238926804182,
      0.9568845240076872,
    ];

    let xyz = xyz.resolve_missing();
    let (r, g, b) = multiply_matrix(MATRIX, xyz.x, xyz.y, xyz.z);
    let (r, g, b) = gam_srgb(r, g, b); // same as sRGB
    P3 {
      r,
      g,
      b,
      alpha: xyz.alpha,
    }
  }
}

impl From<P3> for XYZd65 {
  fn from(p3: P3) -> XYZd65 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L91
    // convert linear-light display-p3 values to CIE XYZ
    // using D65 (no chromatic adaptation)
    // http://www.brucelindbloom.com/index.html?Eqn_RGB_XYZ_Matrix.html
    const MATRIX: &[f32] = &[
      0.4865709486482162,
      0.26566769316909306,
      0.1982172852343625,
      0.2289745640697488,
      0.6917385218365064,
      0.079286914093745,
      0.0000000000000000,
      0.04511338185890264,
      1.043944368900976,
    ];

    let p3 = p3.resolve_missing();
    let (r, g, b) = lin_srgb(p3.r, p3.g, p3.b);
    let (x, y, z) = multiply_matrix(MATRIX, r, g, b);
    XYZd65 {
      x,
      y,
      z,
      alpha: p3.alpha,
    }
  }
}

impl From<A98> for XYZd65 {
  fn from(a98: A98) -> XYZd65 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L181
    #[inline]
    fn lin_a98rgb_component(c: f32) -> f32 {
      let sign = if c < 0.0 { -1.0 } else { 1.0 };
      sign * c.abs().powf(563.0 / 256.0)
    }

    // convert an array of a98-rgb values in the range 0.0 - 1.0
    // to linear light (un-companded) form.
    // negative values are also now accepted
    let a98 = a98.resolve_missing();
    let r = lin_a98rgb_component(a98.r);
    let g = lin_a98rgb_component(a98.g);
    let b = lin_a98rgb_component(a98.b);

    // convert an array of linear-light a98-rgb values to CIE XYZ
    // http://www.brucelindbloom.com/index.html?Eqn_RGB_XYZ_Matrix.html
    // has greater numerical precision than section 4.3.5.3 of
    // https://www.adobe.com/digitalimag/pdfs/AdobeRGB1998.pdf
    // but the values below were calculated from first principles
    // from the chromaticity coordinates of R G B W
    // see matrixmaker.html
    const MATRIX: &[f32] = &[
      0.5766690429101305,
      0.1855582379065463,
      0.1882286462349947,
      0.29734497525053605,
      0.6273635662554661,
      0.07529145849399788,
      0.02703136138641234,
      0.07068885253582723,
      0.9913375368376388,
    ];

    let (x, y, z) = multiply_matrix(MATRIX, r, g, b);
    XYZd65 {
      x,
      y,
      z,
      alpha: a98.alpha,
    }
  }
}

impl From<XYZd65> for A98 {
  fn from(xyz: XYZd65) -> A98 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L222
    // convert XYZ to linear-light a98-rgb
    const MATRIX: &[f32] = &[
      2.0415879038107465,
      -0.5650069742788596,
      -0.34473135077832956,
      -0.9692436362808795,
      1.8759675015077202,
      0.04155505740717557,
      0.013444280632031142,
      -0.11836239223101838,
      1.0151749943912054,
    ];

    #[inline]
    fn gam_a98_component(c: f32) -> f32 {
      // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L193
      // convert linear-light a98-rgb  in the range 0.0-1.0
      // to gamma corrected form
      // negative values are also now accepted
      let sign = if c < 0.0 { -1.0 } else { 1.0 };
      sign * c.abs().powf(256.0 / 563.0)
    }

    let xyz = xyz.resolve_missing();
    let (r, g, b) = multiply_matrix(MATRIX, xyz.x, xyz.y, xyz.z);
    let r = gam_a98_component(r);
    let g = gam_a98_component(g);
    let b = gam_a98_component(b);
    A98 {
      r,
      g,
      b,
      alpha: xyz.alpha,
    }
  }
}

impl From<ProPhoto> for XYZd50 {
  fn from(prophoto: ProPhoto) -> XYZd50 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L118
    // convert an array of prophoto-rgb values
    // where in-gamut colors are in the range [0.0 - 1.0]
    // to linear light (un-companded) form.
    // Transfer curve is gamma 1.8 with a small linear portion
    // Extended transfer function

    #[inline]
    fn lin_prophoto_component(c: f32) -> f32 {
      const ET2: f32 = 16.0 / 512.0;
      let abs = c.abs();
      if abs <= ET2 {
        return c / 16.0;
      }

      let sign = if c < 0.0 { -1.0 } else { 1.0 };
      sign * c.powf(1.8)
    }

    let prophoto = prophoto.resolve_missing();
    let r = lin_prophoto_component(prophoto.r);
    let g = lin_prophoto_component(prophoto.g);
    let b = lin_prophoto_component(prophoto.b);

    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L155
    // convert an array of linear-light prophoto-rgb values to CIE XYZ
    // using  D50 (so no chromatic adaptation needed afterwards)
    // http://www.brucelindbloom.com/index.html?Eqn_RGB_XYZ_Matrix.html
    const MATRIX: &[f32] = &[
      0.7977604896723027,
      0.13518583717574031,
      0.0313493495815248,
      0.2880711282292934,
      0.7118432178101014,
      0.00008565396060525902,
      0.0,
      0.0,
      0.8251046025104601,
    ];

    let (x, y, z) = multiply_matrix(MATRIX, r, g, b);
    XYZd50 {
      x,
      y,
      z,
      alpha: prophoto.alpha,
    }
  }
}

impl From<XYZd50> for ProPhoto {
  fn from(xyz: XYZd50) -> ProPhoto {
    // convert XYZ to linear-light prophoto-rgb
    const MATRIX: &[f32] = &[
      1.3457989731028281,
      -0.25558010007997534,
      -0.05110628506753401,
      -0.5446224939028347,
      1.5082327413132781,
      0.02053603239147973,
      0.0,
      0.0,
      1.2119675456389454,
    ];

    #[inline]
    fn gam_prophoto_component(c: f32) -> f32 {
      // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L137
      // convert linear-light prophoto-rgb  in the range 0.0-1.0
      // to gamma corrected form
      // Transfer curve is gamma 1.8 with a small linear portion
      // TODO for negative values, extend linear portion on reflection of axis, then add pow below that
      const ET: f32 = 1.0 / 512.0;
      let abs = c.abs();
      if abs >= ET {
        let sign = if c < 0.0 { -1.0 } else { 1.0 };
        return sign * abs.powf(1.0 / 1.8);
      }

      16.0 * c
    }

    let xyz = xyz.resolve_missing();
    let (r, g, b) = multiply_matrix(MATRIX, xyz.x, xyz.y, xyz.z);
    let r = gam_prophoto_component(r);
    let g = gam_prophoto_component(g);
    let b = gam_prophoto_component(b);
    ProPhoto {
      r,
      g,
      b,
      alpha: xyz.alpha,
    }
  }
}

impl From<Rec2020> for XYZd65 {
  fn from(rec2020: Rec2020) -> XYZd65 {
    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L235
    // convert an array of rec2020 RGB values in the range 0.0 - 1.0
    // to linear light (un-companded) form.
    // ITU-R BT.2020-2 p.4

    #[inline]
    fn lin_rec2020_component(c: f32) -> f32 {
      const A: f32 = 1.09929682680944;
      const B: f32 = 0.018053968510807;

      let abs = c.abs();
      if abs < B * 4.5 {
        return c / 4.5;
      }

      let sign = if c < 0.0 { -1.0 } else { 1.0 };
      sign * ((abs + A - 1.0) / A).powf(1.0 / 0.45)
    }

    let rec2020 = rec2020.resolve_missing();
    let r = lin_rec2020_component(rec2020.r);
    let g = lin_rec2020_component(rec2020.g);
    let b = lin_rec2020_component(rec2020.b);

    // https://github.com/w3c/csswg-drafts/blob/fba005e2ce9bcac55b49e4aa19b87208b3a0631e/css-color-4/conversions.js#L276
    // convert an array of linear-light rec2020 values to CIE XYZ
    // using  D65 (no chromatic adaptation)
    // http://www.brucelindbloom.com/index.html?Eqn_RGB_XYZ_Matrix.html
    const MATRIX: &[f32] = &[
      0.6369580483012914,
      0.14461690358620832,
      0.1688809751641721,
      0.2627002120112671,
      0.6779980715188708,
      0.05930171646986196,
      0.000000000000000,
      0.028072693049087428,
      1.060985057710791,
    ];

    let (x, y, z) = multiply_matrix(MATRIX, r, g, b);
    XYZd65 {
      x,
      y,
      z,
      alpha: rec2020.alpha,
    }
  }
}

impl From<XYZd65> for Rec2020 {
  fn from(xyz: XYZd65) -> Rec2020 {
    // convert XYZ to linear-light rec2020
    const MATRIX: &[f32] = &[
      1.7166511879712674,
      -0.35567078377639233,
      -0.25336628137365974,
      -0.6666843518324892,
      1.6164812366349395,
      0.01576854581391113,
      0.017639857445310783,
      -0.042770613257808524,
      0.9421031212354738,
    ];

    #[inline]
    fn gam_rec2020_component(c: f32) -> f32 {
      // convert linear-light rec2020 RGB  in the range 0.0-1.0
      // to gamma corrected form
      // ITU-R BT.2020-2 p.4

      const A: f32 = 1.09929682680944;
      const B: f32 = 0.018053968510807;

      let abs = c.abs();
      if abs > B {
        let sign = if c < 0.0 { -1.0 } else { 1.0 };
        return sign * (A * abs.powf(0.45) - (A - 1.0));
      }

      4.5 * c
    }

    let xyz = xyz.resolve_missing();
    let (r, g, b) = multiply_matrix(MATRIX, xyz.x, xyz.y, xyz.z);
    let r = gam_rec2020_component(r);
    let g = gam_rec2020_component(g);
    let b = gam_rec2020_component(b);
    Rec2020 {
      r,
      g,
      b,
      alpha: xyz.alpha,
    }
  }
}

impl From<SRGB> for HSL {
  fn from(rgb: SRGB) -> HSL {
    // https://drafts.csswg.org/css-color/#rgb-to-hsl
    let rgb = rgb.resolve();
    let r = rgb.r;
    let g = rgb.g;
    let b = rgb.b;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let mut h = f32::NAN;
    let mut s: f32 = 0.0;
    let l = (min + max) / 2.0;
    let d = max - min;

    if d != 0.0 {
      s = if l == 0.0 || l == 1.0 {
        0.0
      } else {
        (max - l) / l.min(1.0 - l)
      };

      if max == r {
        h = (g - b) / d + (if g < b { 6.0 } else { 0.0 });
      } else if max == g {
        h = (b - r) / d + 2.0;
      } else if max == b {
        h = (r - g) / d + 4.0;
      }

      h = h * 60.0;
    }

    HSL {
      h,
      s,
      l,
      alpha: rgb.alpha,
    }
  }
}

impl From<HSL> for SRGB {
  fn from(hsl: HSL) -> SRGB {
    // https://drafts.csswg.org/css-color/#hsl-to-rgb
    let hsl = hsl.resolve_missing();
    let h = (hsl.h - 360.0 * (hsl.h / 360.0).floor()) / 360.0;
    let (r, g, b) = hsl_to_rgb(h, hsl.s, hsl.l);
    SRGB {
      r,
      g,
      b,
      alpha: hsl.alpha,
    }
  }
}

impl From<SRGB> for HWB {
  fn from(rgb: SRGB) -> HWB {
    let rgb = rgb.resolve();
    let hsl = HSL::from(rgb);
    let r = rgb.r;
    let g = rgb.g;
    let b = rgb.b;
    let w = r.min(g).min(b);
    let b = 1.0 - r.max(g).max(b);
    HWB {
      h: hsl.h,
      w,
      b,
      alpha: rgb.alpha,
    }
  }
}

impl From<HWB> for SRGB {
  fn from(hwb: HWB) -> SRGB {
    // https://drafts.csswg.org/css-color/#hwb-to-rgb
    let hwb = hwb.resolve_missing();
    let h = hwb.h;
    let w = hwb.w;
    let b = hwb.b;

    if w + b >= 1.0 {
      let gray = w / (w + b);
      return SRGB {
        r: gray,
        g: gray,
        b: gray,
        alpha: hwb.alpha,
      };
    }

    let mut rgba = SRGB::from(HSL {
      h,
      s: 1.0,
      l: 0.5,
      alpha: hwb.alpha,
    });
    let x = 1.0 - w - b;
    rgba.r = rgba.r * x + w;
    rgba.g = rgba.g * x + w;
    rgba.b = rgba.b * x + w;
    rgba
  }
}

impl From<RGBA> for SRGB {
  fn from(rgb: RGBA) -> Self {
    Self::from(&rgb)
  }
}

impl From<&RGBA> for SRGB {
  fn from(rgb: &RGBA) -> SRGB {
    SRGB {
      r: rgb.red_f32(),
      g: rgb.green_f32(),
      b: rgb.blue_f32(),
      alpha: rgb.alpha_f32(),
    }
  }
}

impl From<SRGB> for RGBA {
  fn from(rgb: SRGB) -> RGBA {
    let rgb = rgb.resolve();
    RGBA::from_floats(rgb.r, rgb.g, rgb.b, rgb.alpha)
  }
}

// Once Rust specialization is stable, this could be simplified.
via!(LAB -> XYZd50 -> XYZd65);
via!(ProPhoto -> XYZd50 -> XYZd65);
via!(OKLCH -> OKLAB -> XYZd65);

via!(LAB -> XYZd65 -> OKLAB);
via!(LAB -> XYZd65 -> OKLCH);
via!(LAB -> XYZd65 -> SRGB);
via!(LAB -> XYZd65 -> SRGBLinear);
via!(LAB -> XYZd65 -> P3);
via!(LAB -> XYZd65 -> A98);
via!(LAB -> XYZd65 -> ProPhoto);
via!(LAB -> XYZd65 -> Rec2020);
via!(LAB -> XYZd65 -> HSL);
via!(LAB -> XYZd65 -> HWB);

via!(LCH -> LAB -> XYZd65);
via!(LCH -> XYZd65 -> OKLAB);
via!(LCH -> XYZd65 -> OKLCH);
via!(LCH -> XYZd65 -> SRGB);
via!(LCH -> XYZd65 -> SRGBLinear);
via!(LCH -> XYZd65 -> P3);
via!(LCH -> XYZd65 -> A98);
via!(LCH -> XYZd65 -> ProPhoto);
via!(LCH -> XYZd65 -> Rec2020);
via!(LCH -> XYZd65 -> XYZd50);
via!(LCH -> XYZd65 -> HSL);
via!(LCH -> XYZd65 -> HWB);

via!(SRGB -> SRGBLinear -> XYZd65);
via!(SRGB -> XYZd65 -> OKLAB);
via!(SRGB -> XYZd65 -> OKLCH);
via!(SRGB -> XYZd65 -> P3);
via!(SRGB -> XYZd65 -> A98);
via!(SRGB -> XYZd65 -> ProPhoto);
via!(SRGB -> XYZd65 -> Rec2020);
via!(SRGB -> XYZd65 -> XYZd50);

via!(P3 -> XYZd65 -> SRGBLinear);
via!(P3 -> XYZd65 -> OKLAB);
via!(P3 -> XYZd65 -> OKLCH);
via!(P3 -> XYZd65 -> A98);
via!(P3 -> XYZd65 -> ProPhoto);
via!(P3 -> XYZd65 -> Rec2020);
via!(P3 -> XYZd65 -> XYZd50);
via!(P3 -> XYZd65 -> HSL);
via!(P3 -> XYZd65 -> HWB);

via!(SRGBLinear -> XYZd65 -> OKLAB);
via!(SRGBLinear -> XYZd65 -> OKLCH);
via!(SRGBLinear -> XYZd65 -> A98);
via!(SRGBLinear -> XYZd65 -> ProPhoto);
via!(SRGBLinear -> XYZd65 -> Rec2020);
via!(SRGBLinear -> XYZd65 -> XYZd50);
via!(SRGBLinear -> XYZd65 -> HSL);
via!(SRGBLinear -> XYZd65 -> HWB);

via!(A98 -> XYZd65 -> OKLAB);
via!(A98 -> XYZd65 -> OKLCH);
via!(A98 -> XYZd65 -> ProPhoto);
via!(A98 -> XYZd65 -> Rec2020);
via!(A98 -> XYZd65 -> XYZd50);
via!(A98 -> XYZd65 -> HSL);
via!(A98 -> XYZd65 -> HWB);

via!(ProPhoto -> XYZd65 -> OKLAB);
via!(ProPhoto -> XYZd65 -> OKLCH);
via!(ProPhoto -> XYZd65 -> Rec2020);
via!(ProPhoto -> XYZd65 -> HSL);
via!(ProPhoto -> XYZd65 -> HWB);

via!(XYZd50 -> XYZd65 -> OKLAB);
via!(XYZd50 -> XYZd65 -> OKLCH);
via!(XYZd50 -> XYZd65 -> Rec2020);
via!(XYZd50 -> XYZd65 -> HSL);
via!(XYZd50 -> XYZd65 -> HWB);

via!(Rec2020 -> XYZd65 -> OKLAB);
via!(Rec2020 -> XYZd65 -> OKLCH);
via!(Rec2020 -> XYZd65 -> HSL);
via!(Rec2020 -> XYZd65 -> HWB);

via!(HSL -> XYZd65 -> OKLAB);
via!(HSL -> XYZd65 -> OKLCH);
via!(HSL -> SRGB -> XYZd65);
via!(HSL -> SRGB -> HWB);

via!(HWB -> SRGB -> XYZd65);
via!(HWB -> XYZd65 -> OKLAB);
via!(HWB -> XYZd65 -> OKLCH);

// RGBA is an 8-bit version. Convert to SRGB, which is a
// more accurate floating point representation for all operations.
via!(RGBA -> SRGB -> LAB);
via!(RGBA -> SRGB -> LCH);
via!(RGBA -> SRGB -> OKLAB);
via!(RGBA -> SRGB -> OKLCH);
via!(RGBA -> SRGB -> P3);
via!(RGBA -> SRGB -> SRGBLinear);
via!(RGBA -> SRGB -> A98);
via!(RGBA -> SRGB -> ProPhoto);
via!(RGBA -> SRGB -> XYZd50);
via!(RGBA -> SRGB -> XYZd65);
via!(RGBA -> SRGB -> Rec2020);
via!(RGBA -> SRGB -> HSL);
via!(RGBA -> SRGB -> HWB);

macro_rules! color_space {
  ($space: ty) => {
    impl From<LABColor> for $space {
      fn from(color: LABColor) -> $space {
        use LABColor::*;

        match color {
          LAB(v) => v.into(),
          LCH(v) => v.into(),
          OKLAB(v) => v.into(),
          OKLCH(v) => v.into(),
        }
      }
    }

    impl From<PredefinedColor> for $space {
      fn from(color: PredefinedColor) -> $space {
        use PredefinedColor::*;

        match color {
          SRGB(v) => v.into(),
          SRGBLinear(v) => v.into(),
          DisplayP3(v) => v.into(),
          A98(v) => v.into(),
          ProPhoto(v) => v.into(),
          Rec2020(v) => v.into(),
          XYZd50(v) => v.into(),
          XYZd65(v) => v.into(),
        }
      }
    }

    impl From<FloatColor> for $space {
      fn from(color: FloatColor) -> $space {
        use FloatColor::*;

        match color {
          RGB(v) => v.into(),
          HSL(v) => v.into(),
          HWB(v) => v.into(),
        }
      }
    }

    impl TryFrom<&CssColor> for $space {
      type Error = ();
      fn try_from(color: &CssColor) -> Result<$space, ()> {
        Ok(match color {
          CssColor::RGBA(rgba) => (*rgba).into(),
          CssColor::LAB(lab) => (**lab).into(),
          CssColor::Predefined(predefined) => (**predefined).into(),
          CssColor::Float(float) => (**float).into(),
          CssColor::CurrentColor => return Err(()),
          CssColor::LightDark(..) => return Err(()),
          CssColor::System(..) => return Err(()),
        })
      }
    }

    impl TryFrom<CssColor> for $space {
      type Error = ();
      fn try_from(color: CssColor) -> Result<$space, ()> {
        Ok(match color {
          CssColor::RGBA(rgba) => rgba.into(),
          CssColor::LAB(lab) => (*lab).into(),
          CssColor::Predefined(predefined) => (*predefined).into(),
          CssColor::Float(float) => (*float).into(),
          CssColor::CurrentColor => return Err(()),
          CssColor::LightDark(..) => return Err(()),
          CssColor::System(..) => return Err(()),
        })
      }
    }
  };
}

color_space!(LAB);
color_space!(LCH);
color_space!(OKLAB);
color_space!(OKLCH);
color_space!(SRGB);
color_space!(SRGBLinear);
color_space!(XYZd50);
color_space!(XYZd65);
color_space!(P3);
color_space!(A98);
color_space!(ProPhoto);
color_space!(Rec2020);
color_space!(HSL);
color_space!(HWB);
color_space!(RGBA);

macro_rules! predefined {
  ($key: ident, $t: ty) => {
    impl From<$t> for PredefinedColor {
      fn from(color: $t) -> PredefinedColor {
        PredefinedColor::$key(color)
      }
    }

    impl From<$t> for CssColor {
      fn from(color: $t) -> CssColor {
        CssColor::Predefined(Box::new(PredefinedColor::$key(color)))
      }
    }
  };
}

predefined!(SRGBLinear, SRGBLinear);
predefined!(XYZd50, XYZd50);
predefined!(XYZd65, XYZd65);
predefined!(DisplayP3, P3);
predefined!(A98, A98);
predefined!(ProPhoto, ProPhoto);
predefined!(Rec2020, Rec2020);

macro_rules! lab {
  ($key: ident, $t: ty) => {
    impl From<$t> for LABColor {
      fn from(color: $t) -> LABColor {
        LABColor::$key(color)
      }
    }

    impl From<$t> for CssColor {
      fn from(color: $t) -> CssColor {
        CssColor::LAB(Box::new(LABColor::$key(color)))
      }
    }
  };
}

lab!(LAB, LAB);
lab!(LCH, LCH);
lab!(OKLAB, OKLAB);
lab!(OKLCH, OKLCH);

macro_rules! rgb {
  ($t: ty) => {
    impl From<$t> for CssColor {
      fn from(color: $t) -> CssColor {
        // TODO: should we serialize as color(srgb, ...)?
        // would be more precise than 8-bit color.
        CssColor::RGBA(color.into())
      }
    }
  };
}

rgb!(SRGB);
rgb!(HSL);
rgb!(HWB);

impl From<RGBA> for CssColor {
  fn from(color: RGBA) -> CssColor {
    CssColor::RGBA(color)
  }
}

/// A trait that colors implement to support [gamut mapping](https://www.w3.org/TR/css-color-4/#gamut-mapping).
pub trait ColorGamut {
  /// Returns whether the color is within the gamut of the color space.
  fn in_gamut(&self) -> bool;
  /// Clips the color so that it is within the gamut of the color space.
  fn clip(&self) -> Self;
}

macro_rules! bounded_color_gamut {
  ($t: ty, $a: ident, $b: ident, $c: ident) => {
    impl ColorGamut for $t {
      #[inline]
      fn in_gamut(&self) -> bool {
        self.$a >= 0.0 && self.$a <= 1.0 && self.$b >= 0.0 && self.$b <= 1.0 && self.$c >= 0.0 && self.$c <= 1.0
      }

      #[inline]
      fn clip(&self) -> Self {
        Self {
          $a: self.$a.clamp(0.0, 1.0),
          $b: self.$b.clamp(0.0, 1.0),
          $c: self.$c.clamp(0.0, 1.0),
          alpha: self.alpha.clamp(0.0, 1.0),
        }
      }
    }
  };
}

macro_rules! unbounded_color_gamut {
  ($t: ty, $a: ident, $b: ident, $c: ident) => {
    impl ColorGamut for $t {
      #[inline]
      fn in_gamut(&self) -> bool {
        true
      }

      #[inline]
      fn clip(&self) -> Self {
        *self
      }
    }
  };
}

macro_rules! hsl_hwb_color_gamut {
  ($t: ty, $a: ident, $b: ident) => {
    impl ColorGamut for $t {
      #[inline]
      fn in_gamut(&self) -> bool {
        self.$a >= 0.0 && self.$a <= 1.0 && self.$b >= 0.0 && self.$b <= 1.0
      }

      #[inline]
      fn clip(&self) -> Self {
        Self {
          h: self.h % 360.0,
          $a: self.$a.clamp(0.0, 1.0),
          $b: self.$b.clamp(0.0, 1.0),
          alpha: self.alpha.clamp(0.0, 1.0),
        }
      }
    }
  };
}

bounded_color_gamut!(SRGB, r, g, b);
bounded_color_gamut!(SRGBLinear, r, g, b);
bounded_color_gamut!(P3, r, g, b);
bounded_color_gamut!(A98, r, g, b);
bounded_color_gamut!(ProPhoto, r, g, b);
bounded_color_gamut!(Rec2020, r, g, b);
unbounded_color_gamut!(LAB, l, a, b);
unbounded_color_gamut!(OKLAB, l, a, b);
unbounded_color_gamut!(XYZd50, x, y, z);
unbounded_color_gamut!(XYZd65, x, y, z);
unbounded_color_gamut!(LCH, l, c, h);
unbounded_color_gamut!(OKLCH, l, c, h);
hsl_hwb_color_gamut!(HSL, s, l);
hsl_hwb_color_gamut!(HWB, w, b);

fn delta_eok<T: Into<OKLAB>>(a: T, b: OKLCH) -> f32 {
  // https://www.w3.org/TR/css-color-4/#color-difference-OK
  let a: OKLAB = a.into();
  let b: OKLAB = b.into();
  let delta_l = a.l - b.l;
  let delta_a = a.a - b.a;
  let delta_b = a.b - b.b;

  (delta_l.powi(2) + delta_a.powi(2) + delta_b.powi(2)).sqrt()
}

fn map_gamut<T>(color: T) -> T
where
  T: Into<OKLCH> + ColorGamut + Into<OKLAB> + From<OKLCH> + Copy,
{
  const JND: f32 = 0.02;
  const EPSILON: f32 = 0.00001;

  // https://www.w3.org/TR/css-color-4/#binsearch
  let mut current: OKLCH = color.into();

  // If lightness is >= 100%, return pure white.
  if (current.l - 1.0).abs() < EPSILON || current.l > 1.0 {
    return OKLCH {
      l: 1.0,
      c: 0.0,
      h: 0.0,
      alpha: current.alpha,
    }
    .into();
  }

  // If lightness <= 0%, return pure black.
  if current.l < EPSILON {
    return OKLCH {
      l: 0.0,
      c: 0.0,
      h: 0.0,
      alpha: current.alpha,
    }
    .into();
  }

  let mut min = 0.0;
  let mut max = current.c;

  while (max - min) > EPSILON {
    let chroma = (min + max) / 2.0;
    current.c = chroma;

    let converted = T::from(current);
    if converted.in_gamut() {
      min = chroma;
      continue;
    }

    let clipped = converted.clip();
    let delta_e = delta_eok(clipped, current);
    if delta_e < JND {
      return clipped;
    }

    max = chroma;
  }

  current.into()
}

fn parse_color_mix<'i, 't>(input: &mut Parser<'i, 't>) -> Result<CssColor, ParseError<'i, ParserError<'i>>> {
  input.expect_ident_matching("in")?;
  let method = ColorSpaceName::parse(input)?;

  let hue_method = if matches!(
    method,
    ColorSpaceName::Hsl | ColorSpaceName::Hwb | ColorSpaceName::LCH | ColorSpaceName::OKLCH
  ) {
    let hue_method = input.try_parse(HueInterpolationMethod::parse);
    if hue_method.is_ok() {
      input.expect_ident_matching("hue")?;
    }
    hue_method
  } else {
    Ok(HueInterpolationMethod::Shorter)
  };

  let hue_method = hue_method.unwrap_or(HueInterpolationMethod::Shorter);
  input.expect_comma()?;

  let first_percent = input.try_parse(|input| input.expect_percentage());
  let first_color = CssColor::parse(input)?;
  let first_percent = first_percent
    .or_else(|_| input.try_parse(|input| input.expect_percentage()))
    .ok();
  input.expect_comma()?;

  let second_percent = input.try_parse(|input| input.expect_percentage());
  let second_color = CssColor::parse(input)?;
  let second_percent = second_percent
    .or_else(|_| input.try_parse(|input| input.expect_percentage()))
    .ok();

  // https://drafts.csswg.org/css-color-5/#color-mix-percent-norm
  let (p1, p2) = if first_percent.is_none() && second_percent.is_none() {
    (0.5, 0.5)
  } else {
    let p2 = second_percent.unwrap_or_else(|| 1.0 - first_percent.unwrap());
    let p1 = first_percent.unwrap_or_else(|| 1.0 - second_percent.unwrap());
    (p1, p2)
  };

  if (p1 + p2) == 0.0 {
    return Err(input.new_custom_error(ParserError::InvalidValue));
  }

  match method {
    ColorSpaceName::SRGB => first_color.interpolate::<SRGB>(p1, &second_color, p2, hue_method),
    ColorSpaceName::SRGBLinear => first_color.interpolate::<SRGBLinear>(p1, &second_color, p2, hue_method),
    ColorSpaceName::Hsl => first_color.interpolate::<HSL>(p1, &second_color, p2, hue_method),
    ColorSpaceName::Hwb => first_color.interpolate::<HWB>(p1, &second_color, p2, hue_method),
    ColorSpaceName::LAB => first_color.interpolate::<LAB>(p1, &second_color, p2, hue_method),
    ColorSpaceName::LCH => first_color.interpolate::<LCH>(p1, &second_color, p2, hue_method),
    ColorSpaceName::OKLAB => first_color.interpolate::<OKLAB>(p1, &second_color, p2, hue_method),
    ColorSpaceName::OKLCH => first_color.interpolate::<OKLCH>(p1, &second_color, p2, hue_method),
    ColorSpaceName::XYZ | ColorSpaceName::XYZd65 => {
      first_color.interpolate::<XYZd65>(p1, &second_color, p2, hue_method)
    }
    ColorSpaceName::XYZd50 => first_color.interpolate::<XYZd50>(p1, &second_color, p2, hue_method),
  }
  .map_err(|_| input.new_custom_error(ParserError::InvalidValue))
}

impl CssColor {
  fn get_type_id(&self) -> TypeId {
    match self {
      CssColor::RGBA(..) => TypeId::of::<SRGB>(),
      CssColor::LAB(lab) => match &**lab {
        LABColor::LAB(..) => TypeId::of::<LAB>(),
        LABColor::LCH(..) => TypeId::of::<LCH>(),
        LABColor::OKLAB(..) => TypeId::of::<OKLAB>(),
        LABColor::OKLCH(..) => TypeId::of::<OKLCH>(),
      },
      CssColor::Predefined(predefined) => match &**predefined {
        PredefinedColor::SRGB(..) => TypeId::of::<SRGB>(),
        PredefinedColor::SRGBLinear(..) => TypeId::of::<SRGBLinear>(),
        PredefinedColor::DisplayP3(..) => TypeId::of::<P3>(),
        PredefinedColor::A98(..) => TypeId::of::<A98>(),
        PredefinedColor::ProPhoto(..) => TypeId::of::<ProPhoto>(),
        PredefinedColor::Rec2020(..) => TypeId::of::<Rec2020>(),
        PredefinedColor::XYZd50(..) => TypeId::of::<XYZd50>(),
        PredefinedColor::XYZd65(..) => TypeId::of::<XYZd65>(),
      },
      CssColor::Float(float) => match &**float {
        FloatColor::RGB(..) => TypeId::of::<SRGB>(),
        FloatColor::HSL(..) => TypeId::of::<HSL>(),
        FloatColor::HWB(..) => TypeId::of::<HWB>(),
      },
      _ => unreachable!(),
    }
  }

  fn to_light_dark(&self) -> CssColor {
    match self {
      CssColor::LightDark(..) => self.clone(),
      _ => CssColor::LightDark(Box::new(self.clone()), Box::new(self.clone())),
    }
  }

  /// Mixes this color with another color, including the specified amount of each.
  /// Implemented according to the [`color-mix()`](https://www.w3.org/TR/css-color-5/#color-mix) function.
  pub fn interpolate<T>(
    &self,
    mut p1: f32,
    other: &CssColor,
    mut p2: f32,
    method: HueInterpolationMethod,
  ) -> Result<CssColor, ()>
  where
    for<'a> T: 'static
      + TryFrom<&'a CssColor>
      + Interpolate
      + Into<CssColor>
      + Into<OKLCH>
      + ColorGamut
      + Into<OKLAB>
      + From<OKLCH>
      + Copy,
  {
    if matches!(self, CssColor::CurrentColor | CssColor::System(..))
      || matches!(other, CssColor::CurrentColor | CssColor::System(..))
    {
      return Err(());
    }

    if matches!(self, CssColor::LightDark(..)) || matches!(other, CssColor::LightDark(..)) {
      if let (CssColor::LightDark(al, ad), CssColor::LightDark(bl, bd)) =
        (self.to_light_dark(), other.to_light_dark())
      {
        return Ok(CssColor::LightDark(
          Box::new(al.interpolate::<T>(p1, &bl, p2, method)?),
          Box::new(ad.interpolate::<T>(p1, &bd, p2, method)?),
        ));
      }
    }

    let type_id = TypeId::of::<T>();
    let converted_first = self.get_type_id() != type_id;
    let converted_second = other.get_type_id() != type_id;

    // https://drafts.csswg.org/css-color-5/#color-mix-result
    let mut first_color = T::try_from(self).map_err(|_| ())?;
    let mut second_color = T::try_from(other).map_err(|_| ())?;

    if converted_first && !first_color.in_gamut() {
      first_color = map_gamut(first_color);
    }

    if converted_second && !second_color.in_gamut() {
      second_color = map_gamut(second_color);
    }

    // https://www.w3.org/TR/css-color-4/#powerless
    if converted_first {
      first_color.adjust_powerless_components();
    }

    if converted_second {
      second_color.adjust_powerless_components();
    }

    // https://drafts.csswg.org/css-color-4/#interpolation-missing
    first_color.fill_missing_components(&second_color);
    second_color.fill_missing_components(&first_color);

    // https://www.w3.org/TR/css-color-4/#hue-interpolation
    first_color.adjust_hue(&mut second_color, method);

    // https://www.w3.org/TR/css-color-4/#interpolation-alpha
    first_color.premultiply();
    second_color.premultiply();

    // https://drafts.csswg.org/css-color-5/#color-mix-percent-norm
    let mut alpha_multiplier = p1 + p2;
    if alpha_multiplier != 1.0 {
      p1 = p1 / alpha_multiplier;
      p2 = p2 / alpha_multiplier;
      if alpha_multiplier > 1.0 {
        alpha_multiplier = 1.0;
      }
    }

    let mut result_color = first_color.interpolate(p1, &second_color, p2);
    result_color.unpremultiply(alpha_multiplier);

    Ok(result_color.into())
  }
}

/// A trait that colors implement to support interpolation.
pub trait Interpolate {
  /// Adjusts components that are powerless to be NaN.
  fn adjust_powerless_components(&mut self) {}
  /// Fills missing components (represented as NaN) to match the other color to interpolate with.
  fn fill_missing_components(&mut self, other: &Self);
  /// Adjusts the color hue according to the given hue interpolation method.
  fn adjust_hue(&mut self, _: &mut Self, _: HueInterpolationMethod) {}
  /// Premultiplies the color by its alpha value.
  fn premultiply(&mut self);
  /// Un-premultiplies the color by the given alpha multiplier.
  fn unpremultiply(&mut self, alpha_multiplier: f32);
  /// Interpolates the color with another using the given amounts of each.
  fn interpolate(&self, p1: f32, other: &Self, p2: f32) -> Self;
}

macro_rules! interpolate {
  ($a: ident, $b: ident, $c: ident) => {
    fn fill_missing_components(&mut self, other: &Self) {
      if self.$a.is_nan() {
        self.$a = other.$a;
      }

      if self.$b.is_nan() {
        self.$b = other.$b;
      }

      if self.$c.is_nan() {
        self.$c = other.$c;
      }

      if self.alpha.is_nan() {
        self.alpha = other.alpha;
      }
    }

    fn interpolate(&self, p1: f32, other: &Self, p2: f32) -> Self {
      Self {
        $a: self.$a * p1 + other.$a * p2,
        $b: self.$b * p1 + other.$b * p2,
        $c: self.$c * p1 + other.$c * p2,
        alpha: self.alpha * p1 + other.alpha * p2,
      }
    }
  };
}

macro_rules! rectangular_premultiply {
  ($a: ident, $b: ident, $c: ident) => {
    fn premultiply(&mut self) {
      if !self.alpha.is_nan() {
        self.$a *= self.alpha;
        self.$b *= self.alpha;
        self.$c *= self.alpha;
      }
    }

    fn unpremultiply(&mut self, alpha_multiplier: f32) {
      if !self.alpha.is_nan() && self.alpha != 0.0 {
        self.$a /= self.alpha;
        self.$b /= self.alpha;
        self.$c /= self.alpha;
        self.alpha *= alpha_multiplier;
      }
    }
  };
}

macro_rules! polar_premultiply {
  ($a: ident, $b: ident) => {
    fn premultiply(&mut self) {
      if !self.alpha.is_nan() {
        self.$a *= self.alpha;
        self.$b *= self.alpha;
      }
    }

    fn unpremultiply(&mut self, alpha_multiplier: f32) {
      self.h %= 360.0;
      if !self.alpha.is_nan() {
        self.$a /= self.alpha;
        self.$b /= self.alpha;
        self.alpha *= alpha_multiplier;
      }
    }
  };
}

macro_rules! adjust_powerless_lab {
  () => {
    fn adjust_powerless_components(&mut self) {
      // If the lightness of a LAB color is 0%, both the a and b components are powerless.
      if self.l.abs() < f32::EPSILON {
        self.a = f32::NAN;
        self.b = f32::NAN;
      }
    }
  };
}

macro_rules! adjust_powerless_lch {
  () => {
    fn adjust_powerless_components(&mut self) {
      // If the chroma of an LCH color is 0%, the hue component is powerless.
      // If the lightness of an LCH color is 0%, both the hue and chroma components are powerless.
      if self.c.abs() < f32::EPSILON {
        self.h = f32::NAN;
      }

      if self.l.abs() < f32::EPSILON {
        self.c = f32::NAN;
        self.h = f32::NAN;
      }
    }

    fn adjust_hue(&mut self, other: &mut Self, method: HueInterpolationMethod) {
      method.interpolate(&mut self.h, &mut other.h);
    }
  };
}

impl Interpolate for SRGB {
  rectangular_premultiply!(r, g, b);
  interpolate!(r, g, b);
}

impl Interpolate for SRGBLinear {
  rectangular_premultiply!(r, g, b);
  interpolate!(r, g, b);
}

impl Interpolate for XYZd50 {
  rectangular_premultiply!(x, y, z);
  interpolate!(x, y, z);
}

impl Interpolate for XYZd65 {
  rectangular_premultiply!(x, y, z);
  interpolate!(x, y, z);
}

impl Interpolate for LAB {
  adjust_powerless_lab!();
  rectangular_premultiply!(l, a, b);
  interpolate!(l, a, b);
}

impl Interpolate for OKLAB {
  adjust_powerless_lab!();
  rectangular_premultiply!(l, a, b);
  interpolate!(l, a, b);
}

impl Interpolate for LCH {
  adjust_powerless_lch!();
  polar_premultiply!(l, c);
  interpolate!(l, c, h);
}

impl Interpolate for OKLCH {
  adjust_powerless_lch!();
  polar_premultiply!(l, c);
  interpolate!(l, c, h);
}

impl Interpolate for HSL {
  polar_premultiply!(s, l);

  fn adjust_powerless_components(&mut self) {
    // If the saturation of an HSL color is 0%, then the hue component is powerless.
    // If the lightness of an HSL color is 0% or 100%, both the saturation and hue components are powerless.
    if self.s.abs() < f32::EPSILON {
      self.h = f32::NAN;
    }

    if self.l.abs() < f32::EPSILON || (self.l - 1.0).abs() < f32::EPSILON {
      self.h = f32::NAN;
      self.s = f32::NAN;
    }
  }

  fn adjust_hue(&mut self, other: &mut Self, method: HueInterpolationMethod) {
    method.interpolate(&mut self.h, &mut other.h);
  }

  interpolate!(h, s, l);
}

impl Interpolate for HWB {
  polar_premultiply!(w, b);

  fn adjust_powerless_components(&mut self) {
    // If white+black is equal to 100% (after normalization), it defines an achromatic color,
    // i.e. some shade of gray, without any hint of the chosen hue. In this case, the hue component is powerless.
    if (self.w + self.b - 1.0).abs() < f32::EPSILON {
      self.h = f32::NAN;
    }
  }

  fn adjust_hue(&mut self, other: &mut Self, method: HueInterpolationMethod) {
    method.interpolate(&mut self.h, &mut other.h);
  }

  interpolate!(h, w, b);
}

impl HueInterpolationMethod {
  fn interpolate(&self, a: &mut f32, b: &mut f32) {
    // https://drafts.csswg.org/css-color/#hue-interpolation
    if *self != HueInterpolationMethod::Specified {
      *a = ((*a % 360.0) + 360.0) % 360.0;
      *b = ((*b % 360.0) + 360.0) % 360.0;
    }

    match self {
      HueInterpolationMethod::Shorter => {
        // https://www.w3.org/TR/css-color-4/#hue-shorter
        let delta = *b - *a;
        if delta > 180.0 {
          *a += 360.0;
        } else if delta < -180.0 {
          *b += 360.0;
        }
      }
      HueInterpolationMethod::Longer => {
        // https://www.w3.org/TR/css-color-4/#hue-longer
        let delta = *b - *a;
        if 0.0 < delta && delta < 180.0 {
          *a += 360.0;
        } else if -180.0 < delta && delta < 0.0 {
          *b += 360.0;
        }
      }
      HueInterpolationMethod::Increasing => {
        // https://www.w3.org/TR/css-color-4/#hue-increasing
        if *b < *a {
          *b += 360.0;
        }
      }
      HueInterpolationMethod::Decreasing => {
        // https://www.w3.org/TR/css-color-4/#hue-decreasing
        if *a < *b {
          *a += 360.0;
        }
      }
      HueInterpolationMethod::Specified => {}
    }
  }
}

#[cfg(feature = "visitor")]
#[cfg_attr(docsrs, doc(cfg(feature = "visitor")))]
impl<'i, V: ?Sized + Visitor<'i, T>, T: Visit<'i, T, V>> Visit<'i, T, V> for RGBA {
  const CHILD_TYPES: VisitTypes = VisitTypes::empty();
  fn visit_children(&mut self, _: &mut V) -> Result<(), V::Error> {
    Ok(())
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Parse, ToCss)]
#[css(case = lower)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(rename_all = "lowercase")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
/// A CSS [system color](https://drafts.csswg.org/css-color/#css-system-colors) keyword.
pub enum SystemColor {
  /// Background of accented user interface controls.
  AccentColor,
  /// Text of accented user interface controls.
  AccentColorText,
  /// Text in active links. For light backgrounds, traditionally red.
  ActiveText,
  /// The base border color for push buttons.
  ButtonBorder,
  /// The face background color for push buttons.
  ButtonFace,
  /// Text on push buttons.
  ButtonText,
  /// Background of application content or documents.
  Canvas,
  /// Text in application content or documents.
  CanvasText,
  /// Background of input fields.
  Field,
  /// Text in input fields.
  FieldText,
  /// Disabled text. (Often, but not necessarily, gray.)
  GrayText,
  /// Background of selected text, for example from ::selection.
  Highlight,
  /// Text of selected text.
  HighlightText,
  /// Text in non-active, non-visited links. For light backgrounds, traditionally blue.
  LinkText,
  /// Background of text that has been specially marked (such as by the HTML mark element).
  Mark,
  /// Text that has been specially marked (such as by the HTML mark element).
  MarkText,
  /// Background of selected items, for example a selected checkbox.
  SelectedItem,
  /// Text of selected items.
  SelectedItemText,
  /// Text in visited links. For light backgrounds, traditionally purple.
  VisitedText,

  // Deprecated colors: https://drafts.csswg.org/css-color/#deprecated-system-colors
  /// Active window border. Same as ButtonBorder.
  ActiveBorder,
  /// Active window caption. Same as Canvas.
  ActiveCaption,
  /// Background color of multiple document interface. Same as Canvas.
  AppWorkspace,
  /// Desktop background. Same as Canvas.
  Background,
  /// The color of the border facing the light source for 3-D elements that appear 3-D due to one layer of surrounding border. Same as ButtonFace.
  ButtonHighlight,
  /// The color of the border away from the light source for 3-D elements that appear 3-D due to one layer of surrounding border. Same as ButtonFace.
  ButtonShadow,
  /// Text in caption, size box, and scrollbar arrow box. Same as CanvasText.
  CaptionText,
  /// Inactive window border. Same as ButtonBorder.
  InactiveBorder,
  /// Inactive window caption. Same as Canvas.
  InactiveCaption,
  /// Color of text in an inactive caption. Same as GrayText.
  InactiveCaptionText,
  /// Background color for tooltip controls. Same as Canvas.
  InfoBackground,
  /// Text color for tooltip controls. Same as CanvasText.
  InfoText,
  /// Menu background. Same as Canvas.
  Menu,
  /// Text in menus. Same as CanvasText.
  MenuText,
  /// Scroll bar gray area. Same as Canvas.
  Scrollbar,
  /// The color of the darker (generally outer) of the two borders away from the light source for 3-D elements that appear 3-D due to two concentric layers of surrounding border. Same as ButtonBorder.
  ThreeDDarkShadow,
  /// The face background color for 3-D elements that appear 3-D due to two concentric layers of surrounding border. Same as ButtonFace.
  ThreeDFace,
  /// The color of the lighter (generally outer) of the two borders facing the light source for 3-D elements that appear 3-D due to two concentric layers of surrounding border. Same as ButtonBorder.
  ThreeDHighlight,
  /// The color of the darker (generally inner) of the two borders facing the light source for 3-D elements that appear 3-D due to two concentric layers of surrounding border. Same as ButtonBorder.
  ThreeDLightShadow,
  /// The color of the lighter (generally inner) of the two borders away from the light source for 3-D elements that appear 3-D due to two concentric layers of surrounding border. Same as ButtonBorder.
  ThreeDShadow,
  /// Window background. Same as Canvas.
  Window,
  /// Window frame. Same as ButtonBorder.
  WindowFrame,
  /// Text in windows. Same as CanvasText.
  WindowText,
}

impl IsCompatible for SystemColor {
  fn is_compatible(&self, browsers: Browsers) -> bool {
    use SystemColor::*;
    match self {
      AccentColor | AccentColorText => Feature::AccentSystemColor.is_compatible(browsers),
      _ => true,
    }
  }
}
