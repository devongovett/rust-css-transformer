//! CSS percentage values.

use super::angle::{impl_try_from_angle, Angle};
use super::calc::{Calc, MathFunction};
use super::number::CSSNumber;
use crate::error::{ParserError, PrinterError};
use crate::printer::Printer;
use crate::traits::private::AddInternal;
use crate::traits::{impl_op, private::TryAdd, Op, Parse, Sign, ToCss, TryMap, TryOp, TrySign, Zero};
#[cfg(feature = "visitor")]
use crate::visitor::Visit;
use cssparser::*;

/// A CSS [`<percentage>`](https://www.w3.org/TR/css-values-4/#percentages) value.
///
/// Percentages may be explicit or computed by `calc()`, but are always stored and serialized
/// as their computed value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct Percentage(pub CSSNumber);

impl<'i> Parse<'i> for Percentage {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    match input.try_parse(Calc::parse) {
      Ok(Calc::Value(v)) => return Ok(*v),
      // Percentages are always compatible, so they will always compute to a value.
      Ok(_) => unreachable!(),
      _ => {}
    }

    let percent = input.expect_percentage()?;
    Ok(Percentage(percent))
  }
}

impl ToCss for Percentage {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    use cssparser::ToCss;
    let int_value = if (self.0 * 100.0).fract() == 0.0 {
      Some(self.0 as i32)
    } else {
      None
    };
    let percent = Token::Percentage {
      has_sign: self.0 < 0.0,
      unit_value: self.0,
      int_value,
    };
    if self.0 != 0.0 && self.0.abs() < 0.01 {
      let mut s = String::new();
      percent.to_css(&mut s)?;
      if self.0 < 0.0 {
        dest.write_char('-')?;
        dest.write_str(s.trim_start_matches("-0"))
      } else {
        dest.write_str(s.trim_start_matches('0'))
      }
    } else {
      percent.to_css(dest)?;
      Ok(())
    }
  }
}

impl std::convert::Into<Calc<Percentage>> for Percentage {
  fn into(self) -> Calc<Percentage> {
    Calc::Value(Box::new(self))
  }
}

impl std::convert::TryFrom<Calc<Percentage>> for Percentage {
  type Error = ();

  fn try_from(calc: Calc<Percentage>) -> Result<Percentage, Self::Error> {
    match calc {
      Calc::Value(v) => Ok(*v),
      _ => Err(()),
    }
  }
}

impl std::ops::Mul<CSSNumber> for Percentage {
  type Output = Self;

  fn mul(self, other: CSSNumber) -> Percentage {
    Percentage(self.0 * other)
  }
}

impl AddInternal for Percentage {
  fn add(self, other: Self) -> Self {
    self + other
  }
}

impl std::cmp::PartialOrd<Percentage> for Percentage {
  fn partial_cmp(&self, other: &Percentage) -> Option<std::cmp::Ordering> {
    self.0.partial_cmp(&other.0)
  }
}

impl Op for Percentage {
  fn op<F: FnOnce(f32, f32) -> f32>(&self, to: &Self, op: F) -> Self {
    Percentage(op(self.0, to.0))
  }

  fn op_to<T, F: FnOnce(f32, f32) -> T>(&self, rhs: &Self, op: F) -> T {
    op(self.0, rhs.0)
  }
}

impl TryMap for Percentage {
  fn try_map<F: FnOnce(f32) -> f32>(&self, _: F) -> Option<Self> {
    // Percentages cannot be mapped because we don't know what they will resolve to.
    // For example, they might be positive or negative depending on what they are a
    // percentage of, which we don't know.
    None
  }
}

impl Zero for Percentage {
  fn zero() -> Self {
    Percentage(0.0)
  }

  fn is_zero(&self) -> bool {
    self.0.is_zero()
  }
}

impl Sign for Percentage {
  fn sign(&self) -> f32 {
    self.0.sign()
  }
}

impl_op!(Percentage, std::ops::Rem, rem);
impl_op!(Percentage, std::ops::Add, add);

impl_try_from_angle!(Percentage);

/// Either a `<number>` or `<percentage>`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum NumberOrPercentage {
  /// A percentage.
  Percentage(Percentage),
  /// A number.
  Number(CSSNumber),
}

