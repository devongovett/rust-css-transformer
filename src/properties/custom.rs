//! CSS custom properties and unparsed token values.

use crate::error::{ParserError, PrinterError, PrinterErrorKind};
use crate::macros::enum_property;
use crate::prefixes::Feature;
use crate::printer::Printer;
use crate::properties::PropertyId;
use crate::rules::supports::SupportsCondition;
use crate::stylesheet::ParserOptions;
use crate::targets::{should_compile, Targets};
use crate::traits::{Parse, ParseWithOptions, ToCss};
use crate::values::angle::Angle;
use crate::values::color::{
  parse_hsl_hwb_components, parse_rgb_components, ColorFallbackKind, ComponentParser, CssColor, LightDarkColor,
  HSL, RGBA, SRGB,
};
use crate::values::ident::{CustomIdent, DashedIdent, DashedIdentReference, Ident};
use crate::values::length::{serialize_dimension, LengthValue};
use crate::values::number::CSSInteger;
use crate::values::percentage::Percentage;
use crate::values::resolution::Resolution;
use crate::values::string::CowArcStr;
use crate::values::time::Time;
use crate::values::url::Url;
#[cfg(feature = "visitor")]
use crate::visitor::Visit;
use cssparser::color::parse_hash_color;
use cssparser::*;

use super::AnimationName;
#[cfg(feature = "serde")]
use crate::serialization::ValueWrapper;

/// A CSS custom property, representing any unknown property.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct CustomProperty<'i> {
  /// The name of the property.
  #[cfg_attr(feature = "serde", serde(borrow))]
  pub name: CustomPropertyName<'i>,
  /// The property value, stored as a raw token list.
  pub value: TokenList<'i>,
}

impl<'i> CustomProperty<'i> {
  /// Parses a custom property with the given name.
  pub fn parse<'t>(
    name: CustomPropertyName<'i>,
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let value = input.parse_until_before(Delimiter::Bang | Delimiter::Semicolon, |input| {
      TokenList::parse(input, options, 0)
    })?;
    Ok(CustomProperty { name, value })
  }
}

/// A CSS custom property name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(feature = "serde", derive(serde::Serialize), serde(untagged))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum CustomPropertyName<'i> {
  /// An author-defined CSS custom property.
  #[cfg_attr(feature = "serde", serde(borrow))]
  Custom(DashedIdent<'i>),
  /// An unknown CSS property.
  Unknown(Ident<'i>),
}

impl<'i> From<CowArcStr<'i>> for CustomPropertyName<'i> {
  fn from(name: CowArcStr<'i>) -> Self {
    if name.starts_with("--") {
      CustomPropertyName::Custom(DashedIdent(name))
    } else {
      CustomPropertyName::Unknown(Ident(name))
    }
  }
}

impl<'i> From<CowRcStr<'i>> for CustomPropertyName<'i> {
  fn from(name: CowRcStr<'i>) -> Self {
    CustomPropertyName::from(CowArcStr::from(name))
  }
}

impl<'i> AsRef<str> for CustomPropertyName<'i> {
  #[inline]
  fn as_ref(&self) -> &str {
    match self {
      CustomPropertyName::Custom(c) => c.as_ref(),
      CustomPropertyName::Unknown(u) => u.as_ref(),
    }
  }
}

impl<'i> ToCss for CustomPropertyName<'i> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self {
      CustomPropertyName::Custom(c) => c.to_css(dest),
      CustomPropertyName::Unknown(u) => u.to_css(dest),
    }
  }
}

#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'i, 'de: 'i> serde::Deserialize<'de> for CustomPropertyName<'i> {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let name = CowArcStr::deserialize(deserializer)?;
    Ok(name.into())
  }
}

/// A known property with an unparsed value.
///
/// This type is used when the value of a known property could not
/// be parsed, e.g. in the case css `var()` references are encountered.
/// In this case, the raw tokens are stored instead.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(rename_all = "camelCase")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct UnparsedProperty<'i> {
  /// The id of the property.
  pub property_id: PropertyId<'i>,
  /// The property value, stored as a raw token list.
  #[cfg_attr(feature = "serde", serde(borrow))]
  pub value: TokenList<'i>,
}

impl<'i> UnparsedProperty<'i> {
  /// Parses a property with the given id as a token list.
  pub fn parse<'t>(
    property_id: PropertyId<'i>,
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let value = input.parse_until_before(Delimiter::Bang | Delimiter::Semicolon, |input| {
      TokenList::parse(input, options, 0)
    })?;
    Ok(UnparsedProperty { property_id, value })
  }

  pub(crate) fn get_prefixed(&self, targets: Targets, feature: Feature) -> UnparsedProperty<'i> {
    let mut clone = self.clone();
    let prefix = self.property_id.prefix();
    clone.property_id = clone.property_id.with_prefix(targets.prefixes(prefix.or_none(), feature));
    clone
  }

  /// Returns a new UnparsedProperty with the same value and the given property id.
  pub fn with_property_id(&self, property_id: PropertyId<'i>) -> UnparsedProperty<'i> {
    UnparsedProperty {
      property_id,
      value: self.value.clone(),
    }
  }

  /// Substitutes variables and re-parses the property.
  #[cfg(feature = "substitute_variables")]
  #[cfg_attr(docsrs, doc(cfg(feature = "substitute_variables")))]
  pub fn substitute_variables<'x>(
    mut self,
    vars: &std::collections::HashMap<&str, TokenList<'i>>,
  ) -> Result<super::Property<'x>, ()> {
    use super::Property;
    use crate::stylesheet::PrinterOptions;
    use static_self::IntoOwned;

    // Substitute variables in the token list.
    self.value.substitute_variables(vars);

    // Now stringify and re-parse the property to its fully parsed form.
    // Ideally we'd be able to reuse the tokens rather than printing, but cssparser doesn't provide a way to do that.
    let mut css = String::new();
    let mut dest = Printer::new(&mut css, PrinterOptions::default());
    self.value.to_css(&mut dest, false).unwrap();
    let property =
      Property::parse_string(self.property_id.clone(), &css, ParserOptions::default()).map_err(|_| ())?;
    Ok(property.into_owned())
  }
}

/// A raw list of CSS tokens, with embedded parsed values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "visitor", derive(Visit), visit(visit_token_list, TOKENS))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct TokenList<'i>(#[cfg_attr(feature = "serde", serde(borrow))] pub Vec<TokenOrValue<'i>>);

