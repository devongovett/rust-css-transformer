//! The `@font-palette-values` rule.

use super::supports::SupportsRule;
use super::{CssRule, CssRuleList, Location, MinifyContext};
use crate::error::{ParserError, PrinterError};
use crate::printer::Printer;
use crate::properties::custom::CustomProperty;
use crate::properties::font::FontFamily;
use crate::stylesheet::ParserOptions;
use crate::targets::Targets;
use crate::traits::{Parse, ToCss};
use crate::values::color::{ColorFallbackKind, CssColor};
use crate::values::ident::DashedIdent;
use crate::values::number::CSSInteger;
#[cfg(feature = "visitor")]
use crate::visitor::Visit;
use cssparser::*;

/// A [@font-palette-values](https://drafts.csswg.org/css-fonts-4/#font-palette-values) rule.
#[derive(Debug, PartialEq, Clone)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct FontPaletteValuesRule<'i> {
  /// The name of the font palette.
  pub name: DashedIdent<'i>,
  /// Declarations in the `@font-palette-values` rule.
  #[cfg_attr(feature = "serde", serde(borrow))]
  pub properties: Vec<FontPaletteValuesProperty<'i>>,
  /// The location of the rule in the source file.
  #[cfg_attr(feature = "visitor", skip_visit)]
  pub loc: Location,
}

/// A property within an `@font-palette-values` rule.
///
///  See [FontPaletteValuesRule](FontPaletteValuesRule).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum FontPaletteValuesProperty<'i> {
  /// The `font-family` property.
  #[cfg_attr(feature = "serde", serde(borrow))]
  FontFamily(FontFamily<'i>),
  /// The `base-palette` property.
  BasePalette(BasePalette),
  /// The `override-colors` property.
  OverrideColors(Vec<OverrideColors>),
  /// An unknown or unsupported property.
  Custom(CustomProperty<'i>),
}

/// A value for the [base-palette](https://drafts.csswg.org/css-fonts-4/#base-palette-desc)
/// property in an `@font-palette-values` rule.
#[derive(Debug, PartialEq, Clone)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub enum BasePalette {
  /// A light color palette as defined within the font.
  Light,
  /// A dark color palette as defined within the font.
  Dark,
  /// A palette index within the font.
  Integer(u16),
}

/// A value for the [override-colors](https://drafts.csswg.org/css-fonts-4/#override-color)
/// property in an `@font-palette-values` rule.
#[derive(Debug, PartialEq, Clone)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct OverrideColors {
  /// The index of the color within the palette to override.
  index: u16,
  /// The replacement color.
  color: CssColor,
}

pub(crate) struct FontPaletteValuesDeclarationParser;

impl<'i> cssparser::DeclarationParser<'i> for FontPaletteValuesDeclarationParser {
  type Declaration = FontPaletteValuesProperty<'i>;
  type Error = ParserError<'i>;

  fn parse_value<'t>(
    &mut self,
    name: CowRcStr<'i>,
    input: &mut cssparser::Parser<'i, 't>,
  ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
    let state = input.state();
    match_ignore_ascii_case! { &name,
      "font-family" => {
        // https://drafts.csswg.org/css-fonts-4/#font-family-2-desc
        if let Ok(font_family) = FontFamily::parse(input) {
          return match font_family {
            FontFamily::Generic(_) => Err(input.new_custom_error(ParserError::InvalidDeclaration)),
            _ => Ok(FontPaletteValuesProperty::FontFamily(font_family))
          }
        }
      },
      "base-palette" => {
        // https://drafts.csswg.org/css-fonts-4/#base-palette-desc
        if let Ok(base_palette) = BasePalette::parse(input) {
          return Ok(FontPaletteValuesProperty::BasePalette(base_palette))
        }
      },
      "override-colors" => {
        // https://drafts.csswg.org/css-fonts-4/#override-color
        if let Ok(override_colors) = input.parse_comma_separated(OverrideColors::parse) {
          return Ok(FontPaletteValuesProperty::OverrideColors(override_colors))
        }
      },
      _ => return Err(input.new_custom_error(ParserError::InvalidDeclaration))
    }

