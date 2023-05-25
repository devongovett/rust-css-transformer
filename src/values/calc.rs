//! Mathematical calculation functions and expressions.

use crate::compat::Feature;
use crate::error::{ParserError, PrinterError};
use crate::macros::enum_property;
use crate::printer::Printer;
use crate::targets::Browsers;
use crate::traits::private::AddInternal;
use crate::traits::{IsCompatible, Parse, Sign, ToCss, TryMap, TryOp, TrySign};
#[cfg(feature = "visitor")]
use crate::visitor::Visit;
use cssparser::*;

use super::angle::Angle;
use super::length::Length;
use super::number::CSSNumber;
use super::percentage::Percentage;
use super::time::Time;

/// A CSS [math function](https://www.w3.org/TR/css-values-4/#math-function).
///
/// Math functions may be used in most properties and values that accept numeric
/// values, including lengths, percentages, angles, times, etc.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum MathFunction<V> {
  /// The [`calc()`](https://www.w3.org/TR/css-values-4/#calc-func) function.
  Calc(Calc<V>),
  /// The [`min()`](https://www.w3.org/TR/css-values-4/#funcdef-min) function.
  Min(Vec<Calc<V>>),
  /// The [`max()`](https://www.w3.org/TR/css-values-4/#funcdef-max) function.
  Max(Vec<Calc<V>>),
  /// The [`clamp()`](https://www.w3.org/TR/css-values-4/#funcdef-clamp) function.
  Clamp(Calc<V>, Calc<V>, Calc<V>),
  /// The [`round()`](https://www.w3.org/TR/css-values-4/#funcdef-round) function.
  Round(RoundingStrategy, Calc<V>, Calc<V>),
  /// The [`rem()`](https://www.w3.org/TR/css-values-4/#funcdef-rem) function.
  Rem(Calc<V>, Calc<V>),
  /// The [`mod()`](https://www.w3.org/TR/css-values-4/#funcdef-mod) function.
  Mod(Calc<V>, Calc<V>),
  /// The [`abs()`](https://drafts.csswg.org/css-values-4/#funcdef-abs) function.
  Abs(Calc<V>),
  /// The [`sign()`](https://drafts.csswg.org/css-values-4/#funcdef-sign) function.
  Sign(Calc<V>),
  /// The [`hypot()`](https://drafts.csswg.org/css-values-4/#funcdef-hypot) function.
  Hypot(Vec<Calc<V>>),
}

impl<V: IsCompatible> IsCompatible for MathFunction<V> {
  fn is_compatible(&self, browsers: Browsers) -> bool {
    match self {
      MathFunction::Calc(v) => Feature::Calc.is_compatible(browsers) && v.is_compatible(browsers),
      MathFunction::Min(v) => {
        Feature::MinFunction.is_compatible(browsers) && v.iter().all(|v| v.is_compatible(browsers))
      }
      MathFunction::Max(v) => {
        Feature::MaxFunction.is_compatible(browsers) && v.iter().all(|v| v.is_compatible(browsers))
      }
      MathFunction::Clamp(a, b, c) => {
        Feature::Clamp.is_compatible(browsers)
          && a.is_compatible(browsers)
          && b.is_compatible(browsers)
          && c.is_compatible(browsers)
      }
      MathFunction::Round(_, a, b) => {
        Feature::RoundFunction.is_compatible(browsers) && a.is_compatible(browsers) && b.is_compatible(browsers)
      }
      MathFunction::Rem(a, b) => {
        Feature::RemFunction.is_compatible(browsers) && a.is_compatible(browsers) && b.is_compatible(browsers)
      }
      MathFunction::Mod(a, b) => {
        Feature::ModFunction.is_compatible(browsers) && a.is_compatible(browsers) && b.is_compatible(browsers)
      }
      MathFunction::Abs(v) => Feature::AbsFunction.is_compatible(browsers) && v.is_compatible(browsers),
      MathFunction::Sign(v) => Feature::SignFunction.is_compatible(browsers) && v.is_compatible(browsers),
      MathFunction::Hypot(v) => {
        Feature::HypotFunction.is_compatible(browsers) && v.iter().all(|v| v.is_compatible(browsers))
      }
    }
  }
}