/// A raw CSS token, or a parsed value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit), visit(visit_token, TOKENS), visit_types(TOKENS | COLORS | URLS | VARIABLES | ENVIRONMENT_VARIABLES | FUNCTIONS | LENGTHS | ANGLES | TIMES | RESOLUTIONS | DASHED_IDENTS))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum TokenOrValue<'i> {
  /// A token.
  #[cfg_attr(feature = "serde", serde(borrow))]
  Token(Token<'i>),
  /// A parsed CSS color.
  Color(CssColor),
  /// A color with unresolved components.
  UnresolvedColor(UnresolvedColor<'i>),
  /// A parsed CSS url.
  Url(Url<'i>),
  /// A CSS variable reference.
  Var(Variable<'i>),
  /// A CSS environment variable reference.
  Env(EnvironmentVariable<'i>),
  /// A custom CSS function.
  Function(Function<'i>),
  /// A length.
  Length(LengthValue),
  /// An angle.
  Angle(Angle),
  /// A time.
  Time(Time),
  /// A resolution.
  Resolution(Resolution),
  /// A dashed ident.
  DashedIdent(DashedIdent<'i>),
  /// An animation name.
  AnimationName(AnimationName<'i>),
}

impl<'i> From<Token<'i>> for TokenOrValue<'i> {
  fn from(token: Token<'i>) -> TokenOrValue<'i> {
    TokenOrValue::Token(token)
  }
}

impl<'i> TokenOrValue<'i> {
  /// Returns whether the token is whitespace.
  pub fn is_whitespace(&self) -> bool {
    matches!(self, TokenOrValue::Token(Token::WhiteSpace(_)))
  }
}

impl<'a> Eq for TokenOrValue<'a> {}

impl<'a> std::hash::Hash for TokenOrValue<'a> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    let tag = std::mem::discriminant(self);
    tag.hash(state);
    match self {
      TokenOrValue::Token(t) => t.hash(state),
      _ => {
        // This function is primarily used to deduplicate selectors.
        // Values inside selectors should be exceedingly rare and implementing
        // Hash for them is somewhat complex due to floating point values.
        // For now, we just ignore them, which only means there are more
        // hash collisions. For such a rare case this is probably fine.
      }
    }
  }
}

impl<'i> ParseWithOptions<'i> for TokenList<'i> {
  fn parse_with_options<'t>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    TokenList::parse(input, options, 0)
  }
}

impl<'i> TokenList<'i> {
  pub(crate) fn parse<'t>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
    depth: usize,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let mut tokens = vec![];
    TokenList::parse_into(input, &mut tokens, options, depth)?;

    // Slice off leading and trailing whitespace if there are at least two tokens.
    // If there is only one token, we must preserve it. e.g. `--foo: ;` is valid.
    if tokens.len() >= 2 {
      let mut slice = &tokens[..];
      if matches!(tokens.first(), Some(token) if token.is_whitespace()) {
        slice = &slice[1..];
      }
      if matches!(tokens.last(), Some(token) if token.is_whitespace()) {
        slice = &slice[..slice.len() - 1];
      }
      return Ok(TokenList(slice.to_vec()));
    }

    return Ok(TokenList(tokens));
  }

  pub(crate) fn parse_raw<'t>(
    input: &mut Parser<'i, 't>,
    tokens: &mut Vec<TokenOrValue<'i>>,
    options: &ParserOptions<'_, 'i>,
    depth: usize,
  ) -> Result<(), ParseError<'i, ParserError<'i>>> {
    if depth > 500 {
      return Err(input.new_custom_error(ParserError::MaximumNestingDepth));
    }

    loop {
      let state = input.state();
      match input.next_including_whitespace_and_comments() {
        Ok(token @ &cssparser::Token::ParenthesisBlock)
        | Ok(token @ &cssparser::Token::SquareBracketBlock)
        | Ok(token @ &cssparser::Token::CurlyBracketBlock) => {
          tokens.push(Token::from(token).into());
          let closing_delimiter = match token {
            cssparser::Token::ParenthesisBlock => Token::CloseParenthesis,
            cssparser::Token::SquareBracketBlock => Token::CloseSquareBracket,
            cssparser::Token::CurlyBracketBlock => Token::CloseCurlyBracket,
            _ => unreachable!(),
          };

          input.parse_nested_block(|input| TokenList::parse_raw(input, tokens, options, depth + 1))?;
          tokens.push(closing_delimiter.into());
        }
        Ok(token @ &cssparser::Token::Function(_)) => {
          tokens.push(Token::from(token).into());
          input.parse_nested_block(|input| TokenList::parse_raw(input, tokens, options, depth + 1))?;
          tokens.push(Token::CloseParenthesis.into());
        }
        Ok(token) if token.is_parse_error() => {
          return Err(ParseError {
            kind: ParseErrorKind::Basic(BasicParseErrorKind::UnexpectedToken(token.clone())),
            location: state.source_location(),
          })
        }
        Ok(token) => {
          tokens.push(Token::from(token).into());
        }
        Err(_) => break,
      }
    }

    Ok(())
  }

  fn parse_into<'t>(
    input: &mut Parser<'i, 't>,
    tokens: &mut Vec<TokenOrValue<'i>>,
    options: &ParserOptions<'_, 'i>,
    depth: usize,
  ) -> Result<(), ParseError<'i, ParserError<'i>>> {
    if depth > 500 {
      return Err(input.new_custom_error(ParserError::MaximumNestingDepth));
    }

    let mut last_is_delim = false;
    let mut last_is_whitespace = false;
    loop {
      let state = input.state();
      match input.next_including_whitespace_and_comments() {
        Ok(&cssparser::Token::WhiteSpace(..)) | Ok(&cssparser::Token::Comment(..)) => {
          // Skip whitespace if the last token was a delimiter.
          // Otherwise, replace all whitespace and comments with a single space character.
          if !last_is_delim {
            tokens.push(Token::WhiteSpace(" ".into()).into());
            last_is_whitespace = true;
          }
        }
        Ok(&cssparser::Token::Function(ref f)) => {
          // Attempt to parse embedded color values into hex tokens.
          let f = f.into();
          if let Some(color) = try_parse_color_token(&f, &state, input) {
            tokens.push(TokenOrValue::Color(color));
            last_is_delim = false;
            last_is_whitespace = false;
          } else if let Ok(color) = input.try_parse(|input| UnresolvedColor::parse(&f, input, options)) {
            tokens.push(TokenOrValue::UnresolvedColor(color));
            last_is_delim = true;
            last_is_whitespace = false;
          } else if f == "url" {
            input.reset(&state);
            tokens.push(TokenOrValue::Url(Url::parse(input)?));
            last_is_delim = false;
            last_is_whitespace = false;
          } else if f == "var" {
            let var = input.parse_nested_block(|input| {
              let var = Variable::parse(input, options, depth + 1)?;
              Ok(TokenOrValue::Var(var))
            })?;
            tokens.push(var);
            last_is_delim = true;
            last_is_whitespace = false;
          } else if f == "env" {
            let env = input.parse_nested_block(|input| {
              let env = EnvironmentVariable::parse_nested(input, options, depth + 1)?;
              Ok(TokenOrValue::Env(env))
            })?;
            tokens.push(env);
            last_is_delim = true;
            last_is_whitespace = false;
          } else {
            let arguments = input.parse_nested_block(|input| TokenList::parse(input, options, depth + 1))?;
            tokens.push(TokenOrValue::Function(Function {
              name: Ident(f),
              arguments,
            }));
            last_is_delim = true; // Whitespace is not required after any of these chars.
            last_is_whitespace = false;
          }
        }
        Ok(&cssparser::Token::Hash(ref h)) | Ok(&cssparser::Token::IDHash(ref h)) => {
          if let Ok((r, g, b, a)) = parse_hash_color(h.as_bytes()) {
            tokens.push(TokenOrValue::Color(CssColor::RGBA(RGBA::new(r, g, b, a))));
          } else {
            tokens.push(Token::Hash(h.into()).into());
          }
          last_is_delim = false;
          last_is_whitespace = false;
        }
        Ok(&cssparser::Token::UnquotedUrl(_)) => {
          input.reset(&state);
          tokens.push(TokenOrValue::Url(Url::parse(input)?));
          last_is_delim = false;
          last_is_whitespace = false;
        }
        Ok(&cssparser::Token::Ident(ref name)) if name.starts_with("--") => {
          tokens.push(TokenOrValue::DashedIdent(name.into()));
          last_is_delim = false;
          last_is_whitespace = false;
        }
        Ok(token @ &cssparser::Token::ParenthesisBlock)
        | Ok(token @ &cssparser::Token::SquareBracketBlock)
        | Ok(token @ &cssparser::Token::CurlyBracketBlock) => {
          tokens.push(Token::from(token).into());
          let closing_delimiter = match token {
            cssparser::Token::ParenthesisBlock => Token::CloseParenthesis,
            cssparser::Token::SquareBracketBlock => Token::CloseSquareBracket,
            cssparser::Token::CurlyBracketBlock => Token::CloseCurlyBracket,
            _ => unreachable!(),
          };

          input.parse_nested_block(|input| TokenList::parse_into(input, tokens, options, depth + 1))?;

          tokens.push(closing_delimiter.into());
          last_is_delim = true; // Whitespace is not required after any of these chars.
          last_is_whitespace = false;
        }
        Ok(token @ cssparser::Token::Dimension { .. }) => {
          let value = if let Ok(length) = LengthValue::try_from(token) {
            TokenOrValue::Length(length)
          } else if let Ok(angle) = Angle::try_from(token) {
            TokenOrValue::Angle(angle)
          } else if let Ok(time) = Time::try_from(token) {
            TokenOrValue::Time(time)
          } else if let Ok(resolution) = Resolution::try_from(token) {
            TokenOrValue::Resolution(resolution)
          } else {
            TokenOrValue::Token(token.into())
          };
          tokens.push(value);
          last_is_delim = false;
          last_is_whitespace = false;
        }
        Ok(token) if token.is_parse_error() => {
          return Err(ParseError {
            kind: ParseErrorKind::Basic(BasicParseErrorKind::UnexpectedToken(token.clone())),
            location: state.source_location(),
          })
        }
        Ok(token) => {
          last_is_delim = matches!(token, cssparser::Token::Delim(_) | cssparser::Token::Comma);

          // If this is a delimiter, and the last token was whitespace,
          // replace the whitespace with the delimiter since both are not required.
          if last_is_delim && last_is_whitespace {
            let last = tokens.last_mut().unwrap();
            *last = Token::from(token).into();
          } else {
            tokens.push(Token::from(token).into());
          }

          last_is_whitespace = false;
        }
        Err(_) => break,
      }
    }

    Ok(())
  }
}

