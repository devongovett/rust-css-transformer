//! CSS time values.

use super::angle::impl_try_from_angle;
use super::calc::Calc;
use super::number::CSSNumber;
use crate::error::{ParserError, PrinterError};
use crate::printer::Printer;
use crate::traits::private::AddInternal;
use crate::traits::{impl_op, Map, Op, Parse, Sign, ToCss, Zero};
#[cfg(feature = "visitor")]
use crate::visitor::Visit;
use cssparser::*;

/// A CSS [`<time>`](https://www.w3.org/TR/css-values-4/#time) value, in either
/// seconds or milliseconds.
///
/// Time values may be explicit or computed by `calc()`, but are always stored and serialized
/// as their computed value.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "visitor", visit(visit_time, TIMES))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum Time {
  /// A time in seconds.
  Seconds(CSSNumber),
  /// A time in milliseconds.
  Milliseconds(CSSNumber),
}

impl Time {
  /// Returns the time in milliseconds.
  pub fn to_ms(&self) -> CSSNumber {
    match self {
      Time::Seconds(s) => s * 1000.0,
      Time::Milliseconds(ms) => *ms,
    }
  }
}

impl Zero for Time {
  fn zero() -> Self {
    Time::Milliseconds(0.0)
  }

  fn is_zero(&self) -> bool {
    match self {
      Time::Seconds(s) => s.is_zero(),
      Time::Milliseconds(s) => s.is_zero(),
    }
  }
}

impl<'i> Parse<'i> for Time {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    match input.try_parse(Calc::parse) {
      Ok(Calc::Value(v)) => return Ok(*v),
      // Time is always compatible, so they will always compute to a value.
      Ok(_) => return Err(input.new_custom_error(ParserError::InvalidValue)),
      _ => {}
    }

    let location = input.current_source_location();
    match *input.next()? {
      Token::Dimension { value, ref unit, .. } => {
        match_ignore_ascii_case! { unit,
          "s" => Ok(Time::Seconds(value)),
          "ms" => Ok(Time::Milliseconds(value)),
          _ => Err(location.new_unexpected_token_error(Token::Ident(unit.clone())))
        }
      }
      ref t => Err(location.new_unexpected_token_error(t.clone())),
    }
  }
}

impl<'i> TryFrom<&Token<'i>> for Time {
  type Error = ();

  fn try_from(token: &Token) -> Result<Self, Self::Error> {
    match token {
      Token::Dimension { value, ref unit, .. } => match_ignore_ascii_case! { unit,
        "s" => Ok(Time::Seconds(*value)),
        "ms" => Ok(Time::Milliseconds(*value)),
        _ => Err(()),
      },
      _ => Err(()),
    }
  }
}

impl ToCss for Time {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    // 0.1s is shorter than 100ms
    // anything smaller is longer
    match self {
      Time::Seconds(s) => {
        if *s > 0.0 && *s < 0.1 {
          (*s * 1000.0).to_css(dest)?;
          dest.write_str("ms")
        } else {
          s.to_css(dest)?;
          dest.write_str("s")
        }
      }
      Time::Milliseconds(ms) => {
        if *ms == 0.0 || *ms >= 100.0 {
          (*ms / 1000.0).to_css(dest)?;
          dest.write_str("s")
        } else {
          ms.to_css(dest)?;
          dest.write_str("ms")
        }
      }
    }
  }
}

impl std::convert::Into<Calc<Time>> for Time {
  fn into(self) -> Calc<Time> {
    Calc::Value(Box::new(self))
  }
}

impl std::convert::TryFrom<Calc<Time>> for Time {
  type Error = ();

  fn try_from(calc: Calc<Time>) -> Result<Time, Self::Error> {
    match calc {
      Calc::Value(v) => Ok(*v),
      _ => Err(()),
    }
  }
}

impl std::ops::Mul<f32> for Time {
  type Output = Self;

  fn mul(self, other: f32) -> Time {
    match self {
      Time::Seconds(t) => Time::Seconds(t * other),
      Time::Milliseconds(t) => Time::Milliseconds(t * other),
    }
  }
}

impl AddInternal for Time {
  fn add(self, other: Self) -> Self {
    self + other
  }
}

impl std::cmp::PartialOrd<Time> for Time {
  fn partial_cmp(&self, other: &Time) -> Option<std::cmp::Ordering> {
    self.to_ms().partial_cmp(&other.to_ms())
  }
}

impl Op for Time {
  fn op<F: FnOnce(f32, f32) -> f32>(&self, to: &Self, op: F) -> Self {
    match (self, to) {
      (Time::Seconds(a), Time::Seconds(b)) => Time::Seconds(op(*a, *b)),
      (Time::Milliseconds(a), Time::Milliseconds(b)) => Time::Milliseconds(op(*a, *b)),
      (Time::Seconds(a), Time::Milliseconds(b)) => Time::Seconds(op(*a, b / 1000.0)),
      (Time::Milliseconds(a), Time::Seconds(b)) => Time::Milliseconds(op(*a, b * 1000.0)),
    }
  }

  fn op_to<T, F: FnOnce(f32, f32) -> T>(&self, rhs: &Self, op: F) -> T {
    match (self, rhs) {
      (Time::Seconds(a), Time::Seconds(b)) => op(*a, *b),
      (Time::Milliseconds(a), Time::Milliseconds(b)) => op(*a, *b),
      (Time::Seconds(a), Time::Milliseconds(b)) => op(*a, b / 1000.0),
      (Time::Milliseconds(a), Time::Seconds(b)) => op(*a, b * 1000.0),
    }
  }
}

impl Map for Time {
  fn map<F: FnOnce(f32) -> f32>(&self, op: F) -> Self {
    match self {
      Time::Seconds(t) => Time::Seconds(op(*t)),
      Time::Milliseconds(t) => Time::Milliseconds(op(*t)),
    }
  }
}

impl Sign for Time {
  fn sign(&self) -> f32 {
    match self {
      Time::Seconds(v) | Time::Milliseconds(v) => v.sign(),
    }
  }
}

impl_op!(Time, std::ops::Rem, rem);
impl_op!(Time, std::ops::Add, add);

impl_try_from_angle!(Time);