enum_property! {
  /// A [rounding strategy](https://www.w3.org/TR/css-values-4/#typedef-rounding-strategy),
  /// as used in the `round()` function.
  pub enum RoundingStrategy {
    /// Round to the nearest integer.
    "nearest": Nearest,
    /// Round up (ceil).
    "up": Up,
    /// Round down (floor).
    "down": Down,
    /// Round toward zero (truncate).
    "to-zero": ToZero,
  }
}

impl Default for RoundingStrategy {
  fn default() -> Self {
    RoundingStrategy::Nearest
  }
}

fn round(value: f32, to: f32, strategy: RoundingStrategy) -> f32 {
  let v = value / to;
  match strategy {
    RoundingStrategy::Down => v.floor() * to,
    RoundingStrategy::Up => v.ceil() * to,
    RoundingStrategy::Nearest => v.round() * to,
    RoundingStrategy::ToZero => v.trunc() * to,
  }
}

fn modulo(a: f32, b: f32) -> f32 {
  ((a % b) + b) % b
}

impl<V: ToCss + std::ops::Mul<f32, Output = V> + TrySign + Clone + std::fmt::Debug> ToCss for MathFunction<V> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self {
      MathFunction::Calc(calc) => {
        dest.write_str("calc(")?;
        calc.to_css(dest)?;
        dest.write_char(')')
      }
      MathFunction::Min(args) => {
        dest.write_str("min(")?;
        let mut first = true;
        for arg in args {
          if first {
            first = false;
          } else {
            dest.delim(',', false)?;
          }
          arg.to_css(dest)?;
        }
        dest.write_char(')')
      }
      MathFunction::Max(args) => {
        dest.write_str("max(")?;
        let mut first = true;
        for arg in args {
          if first {
            first = false;
          } else {
            dest.delim(',', false)?;
          }
          arg.to_css(dest)?;
        }
        dest.write_char(')')
      }
      MathFunction::Clamp(a, b, c) => {
        // If clamp() is unsupported by targets, output min()/max()
        if let Some(targets) = dest.targets {
          if !Feature::Clamp.is_compatible(targets) {
            dest.write_str("max(")?;
            a.to_css(dest)?;
            dest.delim(',', false)?;
            dest.write_str("min(")?;
            b.to_css(dest)?;
            dest.delim(',', false)?;
            c.to_css(dest)?;
            dest.write_str("))")?;
            return Ok(());
          }
        }

        dest.write_str("clamp(")?;
        a.to_css(dest)?;
        dest.delim(',', false)?;
        b.to_css(dest)?;
        dest.delim(',', false)?;
        c.to_css(dest)?;
        dest.write_char(')')
      }
      MathFunction::Round(strategy, a, b) => {
        dest.write_str("round(")?;
        if *strategy != RoundingStrategy::default() {
          strategy.to_css(dest)?;
          dest.delim(',', false)?;
        }
        a.to_css(dest)?;
        dest.delim(',', false)?;
        b.to_css(dest)?;
        dest.write_char(')')
      }
      MathFunction::Rem(a, b) => {
        dest.write_str("rem(")?;
        a.to_css(dest)?;
        dest.delim(',', false)?;
        b.to_css(dest)?;
        dest.write_char(')')
      }
      MathFunction::Mod(a, b) => {
        dest.write_str("mod(")?;
        a.to_css(dest)?;
        dest.delim(',', false)?;
        b.to_css(dest)?;
        dest.write_char(')')
      }
      MathFunction::Abs(v) => {
        dest.write_str("abs(")?;
        v.to_css(dest)?;
        dest.write_char(')')
      }
      MathFunction::Sign(v) => {
        dest.write_str("sign(")?;
        v.to_css(dest)?;
        dest.write_char(')')
      }
      MathFunction::Hypot(args) => {
        dest.write_str("hypot(")?;
        let mut first = true;
        for arg in args {
          if first {
            first = false;
          } else {
            dest.delim(',', false)?;
          }
          arg.to_css(dest)?;
        }
        dest.write_char(')')
      }
    }
  }
}