#[inline]
fn try_parse_color_token<'i, 't>(
  f: &CowArcStr<'i>,
  state: &ParserState,
  input: &mut Parser<'i, 't>,
) -> Option<CssColor> {
  match_ignore_ascii_case! { &*f,
    "rgb" | "rgba" | "hsl" | "hsla" | "hwb" | "lab" | "lch" | "oklab" | "oklch" | "color" | "color-mix" | "light-dark" => {
      let s = input.state();
      input.reset(&state);
      if let Ok(color) = CssColor::parse(input) {
        return Some(color)
      }
      input.reset(&s);
    },
    _ => {}
  }

  None
}

impl<'i> TokenList<'i> {
  pub(crate) fn to_css<W>(&self, dest: &mut Printer<W>, is_custom_property: bool) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    if !dest.minify && self.0.len() == 1 && matches!(self.0.first(), Some(token) if token.is_whitespace()) {
      return Ok(());
    }

    let mut has_whitespace = false;
    for (i, token_or_value) in self.0.iter().enumerate() {
      has_whitespace = match token_or_value {
        TokenOrValue::Color(color) => {
          color.to_css(dest)?;
          false
        }
        TokenOrValue::UnresolvedColor(color) => {
          color.to_css(dest, is_custom_property)?;
          false
        }
        TokenOrValue::Url(url) => {
          if dest.dependencies.is_some() && is_custom_property && !url.is_absolute() {
            return Err(dest.error(
              PrinterErrorKind::AmbiguousUrlInCustomProperty {
                url: url.url.as_ref().to_owned(),
              },
              url.loc,
            ));
          }
          url.to_css(dest)?;
          false
        }
        TokenOrValue::Var(var) => {
          var.to_css(dest, is_custom_property)?;
          self.write_whitespace_if_needed(i, dest)?
        }
        TokenOrValue::Env(env) => {
          env.to_css(dest, is_custom_property)?;
          self.write_whitespace_if_needed(i, dest)?
        }
        TokenOrValue::Function(f) => {
          f.to_css(dest, is_custom_property)?;
          self.write_whitespace_if_needed(i, dest)?
        }
        TokenOrValue::Length(v) => {
          // Do not serialize unitless zero lengths in custom properties as it may break calc().
          let (value, unit) = v.to_unit_value();
          serialize_dimension(value, unit, dest)?;
          false
        }
        TokenOrValue::Angle(v) => {
          v.to_css(dest)?;
          false
        }
        TokenOrValue::Time(v) => {
          v.to_css(dest)?;
          false
        }
        TokenOrValue::Resolution(v) => {
          v.to_css(dest)?;
          false
        }
        TokenOrValue::DashedIdent(v) => {
          v.to_css(dest)?;
          false
        }
        TokenOrValue::AnimationName(v) => {
          v.to_css(dest)?;
          false
        }
        TokenOrValue::Token(token) => match token {
          Token::Delim(d) => {
            if *d == '+' || *d == '-' {
              dest.write_char(' ')?;
              dest.write_char(*d)?;
              dest.write_char(' ')?;
            } else {
              let ws_before = !has_whitespace && (*d == '/' || *d == '*');
              dest.delim(*d, ws_before)?;
            }
            true
          }
          Token::Comma => {
            dest.delim(',', false)?;
            true
          }
          Token::CloseParenthesis | Token::CloseSquareBracket | Token::CloseCurlyBracket => {
            token.to_css(dest)?;
            self.write_whitespace_if_needed(i, dest)?
          }
          Token::Dimension { value, unit, .. } => {
            serialize_dimension(*value, unit, dest)?;
            false
          }
          Token::Number { value, .. } => {
            value.to_css(dest)?;
            false
          }
          _ => {
            token.to_css(dest)?;
            matches!(token, Token::WhiteSpace(..))
          }
        },
      };
    }

