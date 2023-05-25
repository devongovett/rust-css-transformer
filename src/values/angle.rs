//! CSS angle values.

use super::calc::Calc;
use super::length::serialize_dimension;
use super::number::CSSNumber;
use super::percentage::DimensionPercentage;
use crate::error::{ParserError, PrinterError};
use crate::printer::Printer;
use crate::traits::{
  impl_op,
  private::{AddInternal, TryAdd},
  Map, Op, Parse, Sign, ToCss, Zero,
};
#[cfg(feature = "visitor")]
use crate::visitor::Visit;
use cssparser::*;
use std::f32::consts::PI;

/// A CSS [`<angle>`](https://www.w3.org/TR/css-values-4/#angles) value.
///
/// Angles may be explicit or computed by `calc()`, but are always stored and serialized
/// as their computed value.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "visitor", visit(visit_angle, ANGLES))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum Angle {
  /// An angle in degrees. There are 360 degrees in a full circle.
  Deg(CSSNumber),
  /// An angle in radians. There are 2π radians in a full circle.
  Rad(CSSNumber),
  /// An angle in gradians. There are 400 gradians in a full circle.
  Grad(CSSNumber),
  /// An angle in turns. There is 1 turn in a full circle.
  Turn(CSSNumber),
}

impl<'i> Parse<'i> for Angle {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    Self::parse_internal(input, false)
  }
}

impl Angle {
  /// Parses an angle, allowing unitless zero values.
  pub fn parse_with_unitless_zero<'i, 't>(
    input: &mut Parser<'i, 't>,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    Self::parse_internal(input, true)
  }

  fn parse_internal<'i, 't>(
    input: &mut Parser<'i, 't>,
    allow_unitless_zero: bool,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    match input.try_parse(Calc::parse) {
      Ok(Calc::Value(v)) => return Ok(*v),
      // Angles are always compatible, so they will always compute to a value.
      Ok(_) => return Err(input.new_custom_error(ParserError::InvalidValue)),
      _ => {}
    }

    let location = input.current_source_location();
    let token = input.next()?;
    match *token {
      Token::Dimension { value, ref unit, .. } => {
        match_ignore_ascii_case! { unit,
          "deg" => Ok(Angle::Deg(value)),
          "grad" => Ok(Angle::Grad(value)),
          "turn" => Ok(Angle::Turn(value)),
          "rad" => Ok(Angle::Rad(value)),
          _ => return Err(location.new_unexpected_token_error(token.clone())),
        }
      }
      Token::Number { value, .. } if value == 0.0 && allow_unitless_zero => Ok(Angle::zero()),
      ref token => return Err(location.new_unexpected_token_error(token.clone())),
    }
  }
}

impl<'i> TryFrom<&Token<'i>> for Angle {
  type Error = ();

  fn try_from(token: &Token) -> Result<Self, Self::Error> {
    match token {
      Token::Dimension { value, ref unit, .. } => match_ignore_ascii_case! { unit,
        "deg" => Ok(Angle::Deg(*value)),
        "grad" => Ok(Angle::Grad(*value)),
        "turn" => Ok(Angle::Turn(*value)),
        "rad" => Ok(Angle::Rad(*value)),
        _ => Err(()),
      },
      _ => Err(()),
    }
  }
}

impl ToCss for Angle {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    let (value, unit) = match self {
      Angle::Deg(val) => (*val, "deg"),
      Angle::Grad(val) => (*val, "grad"),
      Angle::Rad(val) => {
        let deg = self.to_degrees();
        // We print 5 digits of precision by default.
        // Switch to degrees if there are an even number of them.
        if (deg * 100000.0).round().fract() == 0.0 {
          (deg, "deg")
        } else {
          (*val, "rad")
        }
      }
      Angle::Turn(val) => (*val, "turn"),
    };

    serialize_dimension(value, unit, dest)
  }
}

impl Angle {
  /// Prints the angle, allowing unitless zero values.
  pub fn to_css_with_unitless_zero<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    if self.is_zero() {
      (0.0).to_css(dest)
    } else {
      self.to_css(dest)
    }
  }
}

impl Angle {
  /// Returns the angle in radians.
  pub fn to_radians(&self) -> CSSNumber {
    const RAD_PER_DEG: f32 = PI / 180.0;
    match self {
      Angle::Deg(deg) => deg * RAD_PER_DEG,
      Angle::Rad(rad) => *rad,
      Angle::Grad(grad) => grad * 180.0 / 200.0 * RAD_PER_DEG,
      Angle::Turn(turn) => turn * 360.0 * RAD_PER_DEG,
    }
  }

  /// Returns the angle in degrees.
  pub fn to_degrees(&self) -> CSSNumber {
    const DEG_PER_RAD: f32 = 180.0 / PI;
    match self {
      Angle::Deg(deg) => *deg,
      Angle::Rad(rad) => rad * DEG_PER_RAD,
      Angle::Grad(grad) => grad * 180.0 / 200.0,
      Angle::Turn(turn) => turn * 360.0,
    }
  }
}