/// A mathematical expression used within the [`calc()`](https://www.w3.org/TR/css-values-4/#calc-func) function.
///
/// This type supports generic value types. Values such as [Length](super::length::Length), [Percentage](super::percentage::Percentage),
/// [Time](super::time::Time), and [Angle](super::angle::Angle) support `calc()` expressions.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum Calc<V> {
  /// A literal value.
  Value(Box<V>),
  /// A literal number.
  Number(CSSNumber),
  /// A sum of two calc expressions.
  #[cfg_attr(feature = "visitor", skip_type)]
  Sum(Box<Calc<V>>, Box<Calc<V>>),
  /// A product of a number and another calc expression.
  #[cfg_attr(feature = "visitor", skip_type)]
  Product(CSSNumber, Box<Calc<V>>),
  /// A math function, such as `calc()`, `min()`, or `max()`.
  #[cfg_attr(feature = "visitor", skip_type)]
  Function(Box<MathFunction<V>>),
}

impl<V: IsCompatible> IsCompatible for Calc<V> {
  fn is_compatible(&self, browsers: Browsers) -> bool {
    match self {
      Calc::Sum(a, b) => a.is_compatible(browsers) && b.is_compatible(browsers),
      Calc::Product(_, v) => v.is_compatible(browsers),
      Calc::Function(f) => f.is_compatible(browsers),
      Calc::Value(v) => v.is_compatible(browsers),
      Calc::Number(..) => true,
    }
  }
}

enum_property! {
  /// A mathematical constant.
  pub enum Constant {
    /// The base of the natural logarithm
    "e": E,
    /// The ratio of a circle’s circumference to its diameter
    "pi": Pi,
    /// infinity
    "infinity": Infinity,
    /// -infinity
    "-infinity": NegativeInfinity,
    /// Not a number.
    "nan": Nan,
  }
}

impl Into<f32> for Constant {
  fn into(self) -> f32 {
    use std::f32::consts;
    use Constant::*;
    match self {
      E => consts::E,
      Pi => consts::PI,
      Infinity => f32::INFINITY,
      NegativeInfinity => -f32::INFINITY,
      Nan => f32::NAN,
    }
  }
}

impl<
    'i,
    V: Parse<'i>
      + std::ops::Mul<f32, Output = V>
      + AddInternal
      + TryOp
      + TryMap
      + TrySign
      + std::cmp::PartialOrd<V>
      + Into<Calc<V>>
      + TryFrom<Calc<V>>
      + TryFrom<Angle>
      + TryInto<Angle>
      + Clone
      + std::fmt::Debug,
  > Parse<'i> for Calc<V>
{
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    Self::parse_with(input, |_| None)
  }
}