    Ok(())
  }

  pub(crate) fn to_css_raw<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    for token_or_value in &self.0 {
      match token_or_value {
        TokenOrValue::Token(token) => {
          token.to_css(dest)?;
        }
        _ => {
          return Err(PrinterError {
            kind: PrinterErrorKind::FmtError,
            loc: None,
          })
        }
      }
    }

    Ok(())
  }

  #[inline]
  fn write_whitespace_if_needed<W>(&self, i: usize, dest: &mut Printer<W>) -> Result<bool, PrinterError>
  where
    W: std::fmt::Write,
  {
    if !dest.minify
      && i != self.0.len() - 1
      && !matches!(
        self.0[i + 1],
        TokenOrValue::Token(Token::Comma) | TokenOrValue::Token(Token::CloseParenthesis)
      )
    {
      // Whitespace is removed during parsing, so add it back if we aren't minifying.
      dest.write_char(' ')?;
      Ok(true)
    } else {
      Ok(false)
    }
  }
}

/// A raw CSS token.
// Copied from cssparser to change CowRcStr to CowArcStr
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum Token<'a> {
  /// A [`<ident-token>`](https://drafts.csswg.org/css-syntax/#ident-token-diagram)
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  Ident(#[cfg_attr(feature = "serde", serde(borrow))] CowArcStr<'a>),

  /// A [`<at-keyword-token>`](https://drafts.csswg.org/css-syntax/#at-keyword-token-diagram)
  ///
  /// The value does not include the `@` marker.
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  AtKeyword(CowArcStr<'a>),

  /// A [`<hash-token>`](https://drafts.csswg.org/css-syntax/#hash-token-diagram) with the type flag set to "unrestricted"
  ///
  /// The value does not include the `#` marker.
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  Hash(CowArcStr<'a>),

  /// A [`<hash-token>`](https://drafts.csswg.org/css-syntax/#hash-token-diagram) with the type flag set to "id"
  ///
  /// The value does not include the `#` marker.
  #[cfg_attr(feature = "serde", serde(rename = "id-hash", with = "ValueWrapper::<CowArcStr>"))]
  IDHash(CowArcStr<'a>), // Hash that is a valid ID selector.

  /// A [`<string-token>`](https://drafts.csswg.org/css-syntax/#string-token-diagram)
  ///
  /// The value does not include the quotes.
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  String(CowArcStr<'a>),

  /// A [`<url-token>`](https://drafts.csswg.org/css-syntax/#url-token-diagram)
  ///
  /// The value does not include the `url(` `)` markers.  Note that `url( <string-token> )` is represented by a
  /// `Function` token.
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  UnquotedUrl(CowArcStr<'a>),

  /// A `<delim-token>`
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<char>"))]
  Delim(char),

  /// A [`<number-token>`](https://drafts.csswg.org/css-syntax/#number-token-diagram)
  Number {
    /// Whether the number had a `+` or `-` sign.
    ///
    /// This is used is some cases like the <An+B> micro syntax. (See the `parse_nth` function.)
    #[cfg_attr(feature = "serde", serde(skip))]
    has_sign: bool,

    /// The value as a float
    value: f32,

    /// If the origin source did not include a fractional part, the value as an integer.
    #[cfg_attr(feature = "serde", serde(skip))]
    int_value: Option<i32>,
  },

  /// A [`<percentage-token>`](https://drafts.csswg.org/css-syntax/#percentage-token-diagram)
  Percentage {
    /// Whether the number had a `+` or `-` sign.
    #[cfg_attr(feature = "serde", serde(skip))]
    has_sign: bool,

    /// The value as a float, divided by 100 so that the nominal range is 0.0 to 1.0.
    #[cfg_attr(feature = "serde", serde(rename = "value"))]
    unit_value: f32,

    /// If the origin source did not include a fractional part, the value as an integer.
    /// It is **not** divided by 100.
    #[cfg_attr(feature = "serde", serde(skip))]
    int_value: Option<i32>,
  },

  /// A [`<dimension-token>`](https://drafts.csswg.org/css-syntax/#dimension-token-diagram)
  Dimension {
    /// Whether the number had a `+` or `-` sign.
    ///
    /// This is used is some cases like the <An+B> micro syntax. (See the `parse_nth` function.)
    #[cfg_attr(feature = "serde", serde(skip))]
    has_sign: bool,

    /// The value as a float
    value: f32,

    /// If the origin source did not include a fractional part, the value as an integer.
    #[cfg_attr(feature = "serde", serde(skip))]
    int_value: Option<i32>,

    /// The unit, e.g. "px" in `12px`
    unit: CowArcStr<'a>,
  },

  /// A [`<whitespace-token>`](https://drafts.csswg.org/css-syntax/#whitespace-token-diagram)
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  WhiteSpace(CowArcStr<'a>),

  /// A comment.
  ///
  /// The CSS Syntax spec does not generate tokens for comments,
  /// But we do, because we can (borrowed &str makes it cheap).
  ///
  /// The value does not include the `/*` `*/` markers.
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  Comment(CowArcStr<'a>),

  /// A `:` `<colon-token>`
  Colon, // :

  /// A `;` `<semicolon-token>`
  Semicolon, // ;

  /// A `,` `<comma-token>`
  Comma, // ,

  /// A `~=` [`<include-match-token>`](https://drafts.csswg.org/css-syntax/#include-match-token-diagram)
  IncludeMatch,

  /// A `|=` [`<dash-match-token>`](https://drafts.csswg.org/css-syntax/#dash-match-token-diagram)
  DashMatch,

  /// A `^=` [`<prefix-match-token>`](https://drafts.csswg.org/css-syntax/#prefix-match-token-diagram)
  PrefixMatch,

  /// A `$=` [`<suffix-match-token>`](https://drafts.csswg.org/css-syntax/#suffix-match-token-diagram)
  SuffixMatch,

  /// A `*=` [`<substring-match-token>`](https://drafts.csswg.org/css-syntax/#substring-match-token-diagram)
  SubstringMatch,

  /// A `<!--` [`<CDO-token>`](https://drafts.csswg.org/css-syntax/#CDO-token-diagram)
  #[cfg_attr(feature = "serde", serde(rename = "cdo"))]
  CDO,

  /// A `-->` [`<CDC-token>`](https://drafts.csswg.org/css-syntax/#CDC-token-diagram)
  #[cfg_attr(feature = "serde", serde(rename = "cdc"))]
  CDC,

  /// A [`<function-token>`](https://drafts.csswg.org/css-syntax/#function-token-diagram)
  ///
  /// The value (name) does not include the `(` marker.
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  Function(CowArcStr<'a>),

  /// A `<(-token>`
  ParenthesisBlock,

  /// A `<[-token>`
  SquareBracketBlock,

  /// A `<{-token>`
  CurlyBracketBlock,

  /// A `<bad-url-token>`
  ///
  /// This token always indicates a parse error.
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  BadUrl(CowArcStr<'a>),

  /// A `<bad-string-token>`
  ///
  /// This token always indicates a parse error.
  #[cfg_attr(feature = "serde", serde(with = "ValueWrapper::<CowArcStr>"))]
  BadString(CowArcStr<'a>),

  /// A `<)-token>`
  ///
  /// When obtained from one of the `Parser::next*` methods,
  /// this token is always unmatched and indicates a parse error.
  CloseParenthesis,

  /// A `<]-token>`
  ///
  /// When obtained from one of the `Parser::next*` methods,
  /// this token is always unmatched and indicates a parse error.
  CloseSquareBracket,

  /// A `<}-token>`
  ///
  /// When obtained from one of the `Parser::next*` methods,
  /// this token is always unmatched and indicates a parse error.
  CloseCurlyBracket,
}

impl<'a> From<&cssparser::Token<'a>> for Token<'a> {
  #[inline]
  fn from(t: &cssparser::Token<'a>) -> Token<'a> {
    match t {
      cssparser::Token::Ident(x) => Token::Ident(x.into()),
      cssparser::Token::AtKeyword(x) => Token::AtKeyword(x.into()),
      cssparser::Token::Hash(x) => Token::Hash(x.into()),
      cssparser::Token::IDHash(x) => Token::IDHash(x.into()),
      cssparser::Token::QuotedString(x) => Token::String(x.into()),
      cssparser::Token::UnquotedUrl(x) => Token::UnquotedUrl(x.into()),
      cssparser::Token::Function(x) => Token::Function(x.into()),
      cssparser::Token::BadUrl(x) => Token::BadUrl(x.into()),
      cssparser::Token::BadString(x) => Token::BadString(x.into()),
      cssparser::Token::Delim(c) => Token::Delim(*c),
      cssparser::Token::Number {
        has_sign,
        value,
        int_value,
      } => Token::Number {
        has_sign: *has_sign,
        value: *value,
        int_value: *int_value,
      },
      cssparser::Token::Dimension {
        has_sign,
        value,
        int_value,
        unit,
      } => Token::Dimension {
        has_sign: *has_sign,
        value: *value,
        int_value: *int_value,
        unit: unit.into(),
      },
      cssparser::Token::Percentage {
        has_sign,
        unit_value,
        int_value,
      } => Token::Percentage {
        has_sign: *has_sign,
        unit_value: *unit_value,
        int_value: *int_value,
      },
      cssparser::Token::WhiteSpace(w) => Token::WhiteSpace((*w).into()),
      cssparser::Token::Comment(c) => Token::Comment((*c).into()),
      cssparser::Token::Colon => Token::Colon,
      cssparser::Token::Semicolon => Token::Semicolon,
      cssparser::Token::Comma => Token::Comma,
      cssparser::Token::IncludeMatch => Token::IncludeMatch,
      cssparser::Token::DashMatch => Token::DashMatch,
      cssparser::Token::PrefixMatch => Token::PrefixMatch,
      cssparser::Token::SuffixMatch => Token::SuffixMatch,
      cssparser::Token::SubstringMatch => Token::SubstringMatch,
      cssparser::Token::CDO => Token::CDO,
      cssparser::Token::CDC => Token::CDC,
      cssparser::Token::ParenthesisBlock => Token::ParenthesisBlock,
      cssparser::Token::SquareBracketBlock => Token::SquareBracketBlock,
      cssparser::Token::CurlyBracketBlock => Token::CurlyBracketBlock,
      cssparser::Token::CloseParenthesis => Token::CloseParenthesis,
      cssparser::Token::CloseSquareBracket => Token::CloseSquareBracket,
      cssparser::Token::CloseCurlyBracket => Token::CloseCurlyBracket,
    }
  }
}

impl<'a> ToCss for Token<'a> {
  #[inline]
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    use cssparser::ToCss;
    match self {
      Token::Ident(x) => cssparser::Token::Ident(x.as_ref().into()).to_css(dest)?,
      Token::AtKeyword(x) => cssparser::Token::AtKeyword(x.as_ref().into()).to_css(dest)?,
      Token::Hash(x) => cssparser::Token::Hash(x.as_ref().into()).to_css(dest)?,
      Token::IDHash(x) => cssparser::Token::IDHash(x.as_ref().into()).to_css(dest)?,
      Token::String(x) => cssparser::Token::QuotedString(x.as_ref().into()).to_css(dest)?,
      Token::UnquotedUrl(x) => cssparser::Token::UnquotedUrl(x.as_ref().into()).to_css(dest)?,
      Token::Function(x) => cssparser::Token::Function(x.as_ref().into()).to_css(dest)?,
      Token::BadUrl(x) => cssparser::Token::BadUrl(x.as_ref().into()).to_css(dest)?,
      Token::BadString(x) => cssparser::Token::BadString(x.as_ref().into()).to_css(dest)?,
      Token::Delim(c) => cssparser::Token::Delim(*c).to_css(dest)?,
      Token::Number {
        has_sign,
        value,
        int_value,
      } => cssparser::Token::Number {
        has_sign: *has_sign,
        value: *value,
        int_value: *int_value,
      }
      .to_css(dest)?,
      Token::Dimension {
        has_sign,
        value,
        int_value,
        unit,
      } => cssparser::Token::Dimension {
        has_sign: *has_sign,
        value: *value,
        int_value: *int_value,
        unit: unit.as_ref().into(),
      }
      .to_css(dest)?,
      Token::Percentage {
        has_sign,
        unit_value,
        int_value,
      } => cssparser::Token::Percentage {
        has_sign: *has_sign,
        unit_value: *unit_value,
        int_value: *int_value,
      }
      .to_css(dest)?,
      Token::WhiteSpace(w) => cssparser::Token::WhiteSpace(w).to_css(dest)?,
      Token::Comment(c) => cssparser::Token::Comment(c).to_css(dest)?,
      Token::Colon => cssparser::Token::Colon.to_css(dest)?,
      Token::Semicolon => cssparser::Token::Semicolon.to_css(dest)?,
      Token::Comma => cssparser::Token::Comma.to_css(dest)?,
      Token::IncludeMatch => cssparser::Token::IncludeMatch.to_css(dest)?,
      Token::DashMatch => cssparser::Token::DashMatch.to_css(dest)?,
      Token::PrefixMatch => cssparser::Token::PrefixMatch.to_css(dest)?,
      Token::SuffixMatch => cssparser::Token::SuffixMatch.to_css(dest)?,
      Token::SubstringMatch => cssparser::Token::SubstringMatch.to_css(dest)?,
      Token::CDO => cssparser::Token::CDO.to_css(dest)?,
      Token::CDC => cssparser::Token::CDC.to_css(dest)?,
      Token::ParenthesisBlock => cssparser::Token::ParenthesisBlock.to_css(dest)?,
      Token::SquareBracketBlock => cssparser::Token::SquareBracketBlock.to_css(dest)?,
      Token::CurlyBracketBlock => cssparser::Token::CurlyBracketBlock.to_css(dest)?,
      Token::CloseParenthesis => cssparser::Token::CloseParenthesis.to_css(dest)?,
      Token::CloseSquareBracket => cssparser::Token::CloseSquareBracket.to_css(dest)?,
      Token::CloseCurlyBracket => cssparser::Token::CloseCurlyBracket.to_css(dest)?,
    }

    Ok(())
  }
}

impl<'a> Eq for Token<'a> {}

impl<'a> std::hash::Hash for Token<'a> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    let tag = std::mem::discriminant(self);
    tag.hash(state);
    match self {
      Token::Ident(x) => x.hash(state),
      Token::AtKeyword(x) => x.hash(state),
      Token::Hash(x) => x.hash(state),
      Token::IDHash(x) => x.hash(state),
      Token::String(x) => x.hash(state),
      Token::UnquotedUrl(x) => x.hash(state),
      Token::Function(x) => x.hash(state),
      Token::BadUrl(x) => x.hash(state),
      Token::BadString(x) => x.hash(state),
      Token::Delim(x) => x.hash(state),
      Token::Number {
        has_sign,
        value,
        int_value,
      } => {
        has_sign.hash(state);
        integer_decode(*value).hash(state);
        int_value.hash(state);
      }
      Token::Dimension {
        has_sign,
        value,
        int_value,
        unit,
      } => {
        has_sign.hash(state);
        integer_decode(*value).hash(state);
        int_value.hash(state);
        unit.hash(state);
      }
      Token::Percentage {
        has_sign,
        unit_value,
        int_value,
      } => {
        has_sign.hash(state);
        integer_decode(*unit_value).hash(state);
        int_value.hash(state);
      }
      Token::WhiteSpace(w) => w.hash(state),
      Token::Comment(c) => c.hash(state),
      Token::Colon
      | Token::Semicolon
      | Token::Comma
      | Token::IncludeMatch
      | Token::DashMatch
      | Token::PrefixMatch
      | Token::SuffixMatch
      | Token::SubstringMatch
      | Token::CDO
      | Token::CDC
      | Token::ParenthesisBlock
      | Token::SquareBracketBlock
      | Token::CurlyBracketBlock
      | Token::CloseParenthesis
      | Token::CloseSquareBracket
      | Token::CloseCurlyBracket => {}
    }
  }
}

/// Converts a floating point value into its mantissa, exponent,
/// and sign components so that it can be hashed.
fn integer_decode(v: f32) -> (u32, i16, i8) {
  let bits: u32 = unsafe { std::mem::transmute(v) };
  let sign: i8 = if bits >> 31 == 0 { 1 } else { -1 };
  let mut exponent: i16 = ((bits >> 23) & 0xff) as i16;
  let mantissa = if exponent == 0 {
    (bits & 0x7fffff) << 1
  } else {
    (bits & 0x7fffff) | 0x800000
  };
  // Exponent bias + mantissa shift
  exponent -= 127 + 23;
  (mantissa, exponent, sign)
}

impl<'i> TokenList<'i> {
  pub(crate) fn get_necessary_fallbacks(&self, targets: Targets) -> ColorFallbackKind {
    let mut fallbacks = ColorFallbackKind::empty();
    for token in &self.0 {
      match token {
        TokenOrValue::Color(color) => {
          fallbacks |= color.get_possible_fallbacks(targets);
        }
        TokenOrValue::Function(f) => {
          fallbacks |= f.arguments.get_necessary_fallbacks(targets);
        }
        TokenOrValue::Var(v) => {
          if let Some(fallback) = &v.fallback {
            fallbacks |= fallback.get_necessary_fallbacks(targets);
          }
        }
        TokenOrValue::Env(v) => {
          if let Some(fallback) = &v.fallback {
            fallbacks |= fallback.get_necessary_fallbacks(targets);
          }
        }
        _ => {}
      }
    }

    fallbacks
  }

  pub(crate) fn get_fallback(&self, kind: ColorFallbackKind) -> Self {
    let tokens = self
      .0
      .iter()
      .map(|token| match token {
        TokenOrValue::Color(color) => TokenOrValue::Color(color.get_fallback(kind)),
        TokenOrValue::Function(f) => TokenOrValue::Function(f.get_fallback(kind)),
        TokenOrValue::Var(v) => TokenOrValue::Var(v.get_fallback(kind)),
        TokenOrValue::Env(e) => TokenOrValue::Env(e.get_fallback(kind)),
        _ => token.clone(),
      })
      .collect();
    TokenList(tokens)
  }

  pub(crate) fn get_fallbacks(&mut self, targets: Targets) -> Vec<(SupportsCondition<'i>, Self)> {
    // Get the full list of possible fallbacks, and remove the lowest one, which will replace
    // the original declaration. The remaining fallbacks need to be added as @supports rules.
    let mut fallbacks = self.get_necessary_fallbacks(targets);
    let lowest_fallback = fallbacks.lowest();
    fallbacks.remove(lowest_fallback);

    let mut res = Vec::new();
    if fallbacks.contains(ColorFallbackKind::P3) {
      res.push((
        ColorFallbackKind::P3.supports_condition(),
        self.get_fallback(ColorFallbackKind::P3),
      ));
    }

    if fallbacks.contains(ColorFallbackKind::LAB) {
      res.push((
        ColorFallbackKind::LAB.supports_condition(),
        self.get_fallback(ColorFallbackKind::LAB),
      ));
    }

    if !lowest_fallback.is_empty() {
      for token in self.0.iter_mut() {
        match token {
          TokenOrValue::Color(color) => {
            *color = color.get_fallback(lowest_fallback);
          }
          TokenOrValue::Function(f) => *f = f.get_fallback(lowest_fallback),
          TokenOrValue::Var(v) if v.fallback.is_some() => *v = v.get_fallback(lowest_fallback),
          TokenOrValue::Env(v) if v.fallback.is_some() => *v = v.get_fallback(lowest_fallback),
          _ => {}
        }
      }
    }

    res
  }

  /// Substitutes variables with the provided values.
  #[cfg(feature = "substitute_variables")]
  #[cfg_attr(docsrs, doc(cfg(feature = "substitute_variables")))]
  pub fn substitute_variables(&mut self, vars: &std::collections::HashMap<&str, TokenList<'i>>) {
    self.visit(&mut VarInliner { vars }).unwrap()
  }
}

#[cfg(feature = "substitute_variables")]
struct VarInliner<'a, 'i> {
  vars: &'a std::collections::HashMap<&'a str, TokenList<'i>>,
}

#[cfg(feature = "substitute_variables")]
impl<'a, 'i> crate::visitor::Visitor<'i> for VarInliner<'a, 'i> {
  type Error = std::convert::Infallible;

  fn visit_types(&self) -> crate::visitor::VisitTypes {
    crate::visit_types!(TOKENS | VARIABLES)
  }

  fn visit_token_list(&mut self, tokens: &mut TokenList<'i>) -> Result<(), Self::Error> {
    let mut i = 0;
    let mut seen = std::collections::HashSet::new();
    while i < tokens.0.len() {
      let token = &mut tokens.0[i];
      token.visit(self).unwrap();
      if let TokenOrValue::Var(var) = token {
        if let Some(value) = self.vars.get(var.name.ident.0.as_ref()) {
          // Ignore circular references.
          if seen.insert(var.name.ident.0.clone()) {
            tokens.0.splice(i..i + 1, value.0.iter().cloned());
            // Don't advance. We need to replace any variables in the value.
            continue;
          }
        } else if let Some(fallback) = &var.fallback {
          let fallback = fallback.0.clone();
          if seen.insert(var.name.ident.0.clone()) {
            tokens.0.splice(i..i + 1, fallback.into_iter());
            continue;
          }
        }
      }
      seen.clear();
      i += 1;
    }
    Ok(())
  }
}

/// A CSS variable reference.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(feature = "visitor", visit(visit_variable, VARIABLES))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct Variable<'i> {
  /// The variable name.
  #[cfg_attr(feature = "serde", serde(borrow))]
  pub name: DashedIdentReference<'i>,
  /// A fallback value in case the variable is not defined.
  pub fallback: Option<TokenList<'i>>,
}

impl<'i> Variable<'i> {
  fn parse<'t>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
    depth: usize,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let name = DashedIdentReference::parse_with_options(input, options)?;

    let fallback = if input.try_parse(|input| input.expect_comma()).is_ok() {
      Some(TokenList::parse(input, options, depth)?)
    } else {
      None
    };

    Ok(Variable { name, fallback })
  }

  fn to_css<W>(&self, dest: &mut Printer<W>, is_custom_property: bool) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    dest.write_str("var(")?;
    self.name.to_css(dest)?;
    if let Some(fallback) = &self.fallback {
      dest.delim(',', false)?;
      fallback.to_css(dest, is_custom_property)?;
    }
    dest.write_char(')')
  }

  fn get_fallback(&self, kind: ColorFallbackKind) -> Self {
    Variable {
      name: self.name.clone(),
      fallback: self.fallback.as_ref().map(|fallback| fallback.get_fallback(kind)),
    }
  }
}

/// A CSS environment variable reference.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
  feature = "visitor",
  derive(Visit),
  visit(visit_environment_variable, ENVIRONMENT_VARIABLES)
)]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct EnvironmentVariable<'i> {
  /// The environment variable name.
  #[cfg_attr(feature = "serde", serde(borrow))]
  pub name: EnvironmentVariableName<'i>,
  /// Optional indices into the dimensions of the environment variable.
  #[cfg_attr(feature = "serde", serde(default))]
  pub indices: Vec<CSSInteger>,
  /// A fallback value in case the variable is not defined.
  pub fallback: Option<TokenList<'i>>,
}