impl<'i> Parse<'i> for NumberOrPercentage {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    if let Ok(number) = input.try_parse(CSSNumber::parse) {
      return Ok(NumberOrPercentage::Number(number));
    }

    if let Ok(percent) = input.try_parse(|input| Percentage::parse(input)) {
      return Ok(NumberOrPercentage::Percentage(percent));
    }

    Err(input.new_error_for_next_token())
  }
}

impl ToCss for NumberOrPercentage {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self {
      NumberOrPercentage::Percentage(percent) => percent.to_css(dest),
      NumberOrPercentage::Number(number) => number.to_css(dest),
    }
  }
}

impl std::convert::Into<CSSNumber> for &NumberOrPercentage {
  fn into(self) -> CSSNumber {
    match self {
      NumberOrPercentage::Number(a) => *a,
      NumberOrPercentage::Percentage(a) => a.0,
    }
  }
}

/// A generic type that allows any kind of dimension and percentage to be
/// used standalone or mixed within a `calc()` expression.
///
/// <https://drafts.csswg.org/css-values-4/#mixed-percentages>
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum DimensionPercentage<D> {
  /// An explicit dimension value.
  Dimension(D),
  /// A percentage.
  Percentage(Percentage),
  /// A `calc()` expression.
  #[cfg_attr(feature = "visitor", skip_type)]
  Calc(Box<Calc<DimensionPercentage<D>>>),
}

impl<
    'i,
    D: Parse<'i>
      + std::ops::Mul<CSSNumber, Output = D>
      + TryAdd<D>
      + Clone
      + TryOp
      + TryMap
      + Zero
      + TrySign
      + TryFrom<Angle>
      + TryInto<Angle>
      + PartialOrd<D>
      + std::fmt::Debug,
  > Parse<'i> for DimensionPercentage<D>
{
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    match input.try_parse(Calc::parse) {
      Ok(Calc::Value(v)) => return Ok(*v),
      Ok(calc) => return Ok(DimensionPercentage::Calc(Box::new(calc))),
      _ => {}
    }

    if let Ok(length) = input.try_parse(|input| D::parse(input)) {
      return Ok(DimensionPercentage::Dimension(length));
    }

    if let Ok(percent) = input.try_parse(|input| Percentage::parse(input)) {
      return Ok(DimensionPercentage::Percentage(percent));
    }

    Err(input.new_error_for_next_token())
  }
}

impl<D: std::ops::Mul<CSSNumber, Output = D>> std::ops::Mul<CSSNumber> for DimensionPercentage<D> {
  type Output = Self;

  fn mul(self, other: CSSNumber) -> DimensionPercentage<D> {
    match self {
      DimensionPercentage::Dimension(l) => DimensionPercentage::Dimension(l * other),
      DimensionPercentage::Percentage(p) => DimensionPercentage::Percentage(Percentage(p.0 * other)),
      DimensionPercentage::Calc(c) => DimensionPercentage::Calc(Box::new(*c * other)),
    }
  }
}

impl<D: TryAdd<D> + Clone + Zero + TrySign + std::fmt::Debug> std::ops::Add<DimensionPercentage<D>>
  for DimensionPercentage<D>
{
  type Output = DimensionPercentage<D>;

  fn add(self, other: DimensionPercentage<D>) -> DimensionPercentage<D> {
    // Unwrap calc(...) functions so we can add inside.
    // Then wrap the result in a calc(...) again if necessary.
    let a = unwrap_calc(self);
    let b = unwrap_calc(other);
    let res = AddInternal::add(a, b);
    match res {
      DimensionPercentage::Calc(c) => match *c {
        Calc::Value(l) => *l,
        Calc::Function(f) if !matches!(*f, MathFunction::Calc(_)) => {
          DimensionPercentage::Calc(Box::new(Calc::Function(f)))
        }
        c => DimensionPercentage::Calc(Box::new(Calc::Function(Box::new(MathFunction::Calc(c))))),
      },
      _ => res,
    }
  }
}