impl Zero for Angle {
  fn is_zero(&self) -> bool {
    use Angle::*;
    match self {
      Deg(v) | Rad(v) | Grad(v) | Turn(v) => *v == 0.0,
    }
  }

  fn zero() -> Self {
    Angle::Deg(0.0)
  }
}

impl Into<Calc<Angle>> for Angle {
  fn into(self) -> Calc<Angle> {
    Calc::Value(Box::new(self))
  }
}

impl TryFrom<Calc<Angle>> for Angle {
  type Error = ();

  fn try_from(calc: Calc<Angle>) -> Result<Angle, ()> {
    match calc {
      Calc::Value(v) => Ok(*v),
      _ => Err(()),
    }
  }
}

impl std::ops::Mul<CSSNumber> for Angle {
  type Output = Self;

  fn mul(self, other: CSSNumber) -> Angle {
    match self {
      Angle::Deg(v) => Angle::Deg(v * other),
      Angle::Rad(v) => Angle::Rad(v * other),
      Angle::Grad(v) => Angle::Grad(v * other),
      Angle::Turn(v) => Angle::Turn(v * other),
    }
  }
}

impl AddInternal for Angle {
  fn add(self, other: Self) -> Self {
    self + other
  }
}

impl TryAdd<Angle> for Angle {
  fn try_add(&self, other: &Angle) -> Option<Angle> {
    Some(Angle::Deg(self.to_degrees() + other.to_degrees()))
  }
}

impl std::cmp::PartialEq<Angle> for Angle {
  fn eq(&self, other: &Angle) -> bool {
    self.to_degrees() == other.to_degrees()
  }
}

impl std::cmp::PartialOrd<Angle> for Angle {
  fn partial_cmp(&self, other: &Angle) -> Option<std::cmp::Ordering> {
    self.to_degrees().partial_cmp(&other.to_degrees())
  }
}

impl Op for Angle {
  fn op<F: FnOnce(f32, f32) -> f32>(&self, other: &Self, op: F) -> Self {
    match (self, other) {
      (Angle::Deg(a), Angle::Deg(b)) => Angle::Deg(op(*a, *b)),
      (Angle::Rad(a), Angle::Rad(b)) => Angle::Rad(op(*a, *b)),
      (Angle::Grad(a), Angle::Grad(b)) => Angle::Grad(op(*a, *b)),
      (Angle::Turn(a), Angle::Turn(b)) => Angle::Turn(op(*a, *b)),
      (a, b) => Angle::Deg(op(a.to_degrees(), b.to_degrees())),
    }
  }

  fn op_to<T, F: FnOnce(f32, f32) -> T>(&self, other: &Self, op: F) -> T {
    match (self, other) {
      (Angle::Deg(a), Angle::Deg(b)) => op(*a, *b),
      (Angle::Rad(a), Angle::Rad(b)) => op(*a, *b),
      (Angle::Grad(a), Angle::Grad(b)) => op(*a, *b),
      (Angle::Turn(a), Angle::Turn(b)) => op(*a, *b),
      (a, b) => op(a.to_degrees(), b.to_degrees()),
    }
  }
}

impl Map for Angle {
  fn map<F: FnOnce(f32) -> f32>(&self, op: F) -> Self {
    match self {
      Angle::Deg(deg) => Angle::Deg(op(*deg)),
      Angle::Rad(rad) => Angle::Rad(op(*rad)),
      Angle::Grad(grad) => Angle::Grad(op(*grad)),
      Angle::Turn(turn) => Angle::Turn(op(*turn)),
    }
  }
}

impl Sign for Angle {
  fn sign(&self) -> f32 {
    match self {
      Angle::Deg(v) | Angle::Rad(v) | Angle::Grad(v) | Angle::Turn(v) => v.sign(),
    }
  }
}

impl_op!(Angle, std::ops::Rem, rem);
impl_op!(Angle, std::ops::Add, add);

/// A CSS [`<angle-percentage>`](https://www.w3.org/TR/css-values-4/#typedef-angle-percentage) value.
/// May be specified as either an angle or a percentage that resolves to an angle.
pub type AnglePercentage = DimensionPercentage<Angle>;

macro_rules! impl_try_from_angle {
  ($t: ty) => {
    impl TryFrom<crate::values::angle::Angle> for $t {
      type Error = ();
      fn try_from(_: crate::values::angle::Angle) -> Result<Self, Self::Error> {
        Err(())
      }
    }

    impl TryInto<crate::values::angle::Angle> for $t {
      type Error = ();
      fn try_into(self) -> Result<crate::values::angle::Angle, Self::Error> {
        Err(())
      }
    }
  };
}

pub(crate) use impl_try_from_angle;