/// A CSS environment variable name.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", rename_all = "lowercase")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum EnvironmentVariableName<'i> {
  /// A UA-defined environment variable.
  #[cfg_attr(
    feature = "serde",
    serde(with = "crate::serialization::ValueWrapper::<UAEnvironmentVariable>")
  )]
  UA(UAEnvironmentVariable),
  /// A custom author-defined environment variable.
  #[cfg_attr(feature = "serde", serde(borrow))]
  Custom(DashedIdentReference<'i>),
  /// An unknown environment variable.
  #[cfg_attr(feature = "serde", serde(with = "crate::serialization::ValueWrapper::<CustomIdent>"))]
  Unknown(CustomIdent<'i>),
}

enum_property! {
  /// A UA-defined environment variable name.
  pub enum UAEnvironmentVariable {
    /// The safe area inset from the top of the viewport.
    SafeAreaInsetTop,
    /// The safe area inset from the right of the viewport.
    SafeAreaInsetRight,
    /// The safe area inset from the bottom of the viewport.
    SafeAreaInsetBottom,
    /// The safe area inset from the left of the viewport.
    SafeAreaInsetLeft,
    /// The viewport segment width.
    ViewportSegmentWidth,
    /// The viewport segment height.
    ViewportSegmentHeight,
    /// The viewport segment top position.
    ViewportSegmentTop,
    /// The viewport segment left position.
    ViewportSegmentLeft,
    /// The viewport segment bottom position.
    ViewportSegmentBottom,
    /// The viewport segment right position.
    ViewportSegmentRight,
  }
}

impl<'i> EnvironmentVariableName<'i> {
  /// Returns the name of the environment variable as a string.
  pub fn name(&self) -> &str {
    match self {
      EnvironmentVariableName::UA(ua) => ua.as_str(),
      EnvironmentVariableName::Custom(c) => c.ident.as_ref(),
      EnvironmentVariableName::Unknown(u) => u.0.as_ref(),
    }
  }
}

impl<'i> Parse<'i> for EnvironmentVariableName<'i> {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    if let Ok(ua) = input.try_parse(UAEnvironmentVariable::parse) {
      return Ok(EnvironmentVariableName::UA(ua));
    }

    if let Ok(dashed) =
      input.try_parse(|input| DashedIdentReference::parse_with_options(input, &ParserOptions::default()))
    {
      return Ok(EnvironmentVariableName::Custom(dashed));
    }

    let ident = CustomIdent::parse(input)?;
    return Ok(EnvironmentVariableName::Unknown(ident));
  }
}