fn unwrap_calc<D>(v: DimensionPercentage<D>) -> DimensionPercentage<D> {
  match v {
    DimensionPercentage::Calc(c) => match *c {
      Calc::Function(f) => match *f {
        MathFunction::Calc(c) => DimensionPercentage::Calc(Box::new(c)),
        c => DimensionPercentage::Calc(Box::new(Calc::Function(Box::new(c)))),
      },
      _ => DimensionPercentage::Calc(c),
    },
    _ => v,
  }
}

impl<D: TryAdd<D> + Clone + Zero + TrySign + std::fmt::Debug> AddInternal for DimensionPercentage<D> {
  fn add(self, other: Self) -> Self {
    match self.add_recursive(&other) {
      Some(r) => r,
      None => self.add(other),
    }
  }
}

impl<D: TryAdd<D> + Clone + Zero + TrySign + std::fmt::Debug> DimensionPercentage<D> {
  fn add_recursive(&self, other: &DimensionPercentage<D>) -> Option<DimensionPercentage<D>> {
    match (self, other) {
      (DimensionPercentage::Dimension(a), DimensionPercentage::Dimension(b)) => {
        if let Some(res) = a.try_add(b) {
          Some(DimensionPercentage::Dimension(res))
        } else {
          None
        }
      }
      (DimensionPercentage::Percentage(a), DimensionPercentage::Percentage(b)) => {
        Some(DimensionPercentage::Percentage(Percentage(a.0 + b.0)))
      }
      (DimensionPercentage::Calc(a), other) => match &**a {
        Calc::Value(v) => v.add_recursive(other),
        Calc::Sum(a, b) => {
          if let Some(res) = DimensionPercentage::Calc(Box::new(*a.clone())).add_recursive(other) {
            return Some(res.add(DimensionPercentage::from(*b.clone())));
          }

          if let Some(res) = DimensionPercentage::Calc(Box::new(*b.clone())).add_recursive(other) {
            return Some(DimensionPercentage::from(*a.clone()).add(res));
          }

          None
        }
        _ => None,
      },
      (other, DimensionPercentage::Calc(b)) => match &**b {
        Calc::Value(v) => other.add_recursive(&*v),
        Calc::Sum(a, b) => {
          if let Some(res) = other.add_recursive(&DimensionPercentage::Calc(Box::new(*a.clone()))) {
            return Some(res.add(DimensionPercentage::from(*b.clone())));
          }

          if let Some(res) = other.add_recursive(&DimensionPercentage::Calc(Box::new(*b.clone()))) {
            return Some(DimensionPercentage::from(*a.clone()).add(res));
          }

          None
        }
        _ => None,
      },
      _ => None,
    }
  }

  fn add(self, other: DimensionPercentage<D>) -> DimensionPercentage<D> {
    let mut a = self;
    let mut b = other;

    if a.is_zero() {
      return b;
    }

    if b.is_zero() {
      return a;
    }

    if a.is_sign_negative() && b.is_sign_positive() {
      std::mem::swap(&mut a, &mut b);
    }

    match (a, b) {
      (DimensionPercentage::Calc(a), DimensionPercentage::Calc(b)) => {
        DimensionPercentage::Calc(Box::new(a.add(*b).unwrap()))
      }
      (DimensionPercentage::Calc(calc), b) => {
        if let Calc::Value(a) = *calc {
          a.add(b)
        } else {
          DimensionPercentage::Calc(Box::new(Calc::Sum(Box::new((*calc).into()), Box::new(b.into()))))
        }
      }
      (a, DimensionPercentage::Calc(calc)) => {
        if let Calc::Value(b) = *calc {
          a.add(*b)
        } else {
          DimensionPercentage::Calc(Box::new(Calc::Sum(Box::new(a.into()), Box::new((*calc).into()))))
        }
      }
      (a, b) => DimensionPercentage::Calc(Box::new(Calc::Sum(Box::new(a.into()), Box::new(b.into())))),
    }
  }
}

impl<D> std::convert::Into<Calc<DimensionPercentage<D>>> for DimensionPercentage<D> {
  fn into(self) -> Calc<DimensionPercentage<D>> {
    match self {
      DimensionPercentage::Calc(c) => *c,
      b => Calc::Value(Box::new(b)),
    }
  }
}