impl<
    'i,
    V: Parse<'i>
      + std::ops::Mul<f32, Output = V>
      + AddInternal
      + TryOp
      + TryMap
      + TrySign
      + std::cmp::PartialOrd<V>
      + Into<Calc<V>>
      + TryFrom<Calc<V>>
      + TryFrom<Angle>
      + TryInto<Angle>
      + Clone
      + std::fmt::Debug,
  > Calc<V>
{
  pub(crate) fn parse_with<'t, Parse: Copy + Fn(&str) -> Option<Calc<V>>>(
    input: &mut Parser<'i, 't>,
    parse_ident: Parse,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let location = input.current_source_location();
    let f = input.expect_function()?;
    match_ignore_ascii_case! { &f,
      "calc" => {
        let calc = input.parse_nested_block(|input| Calc::parse_sum(input, parse_ident))?;
        match calc {
          Calc::Value(_) | Calc::Number(_) => Ok(calc),
          _ => Ok(Calc::Function(Box::new(MathFunction::Calc(calc))))
        }
      },
      "min" => {
        let mut args = input.parse_nested_block(|input| input.parse_comma_separated(|input| Calc::parse_sum(input, parse_ident)))?;
        let mut reduced = Calc::reduce_args(&mut args, std::cmp::Ordering::Less);
        if reduced.len() == 1 {
          return Ok(reduced.remove(0))
        }
        Ok(Calc::Function(Box::new(MathFunction::Min(reduced))))
      },
      "max" => {
        let mut args = input.parse_nested_block(|input| input.parse_comma_separated(|input| Calc::parse_sum(input, parse_ident)))?;
        let mut reduced = Calc::reduce_args(&mut args, std::cmp::Ordering::Greater);
        if reduced.len() == 1 {
          return Ok(reduced.remove(0))
        }
        Ok(Calc::Function(Box::new(MathFunction::Max(reduced))))
      },
      "clamp" => {
        let (mut min, mut center, mut max) = input.parse_nested_block(|input| {
          let min = Some(Calc::parse_sum(input, parse_ident)?);
          input.expect_comma()?;
          let center: Calc<V> = Calc::parse_sum(input, parse_ident)?;
          input.expect_comma()?;
          let max = Some(Calc::parse_sum(input, parse_ident)?);
          Ok((min, center, max))
        })?;

        // According to the spec, the minimum should "win" over the maximum if they are in the wrong order.
        let cmp = if let (Some(Calc::Value(max_val)), Calc::Value(center_val)) = (&max, &center) {
          center_val.partial_cmp(&max_val)
        } else {
          None
        };

        // If center is known to be greater than the maximum, replace it with maximum and remove the max argument.
        // Otherwise, if center is known to be less than the maximum, remove the max argument.
        match cmp {
          Some(std::cmp::Ordering::Greater) => {
            center = std::mem::take(&mut max).unwrap();
          }
          Some(_) => {
            max = None;
          }
          None => {}
        }

        let cmp = if let (Some(Calc::Value(min_val)), Calc::Value(center_val)) = (&min, &center) {
          center_val.partial_cmp(&min_val)
        } else {
          None
        };

        // If center is known to be less than the minimum, replace it with minimum and remove the min argument.
        // Otherwise, if center is known to be greater than the minimum, remove the min argument.
        match cmp {
          Some(std::cmp::Ordering::Less) => {
            center = std::mem::take(&mut min).unwrap();
          }
          Some(_) => {
            min = None;
          }
          None => {}
        }

        // Generate clamp(), min(), max(), or value depending on which arguments are left.
        match (min, max) {
          (None, None) => Ok(center),
          (Some(min), None) => Ok(Calc::Function(Box::new(MathFunction::Max(vec![min, center])))),
          (None, Some(max)) => Ok(Calc::Function(Box::new(MathFunction::Min(vec![center, max])))),
          (Some(min), Some(max)) => Ok(Calc::Function(Box::new(MathFunction::Clamp(min, center, max))))
        }
      },
      "round" => {
        input.parse_nested_block(|input| {
          let strategy = if let Ok(s) = input.try_parse(RoundingStrategy::parse) {
            input.expect_comma()?;
            s
          } else {
            RoundingStrategy::default()
          };

          Self::parse_math_fn(
            input,
            |a, b| round(a, b, strategy),
            |a, b| MathFunction::Round(strategy, a, b),
            parse_ident
          )
        })
      },
      "rem" => {
        input.parse_nested_block(|input| {
          Self::parse_math_fn(input, std::ops::Rem::rem, MathFunction::Rem, parse_ident)
        })
      },
      "mod" => {
        input.parse_nested_block(|input| {
          Self::parse_math_fn(input, modulo, MathFunction::Mod, parse_ident)
        })
      },
      "sin" => Self::parse_trig(input, f32::sin, false, parse_ident),
      "cos" => Self::parse_trig(input, f32::cos, false, parse_ident),
      "tan" => Self::parse_trig(input, f32::tan, false, parse_ident),
      "asin" => Self::parse_trig(input, f32::asin, true, parse_ident),
      "acos" => Self::parse_trig(input, f32::acos, true, parse_ident),
      "atan" => Self::parse_trig(input, f32::atan, true, parse_ident),
      "atan2" => {
        input.parse_nested_block(|input| {
          let res = Self::parse_atan2(input, parse_ident)?;
          if let Ok(v) = V::try_from(res) {
            return Ok(Calc::Value(Box::new(v)))
          }

          Err(input.new_custom_error(ParserError::InvalidValue))
        })
      },
      "pow" => {
        input.parse_nested_block(|input| {
          let a = Self::parse_numeric(input, parse_ident)?;
          input.expect_comma()?;
          let b = Self::parse_numeric(input, parse_ident)?;
          Ok(Calc::Number(a.powf(b)))
        })
      },
      "log" => {
        input.parse_nested_block(|input| {
          let value = Self::parse_numeric(input, parse_ident)?;
          if input.try_parse(|input| input.expect_comma()).is_ok() {
            let base = Self::parse_numeric(input, parse_ident)?;
            Ok(Calc::Number(value.log(base)))
          } else {
            Ok(Calc::Number(value.ln()))
          }
        })
      },
      "sqrt" => Self::parse_numeric_fn(input, f32::sqrt, parse_ident),
      "exp" => Self::parse_numeric_fn(input, f32::exp, parse_ident),
      "hypot" => {
        input.parse_nested_block(|input| {
          let args: Vec<Self> = input.parse_comma_separated(|input| Calc::parse_sum(input, parse_ident))?;
          Self::parse_hypot(&args)?
            .map_or_else(
              || Ok(Calc::Function(Box::new(MathFunction::Hypot(args)))),
              |v| Ok(v)
            )
        })
      },
      "abs" => {
        input.parse_nested_block(|input| {
          let v: Calc<V> = Self::parse_sum(input, parse_ident)?;
          Self::apply_map(&v, f32::abs)
            .map_or_else(
              || Ok(Calc::Function(Box::new(MathFunction::Abs(v)))),
              |v| Ok(v)
            )
        })
      },
      "sign" => {
        input.parse_nested_block(|input| {
          let v: Calc<V> = Self::parse_sum(input, parse_ident)?;
          match &v {
            Calc::Number(n) => return Ok(Calc::Number(n.sign())),
            Calc::Value(v) => {
              // First map so we ignore percentages, which must be resolved to their
              // computed value in order to determine the sign.
              if let Some(v) = v.try_map(|s| s.sign()) {
                // sign() always resolves to a number.
                return Ok(Calc::Number(v.try_sign().unwrap()));
              }
            }
            _ => {}
          }

          Ok(Calc::Function(Box::new(MathFunction::Sign(v))))
        })
      },
       _ => Err(location.new_unexpected_token_error(Token::Ident(f.clone()))),
    }
  }

  fn parse_sum<'t, Parse: Copy + Fn(&str) -> Option<Calc<V>>>(
    input: &mut Parser<'i, 't>,
    parse_ident: Parse,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let mut cur: Calc<V> = Calc::parse_product(input, parse_ident)?;
    loop {
      let start = input.state();
      match input.next_including_whitespace() {
        Ok(&Token::WhiteSpace(_)) => {
          if input.is_exhausted() {
            break; // allow trailing whitespace
          }
          match *input.next()? {
            Token::Delim('+') => {
              let next = Calc::parse_product(input, parse_ident)?;
              cur = cur.add(next).map_err(|_| input.new_custom_error(ParserError::InvalidValue))?;
            }
            Token::Delim('-') => {
              let mut rhs = Calc::parse_product(input, parse_ident)?;
              rhs = rhs * -1.0;
              cur = cur.add(rhs).map_err(|_| input.new_custom_error(ParserError::InvalidValue))?;
            }
            ref t => {
              let t = t.clone();
              return Err(input.new_unexpected_token_error(t));
            }
          }
        }
        _ => {
          input.reset(&start);
          break;
        }
      }
    }
    Ok(cur)
  }

  fn parse_product<'t, Parse: Copy + Fn(&str) -> Option<Calc<V>>>(
    input: &mut Parser<'i, 't>,
    parse_ident: Parse,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let mut node = Calc::parse_value(input, parse_ident)?;
    loop {
      let start = input.state();
      match input.next() {
        Ok(&Token::Delim('*')) => {
          // At least one of the operands must be a number.
          let rhs = Self::parse_value(input, parse_ident)?;
          if let Calc::Number(val) = rhs {
            node = node * val;
          } else if let Calc::Number(val) = node {
            node = rhs;
            node = node * val;
          } else {
            return Err(input.new_unexpected_token_error(Token::Delim('*')));
          }
        }
        Ok(&Token::Delim('/')) => {
          let rhs = Self::parse_value(input, parse_ident)?;
          if let Calc::Number(val) = rhs {
            if val != 0.0 {
              node = node * (1.0 / val);
              continue;
            }
          }
          return Err(input.new_custom_error(ParserError::InvalidValue));
        }
        _ => {
          input.reset(&start);
          break;
        }
      }
    }
    Ok(node)
  }

  fn parse_value<'t, Parse: Copy + Fn(&str) -> Option<Calc<V>>>(
    input: &mut Parser<'i, 't>,
    parse_ident: Parse,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    // Parse nested calc() and other math functions.
    if let Ok(calc) = input.try_parse(Self::parse) {
      match calc {
        Calc::Function(f) => {
          return Ok(match *f {
            MathFunction::Calc(c) => c,
            _ => Calc::Function(f),
          })
        }
        c => return Ok(c),
      }
    }

    if input.try_parse(|input| input.expect_parenthesis_block()).is_ok() {
      return input.parse_nested_block(|input| Calc::parse_sum(input, parse_ident));
    }

    if let Ok(num) = input.try_parse(|input| input.expect_number()) {
      return Ok(Calc::Number(num));
    }

    if let Ok(constant) = input.try_parse(Constant::parse) {
      return Ok(Calc::Number(constant.into()));
    }

    let location = input.current_source_location();
    if let Ok(ident) = input.try_parse(|input| input.expect_ident_cloned()) {
      if let Some(v) = parse_ident(ident.as_ref()) {
        return Ok(v);
      }

      return Err(location.new_unexpected_token_error(Token::Ident(ident.clone())));
    }

    let value = input.try_parse(V::parse)?;
    Ok(Calc::Value(Box::new(value)))
  }

  fn reduce_args(args: &mut Vec<Calc<V>>, cmp: std::cmp::Ordering) -> Vec<Calc<V>> {
    // Reduces the arguments of a min() or max() expression, combining compatible values.
    // e.g. min(1px, 1em, 2px, 3in) => min(1px, 1em)
    let mut reduced: Vec<Calc<V>> = vec![];
    for arg in args.drain(..) {
      let mut found = None;
      match &arg {
        Calc::Value(val) => {
          for b in reduced.iter_mut() {
            if let Calc::Value(v) = b {
              match val.partial_cmp(v) {
                Some(ord) if ord == cmp => {
                  found = Some(Some(b));
                  break;
                }
                Some(_) => {
                  found = Some(None);
                  break;
                }
                None => {}
              }
            }
          }
        }
        _ => {}
      }
      if let Some(r) = found {
        if let Some(r) = r {
          *r = arg
        }
      } else {
        reduced.push(arg)
      }
    }
    reduced
  }

  fn parse_math_fn<
    't,
    O: FnOnce(f32, f32) -> f32,
    F: FnOnce(Calc<V>, Calc<V>) -> MathFunction<V>,
    Parse: Copy + Fn(&str) -> Option<Calc<V>>,
  >(
    input: &mut Parser<'i, 't>,
    op: O,
    fallback: F,
    parse_ident: Parse,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let a: Calc<V> = Calc::parse_sum(input, parse_ident)?;
    input.expect_comma()?;
    let b: Calc<V> = Calc::parse_sum(input, parse_ident)?;

    Ok(Self::apply_op(&a, &b, op).unwrap_or_else(|| Calc::Function(Box::new(fallback(a, b)))))
  }

  fn apply_op<'t, O: FnOnce(f32, f32) -> f32>(a: &Calc<V>, b: &Calc<V>, op: O) -> Option<Self> {
    match (a, b) {
      (Calc::Value(a), Calc::Value(b)) => {
        if let Some(v) = a.try_op(&**b, op) {
          return Some(Calc::Value(Box::new(v)));
        }
      }
      (Calc::Number(a), Calc::Number(b)) => return Some(Calc::Number(op(*a, *b))),
      _ => {}
    }

    None
  }

  fn apply_map<'t, O: FnOnce(f32) -> f32>(v: &Calc<V>, op: O) -> Option<Self> {
    match v {
      Calc::Number(n) => return Some(Calc::Number(op(*n))),
      Calc::Value(v) => {
        if let Some(v) = v.try_map(op) {
          return Some(Calc::Value(Box::new(v)));
        }
      }
      _ => {}
    }

    None
  }

  fn parse_trig<'t, F: FnOnce(f32) -> f32, Parse: Copy + Fn(&str) -> Option<Calc<V>>>(
    input: &mut Parser<'i, 't>,
    f: F,
    to_angle: bool,
    parse_ident: Parse,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    input.parse_nested_block(|input| {
      let v: Calc<Angle> = Calc::parse_sum(input, |v| {
        parse_ident(v).and_then(|v| -> Option<Calc<Angle>> {
          match v {
            Calc::Number(v) => Some(Calc::Number(v)),
            Calc::Value(v) => (*v).try_into().ok().map(|v| Calc::Value(Box::new(v))),
            _ => None,
          }
        })
      })?;
      let rad = match v {
        Calc::Value(angle) if !to_angle => f(angle.to_radians()),
        Calc::Number(v) => f(v),
        _ => return Err(input.new_custom_error(ParserError::InvalidValue)),
      };

      if to_angle && !rad.is_nan() {
        if let Ok(v) = V::try_from(Angle::Rad(rad)) {
          return Ok(Calc::Value(Box::new(v)));
        } else {
          return Err(input.new_custom_error(ParserError::InvalidValue));
        }
      } else {
        Ok(Calc::Number(rad))
      }
    })
  }

  fn parse_numeric<'t, Parse: Copy + Fn(&str) -> Option<Calc<V>>>(
    input: &mut Parser<'i, 't>,
    parse_ident: Parse,
  ) -> Result<f32, ParseError<'i, ParserError<'i>>> {
    let v: Calc<CSSNumber> = Calc::parse_sum(input, |v| {
      parse_ident(v).and_then(|v| match v {
        Calc::Number(v) => Some(Calc::Number(v)),
        _ => None,
      })
    })?;
    match v {
      Calc::Number(n) => Ok(n),
      Calc::Value(v) => Ok(*v),
      _ => Err(input.new_custom_error(ParserError::InvalidValue)),
    }
  }

  fn parse_numeric_fn<'t, F: FnOnce(f32) -> f32, Parse: Copy + Fn(&str) -> Option<Calc<V>>>(
    input: &mut Parser<'i, 't>,
    f: F,
    parse_ident: Parse,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    input.parse_nested_block(|input| {
      let v = Self::parse_numeric(input, parse_ident)?;
      Ok(Calc::Number(f(v)))
    })
  }

  fn parse_atan2<'t, Parse: Copy + Fn(&str) -> Option<Calc<V>>>(
    input: &mut Parser<'i, 't>,
    parse_ident: Parse,
  ) -> Result<Angle, ParseError<'i, ParserError<'i>>> {
    // atan2 supports arguments of any <number>, <dimension>, or <percentage>, even ones that wouldn't
    // normally be supported by V. The only requirement is that the arguments be of the same type.
    // Try parsing with each type, and return the first one that parses successfully.
    if let Ok(v) = input.try_parse(|input| Calc::<Length>::parse_atan2_args(input, |_| None)) {
      return Ok(v);
    }

    if let Ok(v) = input.try_parse(|input| Calc::<Percentage>::parse_atan2_args(input, |_| None)) {
      return Ok(v);
    }

    if let Ok(v) = input.try_parse(|input| Calc::<Angle>::parse_atan2_args(input, |_| None)) {
      return Ok(v);
    }

    if let Ok(v) = input.try_parse(|input| Calc::<Time>::parse_atan2_args(input, |_| None)) {
      return Ok(v);
    }

    Calc::<CSSNumber>::parse_atan2_args(input, |v| {
      parse_ident(v).and_then(|v| match v {
        Calc::Number(v) => Some(Calc::Number(v)),
        _ => None,
      })
    })
  }

  fn parse_atan2_args<'t, Parse: Copy + Fn(&str) -> Option<Calc<V>>>(
    input: &mut Parser<'i, 't>,
    parse_ident: Parse,
  ) -> Result<Angle, ParseError<'i, ParserError<'i>>> {
    let a = Calc::<V>::parse_sum(input, parse_ident)?;
    input.expect_comma()?;
    let b = Calc::<V>::parse_sum(input, parse_ident)?;

    match (&a, &b) {
      (Calc::Value(a), Calc::Value(b)) => {
        if let Some(v) = a.try_op_to(&**b, |a, b| Angle::Rad(a.atan2(b))) {
          return Ok(v);
        }
      }
      (Calc::Number(a), Calc::Number(b)) => return Ok(Angle::Rad(a.atan2(*b))),
      _ => {}
    }

    // We don't have a way to represent arguments that aren't angles, so just error.
    // This will fall back to an unparsed property, leaving the atan2() function intact.
    Err(input.new_custom_error(ParserError::InvalidValue))
  }

  fn parse_hypot<'t>(args: &Vec<Self>) -> Result<Option<Self>, ParseError<'i, ParserError<'i>>> {
    if args.len() == 1 {
      return Ok(Some(args[0].clone()));
    }

    if args.len() == 2 {
      return Ok(Self::apply_op(&args[0], &args[1], f32::hypot));
    }

    let mut iter = args.iter();
    let first = match Self::apply_map(&iter.next().unwrap(), |v| v.powi(2)) {
      Some(v) => v,
      None => return Ok(None),
    };
    let sum = iter.try_fold(first, |acc, arg| {
      Self::apply_op(&acc, &arg, |a, b| a + b.powi(2)).map_or_else(|| Err(()), |v| Ok(v))
    });

    let sum = match sum {
      Ok(s) => s,
      Err(_) => return Ok(None),
    };

    Ok(Self::apply_map(&sum, f32::sqrt))
  }
}