impl<'i> ToCss for EnvironmentVariableName<'i> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self {
      EnvironmentVariableName::UA(ua) => ua.to_css(dest),
      EnvironmentVariableName::Custom(custom) => custom.to_css(dest),
      EnvironmentVariableName::Unknown(unknown) => unknown.to_css(dest),
    }
  }
}

impl<'i> EnvironmentVariable<'i> {
  pub(crate) fn parse<'t>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
    depth: usize,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    input.expect_function_matching("env")?;
    input.parse_nested_block(|input| Self::parse_nested(input, options, depth))
  }

  pub(crate) fn parse_nested<'t>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
    depth: usize,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let name = EnvironmentVariableName::parse(input)?;
    let mut indices = Vec::new();
    while let Ok(index) = input.try_parse(CSSInteger::parse) {
      indices.push(index);
    }

    let fallback = if input.try_parse(|input| input.expect_comma()).is_ok() {
      Some(TokenList::parse(input, options, depth + 1)?)
    } else {
      None
    };

    Ok(EnvironmentVariable {
      name,
      indices,
      fallback,
    })
  }

  pub(crate) fn to_css<W>(&self, dest: &mut Printer<W>, is_custom_property: bool) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    dest.write_str("env(")?;
    self.name.to_css(dest)?;

    for item in &self.indices {
      dest.write_char(' ')?;
      item.to_css(dest)?;
    }

    if let Some(fallback) = &self.fallback {
      dest.delim(',', false)?;
      fallback.to_css(dest, is_custom_property)?;
    }
    dest.write_char(')')
  }

  fn get_fallback(&self, kind: ColorFallbackKind) -> Self {
    EnvironmentVariable {
      name: self.name.clone(),
      indices: self.indices.clone(),
      fallback: self.fallback.as_ref().map(|fallback| fallback.get_fallback(kind)),
    }
  }
}