    input.reset(&state);
    return Ok(FontPaletteValuesProperty::Custom(CustomProperty::parse(
      name.into(),
      input,
      &ParserOptions::default(),
    )?));
  }
}

/// Default methods reject all at rules.
impl<'i> AtRuleParser<'i> for FontPaletteValuesDeclarationParser {
  type Prelude = ();
  type AtRule = FontPaletteValuesProperty<'i>;
  type Error = ParserError<'i>;
}

impl<'i> QualifiedRuleParser<'i> for FontPaletteValuesDeclarationParser {
  type Prelude = ();
  type QualifiedRule = FontPaletteValuesProperty<'i>;
  type Error = ParserError<'i>;
}

impl<'i> RuleBodyItemParser<'i, FontPaletteValuesProperty<'i>, ParserError<'i>>
  for FontPaletteValuesDeclarationParser
{
  fn parse_qualified(&self) -> bool {
    false
  }

  fn parse_declarations(&self) -> bool {
    true
  }
}

impl<'i> FontPaletteValuesRule<'i> {
  pub(crate) fn parse<'t>(
    name: DashedIdent<'i>,
    input: &mut Parser<'i, 't>,
    loc: Location,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let mut decl_parser = FontPaletteValuesDeclarationParser;
    let mut parser = RuleBodyParser::new(input, &mut decl_parser);
    let mut properties = vec![];
    while let Some(decl) = parser.next() {
      if let Ok(decl) = decl {
        properties.push(decl);
      }
    }

    Ok(FontPaletteValuesRule { name, properties, loc })
  }
}

impl<'i> Parse<'i> for BasePalette {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    if let Ok(i) = input.try_parse(CSSInteger::parse) {
      if i.is_negative() {
        return Err(input.new_custom_error(ParserError::InvalidValue));
      }
      return Ok(BasePalette::Integer(i as u16));
    }

    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    match_ignore_ascii_case! { &*ident,
      "light" => Ok(BasePalette::Light),
      "dark" => Ok(BasePalette::Dark),
      _ => Err(location.new_unexpected_token_error(Token::Ident(ident.clone())))
    }
  }
}

impl ToCss for BasePalette {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self {
      BasePalette::Light => dest.write_str("light"),
      BasePalette::Dark => dest.write_str("dark"),
      BasePalette::Integer(i) => (*i as CSSInteger).to_css(dest),
    }
  }
}

impl<'i> Parse<'i> for OverrideColors {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let index = CSSInteger::parse(input)?;
    if index.is_negative() {
      return Err(input.new_custom_error(ParserError::InvalidValue));
    }

    let color = CssColor::parse(input)?;
    if matches!(color, CssColor::CurrentColor) {
      return Err(input.new_custom_error(ParserError::InvalidValue));
    }

    Ok(OverrideColors {
      index: index as u16,
      color,
    })
  }
}

impl ToCss for OverrideColors {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    (self.index as CSSInteger).to_css(dest)?;
    dest.write_char(' ')?;
    self.color.to_css(dest)
  }
}

impl OverrideColors {
  fn get_fallback(&self, kind: ColorFallbackKind) -> OverrideColors {
    OverrideColors {
      index: self.index,
      color: self.color.get_fallback(kind),
    }
  }
}

impl<'i> FontPaletteValuesRule<'i> {
  pub(crate) fn minify(&mut self, context: &mut MinifyContext<'_, 'i>, _: bool) {
    let mut properties = Vec::with_capacity(self.properties.len());
    for property in &self.properties {
      match property {
        FontPaletteValuesProperty::OverrideColors(override_colors) => {
          // Generate color fallbacks.
          let mut fallbacks = ColorFallbackKind::empty();
          for o in override_colors {
            fallbacks |= o.color.get_necessary_fallbacks(context.targets.current);
          }

          if fallbacks.contains(ColorFallbackKind::RGB) {
            properties.push(FontPaletteValuesProperty::OverrideColors(
              override_colors.iter().map(|o| o.get_fallback(ColorFallbackKind::RGB)).collect(),
            ));
          }

          if fallbacks.contains(ColorFallbackKind::P3) {
            properties.push(FontPaletteValuesProperty::OverrideColors(
              override_colors.iter().map(|o| o.get_fallback(ColorFallbackKind::P3)).collect(),
            ));
          }

          let override_colors = if fallbacks.contains(ColorFallbackKind::LAB) {
            override_colors.iter().map(|o| o.get_fallback(ColorFallbackKind::P3)).collect()
          } else {
            override_colors.clone()
          };

          properties.push(FontPaletteValuesProperty::OverrideColors(override_colors));
        }
        _ => properties.push(property.clone()),
      }
    }

    self.properties = properties;
  }