impl<V: std::ops::Mul<f32, Output = V>> std::ops::Mul<f32> for Calc<V> {
  type Output = Self;

  fn mul(self, other: f32) -> Self {
    if other == 1.0 {
      return self;
    }

    match self {
      Calc::Value(v) => Calc::Value(Box::new(*v * other)),
      Calc::Number(n) => Calc::Number(n * other),
      Calc::Sum(a, b) => Calc::Sum(Box::new(*a * other), Box::new(*b * other)),
      Calc::Product(num, calc) => {
        let num = num * other;
        if num == 1.0 {
          return *calc;
        }
        Calc::Product(num, calc)
      }
      Calc::Function(f) => match *f {
        MathFunction::Calc(c) => Calc::Function(Box::new(MathFunction::Calc(c * other))),
        _ => Calc::Product(other, Box::new(Calc::Function(f))),
      },
    }
  }
}

impl<V: AddInternal + std::convert::Into<Calc<V>> + std::convert::TryFrom<Calc<V>> + std::fmt::Debug> Calc<V> {
  pub(crate) fn add(self, other: Calc<V>) -> Result<Calc<V>, <V as TryFrom<Calc<V>>>::Error> {
    Ok(match (self, other) {
      (Calc::Value(a), Calc::Value(b)) => (a.add(*b)).into(),
      (Calc::Number(a), Calc::Number(b)) => Calc::Number(a + b),
      (Calc::Value(a), b) => (a.add(V::try_from(b)?)).into(),
      (a, Calc::Value(b)) => (V::try_from(a)?.add(*b)).into(),
      (Calc::Function(a), b) => Calc::Sum(Box::new(Calc::Function(a)), Box::new(b)),
      (a, Calc::Function(b)) => Calc::Sum(Box::new(a), Box::new(Calc::Function(b))),
      (a, b) => V::try_from(a)?.add(V::try_from(b)?).into(),
    })
  }
}