/// A custom CSS function.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(feature = "visitor", visit(visit_function, FUNCTIONS))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct Function<'i> {
  /// The function name.
  #[cfg_attr(feature = "serde", serde(borrow))]
  pub name: Ident<'i>,
  /// The function arguments.
  pub arguments: TokenList<'i>,
}

impl<'i> Function<'i> {
  fn to_css<W>(&self, dest: &mut Printer<W>, is_custom_property: bool) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self.name.as_ref() {
      "global" if !is_custom_property => self.arguments.to_css(dest, is_custom_property),
      _ => {
        self.name.to_css(dest)?;
        dest.write_char('(')?;
        self.arguments.to_css(dest, is_custom_property)?;
        dest.write_char(')')
      }
    }
  }

  fn get_fallback(&self, kind: ColorFallbackKind) -> Self {
    Function {
      name: self.name.clone(),
      arguments: self.arguments.get_fallback(kind),
    }
  }
}

/// A color value with an unresolved alpha value (e.g. a variable).
/// These can be converted from the modern slash syntax to older comma syntax.
/// This can only be done when the only unresolved component is the alpha
/// since variables can resolve to multiple tokens.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", rename_all = "lowercase")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum UnresolvedColor<'i> {
  /// An rgb() color.
  RGB {
    /// The red component.
    r: f32,
    /// The green component.
    g: f32,
    /// The blue component.
    b: f32,
    /// The unresolved alpha component.
    #[cfg_attr(feature = "serde", serde(borrow))]
    alpha: TokenList<'i>,
  },
  /// An hsl() color.
  HSL {
    /// The hue component.
    h: f32,
    /// The saturation component.
    s: f32,
    /// The lightness component.
    l: f32,
    /// The unresolved alpha component.
    #[cfg_attr(feature = "serde", serde(borrow))]
    alpha: TokenList<'i>,
  },
  /// The light-dark() function.
  #[cfg_attr(feature = "serde", serde(rename = "light-dark"))]
  LightDark {
    /// The light value.
    light: TokenList<'i>,
    /// The dark value.
    dark: TokenList<'i>,
  },
}