  pub(crate) fn get_fallbacks<T>(&mut self, targets: Targets) -> Vec<CssRule<'i, T>> {
    // Get fallbacks for unparsed properties. These will generate @supports rules
    // containing duplicate @font-palette-values rules.
    let mut fallbacks = ColorFallbackKind::empty();
    for property in &self.properties {
      match property {
        FontPaletteValuesProperty::Custom(CustomProperty { value, .. }) => {
          fallbacks |= value.get_necessary_fallbacks(targets);
        }
        _ => {}
      }
    }

    let mut res = Vec::new();
    let lowest_fallback = fallbacks.lowest();
    fallbacks.remove(lowest_fallback);

    if fallbacks.contains(ColorFallbackKind::P3) {
      res.push(self.get_fallback(ColorFallbackKind::P3));
    }

    if fallbacks.contains(ColorFallbackKind::LAB)
      || (!lowest_fallback.is_empty() && lowest_fallback != ColorFallbackKind::LAB)
    {
      res.push(self.get_fallback(ColorFallbackKind::LAB));
    }

    if !lowest_fallback.is_empty() {
      for property in &mut self.properties {
        match property {
          FontPaletteValuesProperty::Custom(CustomProperty { value, .. }) => {
            *value = value.get_fallback(lowest_fallback);
          }
          _ => {}
        }
      }
    }

    res
  }

  fn get_fallback<T>(&self, kind: ColorFallbackKind) -> CssRule<'i, T> {
    let properties = self
      .properties
      .iter()
      .map(|property| match property {
        FontPaletteValuesProperty::Custom(custom) => FontPaletteValuesProperty::Custom(CustomProperty {
          name: custom.name.clone(),
          value: custom.value.get_fallback(kind),
        }),
        _ => property.clone(),
      })
      .collect();
    CssRule::Supports(SupportsRule {
      condition: kind.supports_condition(),
      rules: CssRuleList(vec![CssRule::FontPaletteValues(FontPaletteValuesRule {
        name: self.name.clone(),
        properties,
        loc: self.loc.clone(),
      })]),
      loc: self.loc.clone(),
    })
  }
}

impl<'i> ToCss for FontPaletteValuesRule<'i> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    #[cfg(feature = "sourcemap")]
    dest.add_mapping(self.loc);
    dest.write_str("@font-palette-values ")?;
    self.name.to_css(dest)?;
    dest.whitespace()?;
    dest.write_char('{')?;
    dest.indent();
    let len = self.properties.len();
    for (i, prop) in self.properties.iter().enumerate() {
      dest.newline()?;
      prop.to_css(dest)?;
      if i != len - 1 || !dest.minify {
        dest.write_char(';')?;
      }
    }
    dest.dedent();
    dest.newline()?;
    dest.write_char('}')
  }
}

impl<'i> ToCss for FontPaletteValuesProperty<'i> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    macro_rules! property {
      ($prop: literal, $value: expr) => {{
        dest.write_str($prop)?;
        dest.delim(':', false)?;
        $value.to_css(dest)
      }};
    }

    match self {
      FontPaletteValuesProperty::FontFamily(f) => property!("font-family", f),
      FontPaletteValuesProperty::BasePalette(b) => property!("base-palette", b),
      FontPaletteValuesProperty::OverrideColors(o) => property!("override-colors", o),
      FontPaletteValuesProperty::Custom(custom) => {
        dest.write_str(custom.name.as_ref())?;
        dest.delim(':', false)?;
        custom.value.to_css(dest, true)
      }
    }
  }
}