impl<V: ToCss + std::ops::Mul<f32, Output = V> + TrySign + Clone + std::fmt::Debug> ToCss for Calc<V> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    let was_in_calc = dest.in_calc;
    dest.in_calc = true;

    let res = match self {
      Calc::Value(v) => v.to_css(dest),
      Calc::Number(n) => n.to_css(dest),
      Calc::Sum(a, b) => {
        a.to_css(dest)?;
        // Whitespace is always required.
        let b = &**b;
        if b.is_sign_negative() {
          dest.write_str(" - ")?;
          let b = b.clone() * -1.0;
          b.to_css(dest)
        } else {
          dest.write_str(" + ")?;
          b.to_css(dest)
        }
      }
      Calc::Product(num, calc) => {
        if num.abs() < 1.0 {
          let div = 1.0 / num;
          calc.to_css(dest)?;
          dest.delim('/', true)?;
          div.to_css(dest)
        } else {
          num.to_css(dest)?;
          dest.delim('*', true)?;
          calc.to_css(dest)
        }
      }
      Calc::Function(f) => f.to_css(dest),
    };

    dest.in_calc = was_in_calc;
    res
  }
}

impl<V: TrySign> TrySign for Calc<V> {
  fn try_sign(&self) -> Option<f32> {
    match self {
      Calc::Number(v) => v.try_sign(),
      Calc::Value(v) => v.try_sign(),
      _ => None,
    }
  }
}