impl<'i> LightDarkColor for UnresolvedColor<'i> {
  #[inline]
  fn light_dark(light: Self, dark: Self) -> Self {
    UnresolvedColor::LightDark {
      light: TokenList(vec![TokenOrValue::UnresolvedColor(light)]),
      dark: TokenList(vec![TokenOrValue::UnresolvedColor(dark)]),
    }
  }
}

impl<'i> UnresolvedColor<'i> {
  fn parse<'t>(
    f: &CowArcStr<'i>,
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let mut parser = ComponentParser::new(false);
    match_ignore_ascii_case! { &*f,
      "rgb" => {
        input.parse_nested_block(|input| {
          parser.parse_relative::<SRGB, _, _>(input, |input, parser| {
            let (r, g, b, is_legacy) = parse_rgb_components(input, parser)?;
            if is_legacy {
              return Err(input.new_custom_error(ParserError::InvalidValue))
            }
            input.expect_delim('/')?;
            let alpha = TokenList::parse(input, options, 0)?;
            Ok(UnresolvedColor::RGB { r, g, b, alpha })
          })
        })
      },
      "hsl" => {
        input.parse_nested_block(|input| {
          parser.parse_relative::<HSL, _, _>(input, |input, parser| {
            let (h, s, l, is_legacy) = parse_hsl_hwb_components::<HSL>(input, parser, false)?;
            if is_legacy {
              return Err(input.new_custom_error(ParserError::InvalidValue))
            }
            input.expect_delim('/')?;
            let alpha = TokenList::parse(input, options, 0)?;
            Ok(UnresolvedColor::HSL { h, s, l, alpha })
          })
        })
      },
      "light-dark" => {
        input.parse_nested_block(|input| {
          let light = input.parse_until_before(Delimiter::Comma, |input|
            TokenList::parse(input, options, 0)
          )?;
          input.expect_comma()?;
          let dark = TokenList::parse(input, options, 0)?;
          Ok(UnresolvedColor::LightDark { light, dark })
        })
      },
      _ => Err(input.new_custom_error(ParserError::InvalidValue))
    }
  }

  fn to_css<W>(&self, dest: &mut Printer<W>, is_custom_property: bool) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    #[inline]
    fn c(c: &f32) -> i32 {
      (c * 255.0).round().clamp(0.0, 255.0) as i32
    }

    match self {
      UnresolvedColor::RGB { r, g, b, alpha } => {
        if should_compile!(dest.targets, SpaceSeparatedColorNotation) {
          dest.write_str("rgba(")?;
          c(r).to_css(dest)?;
          dest.delim(',', false)?;
          c(g).to_css(dest)?;
          dest.delim(',', false)?;
          c(b).to_css(dest)?;
          dest.delim(',', false)?;
          alpha.to_css(dest, is_custom_property)?;
          dest.write_char(')')?;
          return Ok(());
        }

        dest.write_str("rgb(")?;
        c(r).to_css(dest)?;
        dest.write_char(' ')?;
        c(g).to_css(dest)?;
        dest.write_char(' ')?;
        c(b).to_css(dest)?;
        dest.delim('/', true)?;
        alpha.to_css(dest, is_custom_property)?;
        dest.write_char(')')
      }
      UnresolvedColor::HSL { h, s, l, alpha } => {
        if should_compile!(dest.targets, SpaceSeparatedColorNotation) {
          dest.write_str("hsla(")?;
          h.to_css(dest)?;
          dest.delim(',', false)?;
          Percentage(*s).to_css(dest)?;
          dest.delim(',', false)?;
          Percentage(*l).to_css(dest)?;
          dest.delim(',', false)?;
          alpha.to_css(dest, is_custom_property)?;
          dest.write_char(')')?;
          return Ok(());
        }

        dest.write_str("hsl(")?;
        h.to_css(dest)?;
        dest.write_char(' ')?;
        Percentage(*s).to_css(dest)?;
        dest.write_char(' ')?;
        Percentage(*l).to_css(dest)?;
        dest.delim('/', true)?;
        alpha.to_css(dest, is_custom_property)?;
        dest.write_char(')')
      }
      UnresolvedColor::LightDark { light, dark } => {
        if should_compile!(dest.targets, LightDark) {
          dest.write_str("var(--lightningcss-light")?;
          dest.delim(',', false)?;
          light.to_css(dest, is_custom_property)?;
          dest.write_char(')')?;
          dest.whitespace()?;
          dest.write_str("var(--lightningcss-dark")?;
          dest.delim(',', false)?;
          dark.to_css(dest, is_custom_property)?;
          return dest.write_char(')');
        }

        dest.write_str("light-dark(")?;
        light.to_css(dest, is_custom_property)?;
        dest.delim(',', false)?;
        dark.to_css(dest, is_custom_property)?;
        dest.write_char(')')
      }
    }
  }
}