impl<D> std::convert::From<Calc<DimensionPercentage<D>>> for DimensionPercentage<D> {
  fn from(calc: Calc<DimensionPercentage<D>>) -> DimensionPercentage<D> {
    DimensionPercentage::Calc(Box::new(calc))
  }
}

impl<D: std::cmp::PartialOrd<D>> std::cmp::PartialOrd<DimensionPercentage<D>> for DimensionPercentage<D> {
  fn partial_cmp(&self, other: &DimensionPercentage<D>) -> Option<std::cmp::Ordering> {
    match (self, other) {
      (DimensionPercentage::Dimension(a), DimensionPercentage::Dimension(b)) => a.partial_cmp(b),
      (DimensionPercentage::Percentage(a), DimensionPercentage::Percentage(b)) => a.partial_cmp(b),
      _ => None,
    }
  }
}

impl<D: TryOp> TryOp for DimensionPercentage<D> {
  fn try_op<F: FnOnce(f32, f32) -> f32>(&self, rhs: &Self, op: F) -> Option<Self> {
    match (self, rhs) {
      (DimensionPercentage::Dimension(a), DimensionPercentage::Dimension(b)) => {
        a.try_op(b, op).map(DimensionPercentage::Dimension)
      }
      (DimensionPercentage::Percentage(a), DimensionPercentage::Percentage(b)) => {
        Some(DimensionPercentage::Percentage(Percentage(op(a.0, b.0))))
      }
      _ => None,
    }
  }

  fn try_op_to<T, F: FnOnce(f32, f32) -> T>(&self, rhs: &Self, op: F) -> Option<T> {
    match (self, rhs) {
      (DimensionPercentage::Dimension(a), DimensionPercentage::Dimension(b)) => a.try_op_to(b, op),
      (DimensionPercentage::Percentage(a), DimensionPercentage::Percentage(b)) => Some(op(a.0, b.0)),
      _ => None,
    }
  }
}

impl<D: TryMap> TryMap for DimensionPercentage<D> {
  fn try_map<F: FnOnce(f32) -> f32>(&self, op: F) -> Option<Self> {
    match self {
      DimensionPercentage::Dimension(v) => v.try_map(op).map(DimensionPercentage::Dimension),
      _ => None,
    }
  }
}

impl<E, D: TryFrom<Angle, Error = E>> TryFrom<Angle> for DimensionPercentage<D> {
  type Error = E;

  fn try_from(value: Angle) -> Result<Self, Self::Error> {
    Ok(DimensionPercentage::Dimension(D::try_from(value)?))
  }
}

impl<E, D: TryInto<Angle, Error = E>> TryInto<Angle> for DimensionPercentage<D> {
  type Error = ();

  fn try_into(self) -> Result<Angle, Self::Error> {
    match self {
      DimensionPercentage::Dimension(d) => d.try_into().map_err(|_| ()),
      _ => Err(()),
    }
  }
}

impl<D: Zero> Zero for DimensionPercentage<D> {
  fn zero() -> Self {
    DimensionPercentage::Dimension(D::zero())
  }

  fn is_zero(&self) -> bool {
    match self {
      DimensionPercentage::Dimension(d) => d.is_zero(),
      DimensionPercentage::Percentage(p) => p.is_zero(),
      _ => false,
    }
  }
}

impl<D: TrySign> TrySign for DimensionPercentage<D> {
  fn try_sign(&self) -> Option<f32> {
    match self {
      DimensionPercentage::Dimension(d) => d.try_sign(),
      DimensionPercentage::Percentage(p) => p.try_sign(),
      DimensionPercentage::Calc(c) => c.try_sign(),
    }
  }
}

impl<D: ToCss + std::ops::Mul<CSSNumber, Output = D> + TrySign + Clone + std::fmt::Debug> ToCss
  for DimensionPercentage<D>
{
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self {
      DimensionPercentage::Dimension(length) => length.to_css(dest),
      DimensionPercentage::Percentage(percent) => percent.to_css(dest),
      DimensionPercentage::Calc(calc) => calc.to_css(dest),
    }
  }
}
