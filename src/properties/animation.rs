//! CSS properties related to keyframe animations.

use std::borrow::Cow;

use crate::context::PropertyHandlerContext;
use crate::declaration::{DeclarationBlock, DeclarationList};
use crate::error::{ParserError, PrinterError};
use crate::macros::*;
use crate::prefixes::Feature;
use crate::printer::Printer;
use crate::properties::{Property, PropertyId, TokenOrValue, VendorPrefix};
use crate::traits::{Parse, PropertyHandler, Shorthand, ToCss, Zero};
use crate::values::ident::DashedIdent;
use crate::values::number::CSSNumber;
use crate::values::percentage::Percentage;
use crate::values::size::Size2D;
use crate::values::string::CSSString;
use crate::values::{easing::EasingFunction, ident::CustomIdent, time::Time};
#[cfg(feature = "visitor")]
use crate::visitor::Visit;
use cssparser::*;
use itertools::izip;
use smallvec::SmallVec;

use super::{LengthPercentage, LengthPercentageOrAuto};

/// A value for the [animation-name](https://drafts.csswg.org/css-animations/#animation-name) property.
#[derive(Debug, Clone, PartialEq, Parse)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum AnimationName<'i> {
  /// The `none` keyword.
  None,
  /// An identifier of a `@keyframes` rule.
  #[cfg_attr(feature = "serde", serde(borrow))]
  Ident(CustomIdent<'i>),
  /// A `<string>` name of a `@keyframes` rule.
  #[cfg_attr(feature = "serde", serde(borrow))]
  String(CSSString<'i>),
}

impl<'i> ToCss for AnimationName<'i> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    let css_module_animation_enabled =
      dest.css_module.as_ref().map_or(false, |css_module| css_module.config.animation);

    match self {
      AnimationName::None => dest.write_str("none"),
      AnimationName::Ident(s) => {
        if css_module_animation_enabled {
          if let Some(css_module) = &mut dest.css_module {
            css_module.reference(&s.0, dest.loc.source_index)
          }
        }
        s.to_css_with_options(dest, css_module_animation_enabled)
      }
      AnimationName::String(s) => {
        if css_module_animation_enabled {
          if let Some(css_module) = &mut dest.css_module {
            css_module.reference(&s, dest.loc.source_index)
          }
        }

        // CSS-wide keywords and `none` cannot remove quotes.
        match_ignore_ascii_case! { &*s,
          "none" | "initial" | "inherit" | "unset" | "default" | "revert" | "revert-layer" => {
            serialize_string(&s, dest)?;
            Ok(())
          },
          _ => {
            dest.write_ident(s.as_ref(), css_module_animation_enabled)
          }
        }
      }
    }
  }
}

/// A list of animation names.
pub type AnimationNameList<'i> = SmallVec<[AnimationName<'i>; 1]>;

/// A value for the [animation-iteration-count](https://drafts.csswg.org/css-animations/#animation-iteration-count) property.
#[derive(Debug, Clone, PartialEq, Parse, ToCss)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub enum AnimationIterationCount {
  /// The animation will repeat the specified number of times.
  Number(CSSNumber),
  /// The animation will repeat forever.
  Infinite,
}

impl Default for AnimationIterationCount {
  fn default() -> Self {
    AnimationIterationCount::Number(1.0)
  }
}

enum_property! {
  /// A value for the [animation-direction](https://drafts.csswg.org/css-animations/#animation-direction) property.
  pub enum AnimationDirection {
    /// The animation is played as specified
    Normal,
    /// The animation is played in reverse.
    Reverse,
    /// The animation iterations alternate between forward and reverse.
    Alternate,
    /// The animation iterations alternate between forward and reverse, with reverse occurring first.
    AlternateReverse,
  }
}

impl Default for AnimationDirection {
  fn default() -> Self {
    AnimationDirection::Normal
  }
}

enum_property! {
  /// A value for the [animation-play-state](https://drafts.csswg.org/css-animations/#animation-play-state) property.
  pub enum AnimationPlayState {
    /// The animation is playing.
    Running,
    /// The animation is paused.
    Paused,
  }
}

impl Default for AnimationPlayState {
  fn default() -> Self {
    AnimationPlayState::Running
  }
}

enum_property! {
  /// A value for the [animation-fill-mode](https://drafts.csswg.org/css-animations/#animation-fill-mode) property.
  pub enum AnimationFillMode {
    /// The animation has no effect while not playing.
    None,
    /// After the animation, the ending values are applied.
    Forwards,
    /// Before the animation, the starting values are applied.
    Backwards,
    /// Both forwards and backwards apply.
    Both,
  }
}

impl Default for AnimationFillMode {
  fn default() -> Self {
    AnimationFillMode::None
  }
}

enum_property! {
  /// A value for the [animation-composition](https://drafts.csswg.org/css-animations-2/#animation-composition) property.
  pub enum AnimationComposition {
    /// The result of compositing the effect value with the underlying value is simply the effect value.
    Replace,
    /// The effect value is added to the underlying value.
    Add,
    /// The effect value is accumulated onto the underlying value.
    Accumulate,
  }
}

/// A value for the [animation-timeline](https://drafts.csswg.org/css-animations-2/#animation-timeline) property.
#[derive(Debug, Clone, PartialEq, Parse, ToCss)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub enum AnimationTimeline<'i> {
  /// The animation’s timeline is a DocumentTimeline, more specifically the default document timeline.
  Auto,
  /// The animation is not associated with a timeline.
  None,
  /// A timeline referenced by name.
  #[cfg_attr(feature = "serde", serde(borrow))]
  DashedIdent(DashedIdent<'i>),
  /// The scroll() function.
  Scroll(ScrollTimeline),
  /// The view() function.
  View(ViewTimeline),
}

impl<'i> Default for AnimationTimeline<'i> {
  fn default() -> Self {
    AnimationTimeline::Auto
  }
}

/// The [scroll()](https://drafts.csswg.org/scroll-animations-1/#scroll-notation) function.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct ScrollTimeline {
  /// Specifies which element to use as the scroll container.
  pub scroller: Scroller,
  /// Specifies which axis of the scroll container to use as the progress for the timeline.
  pub axis: ScrollAxis,
}

impl<'i> Parse<'i> for ScrollTimeline {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    input.expect_function_matching("scroll")?;
    input.parse_nested_block(|input| {
      let mut scroller = None;
      let mut axis = None;
      loop {
        if scroller.is_none() {
          scroller = input.try_parse(Scroller::parse).ok();
        }

        if axis.is_none() {
          axis = input.try_parse(ScrollAxis::parse).ok();
          if axis.is_some() {
            continue;
          }
        }
        break;
      }

      Ok(ScrollTimeline {
        scroller: scroller.unwrap_or_default(),
        axis: axis.unwrap_or_default(),
      })
    })
  }
}

impl ToCss for ScrollTimeline {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    dest.write_str("scroll(")?;

    let mut needs_space = false;
    if self.scroller != Scroller::default() {
      self.scroller.to_css(dest)?;
      needs_space = true;
    }

    if self.axis != ScrollAxis::default() {
      if needs_space {
        dest.write_char(' ')?;
      }
      self.axis.to_css(dest)?;
    }

    dest.write_char(')')
  }
}

enum_property! {
  /// A scroller, used in the `scroll()` function.
  pub enum Scroller {
    /// Specifies to use the document viewport as the scroll container.
    "root": Root,
    /// Specifies to use the nearest ancestor scroll container.
    "nearest": Nearest,
    /// Specifies to use the element’s own principal box as the scroll container.
    "self": SelfElement,
  }
}

impl Default for Scroller {
  fn default() -> Self {
    Scroller::Nearest
  }
}

enum_property! {
  /// A scroll axis, used in the `scroll()` function.
  pub enum ScrollAxis {
    /// Specifies to use the measure of progress along the block axis of the scroll container.
    Block,
    /// Specifies to use the measure of progress along the inline axis of the scroll container.
    Inline,
    /// Specifies to use the measure of progress along the horizontal axis of the scroll container.
    X,
    /// Specifies to use the measure of progress along the vertical axis of the scroll container.
    Y,
  }
}

impl Default for ScrollAxis {
  fn default() -> Self {
    ScrollAxis::Block
  }
}

/// The [view()](https://drafts.csswg.org/scroll-animations-1/#view-notation) function.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct ViewTimeline {
  /// Specifies which axis of the scroll container to use as the progress for the timeline.
  pub axis: ScrollAxis,
  /// Provides an adjustment of the view progress visibility range.
  pub inset: Size2D<LengthPercentageOrAuto>,
}

impl<'i> Parse<'i> for ViewTimeline {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    input.expect_function_matching("view")?;
    input.parse_nested_block(|input| {
      let mut axis = None;
      let mut inset = None;
      loop {
        if axis.is_none() {
          axis = input.try_parse(ScrollAxis::parse).ok();
        }

        if inset.is_none() {
          inset = input.try_parse(Size2D::parse).ok();
          if inset.is_some() {
            continue;
          }
        }
        break;
      }

      Ok(ViewTimeline {
        axis: axis.unwrap_or_default(),
        inset: inset.unwrap_or(Size2D(LengthPercentageOrAuto::Auto, LengthPercentageOrAuto::Auto)),
      })
    })
  }
}

impl ToCss for ViewTimeline {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    dest.write_str("view(")?;
    let mut needs_space = false;
    if self.axis != ScrollAxis::default() {
      self.axis.to_css(dest)?;
      needs_space = true;
    }

    if self.inset.0 != LengthPercentageOrAuto::Auto || self.inset.1 != LengthPercentageOrAuto::Auto {
      if needs_space {
        dest.write_char(' ')?;
      }
      self.inset.to_css(dest)?;
    }

    dest.write_char(')')
  }
}

/// A [view progress timeline range](https://drafts.csswg.org/scroll-animations/#view-timelines-ranges)
#[derive(Debug, Clone, PartialEq, Parse, ToCss)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub enum TimelineRangeName {
  /// Represents the full range of the view progress timeline.
  Cover,
  /// Represents the range during which the principal box is either fully contained by,
  /// or fully covers, its view progress visibility range within the scrollport.
  Contain,
  /// Represents the range during which the principal box is entering the view progress visibility range.
  Entry,
  /// Represents the range during which the principal box is exiting the view progress visibility range.
  Exit,
  /// Represents the range during which the principal box crosses the end border edge.
  EntryCrossing,
  /// Represents the range during which the principal box crosses the start border edge.
  ExitCrossing,
}

/// A value for the [animation-range-start](https://drafts.csswg.org/scroll-animations/#animation-range-start)
/// or [animation-range-end](https://drafts.csswg.org/scroll-animations/#animation-range-end) property.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(rename_all = "lowercase")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub enum AnimationAttachmentRange {
  /// The start of the animation’s attachment range is the start of its associated timeline.
  Normal,
  /// The animation attachment range starts at the specified point on the timeline measuring from the start of the timeline.
  #[cfg_attr(feature = "serde", serde(untagged))]
  LengthPercentage(LengthPercentage),
  /// The animation attachment range starts at the specified point on the timeline measuring from the start of the specified named timeline range.
  #[cfg_attr(feature = "serde", serde(untagged))]
  TimelineRange {
    /// The name of the timeline range.
    name: TimelineRangeName,
    /// The offset from the start of the named timeline range.
    offset: LengthPercentage,
  },
}

impl<'i> AnimationAttachmentRange {
  fn parse<'t>(input: &mut Parser<'i, 't>, default: f32) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    if input.try_parse(|input| input.expect_ident_matching("normal")).is_ok() {
      return Ok(AnimationAttachmentRange::Normal);
    }

    if let Ok(val) = input.try_parse(LengthPercentage::parse) {
      return Ok(AnimationAttachmentRange::LengthPercentage(val));
    }

    let name = TimelineRangeName::parse(input)?;
    let offset = input
      .try_parse(LengthPercentage::parse)
      .unwrap_or(LengthPercentage::Percentage(Percentage(default)));
    Ok(AnimationAttachmentRange::TimelineRange { name, offset })
  }

  fn to_css<W>(&self, dest: &mut Printer<W>, default: f32) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self {
      Self::Normal => dest.write_str("normal"),
      Self::LengthPercentage(val) => val.to_css(dest),
      Self::TimelineRange { name, offset } => {
        name.to_css(dest)?;
        if *offset != LengthPercentage::Percentage(Percentage(default)) {
          dest.write_char(' ')?;
          offset.to_css(dest)?;
        }
        Ok(())
      }
    }
  }
}

impl Default for AnimationAttachmentRange {
  fn default() -> Self {
    AnimationAttachmentRange::Normal
  }
}

/// A value for the [animation-range-start](https://drafts.csswg.org/scroll-animations/#animation-range-start) property.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct AnimationRangeStart(pub AnimationAttachmentRange);

impl<'i> Parse<'i> for AnimationRangeStart {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let range = AnimationAttachmentRange::parse(input, 0.0)?;
    Ok(Self(range))
  }
}

impl ToCss for AnimationRangeStart {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    self.0.to_css(dest, 0.0)
  }
}

/// A value for the [animation-range-end](https://drafts.csswg.org/scroll-animations/#animation-range-end) property.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct AnimationRangeEnd(pub AnimationAttachmentRange);

impl<'i> Parse<'i> for AnimationRangeEnd {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let range = AnimationAttachmentRange::parse(input, 1.0)?;
    Ok(Self(range))
  }
}

impl ToCss for AnimationRangeEnd {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    self.0.to_css(dest, 1.0)
  }
}

/// A value for the [animation-range](https://drafts.csswg.org/scroll-animations/#animation-range) shorthand property.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct AnimationRange {
  /// The start of the animation's attachment range.
  pub start: AnimationRangeStart,
  /// The end of the animation's attachment range.
  pub end: AnimationRangeEnd,
}

impl<'i> Parse<'i> for AnimationRange {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let start = AnimationRangeStart::parse(input)?;
    let end = input
      .try_parse(AnimationRangeStart::parse)
      .map(|r| AnimationRangeEnd(r.0))
      .unwrap_or_else(|_| {
        // If <'animation-range-end'> is omitted and <'animation-range-start'> includes a <timeline-range-name> component, then
        // animation-range-end is set to that same <timeline-range-name> and 100%. Otherwise, any omitted longhand is set to its initial value.
        match &start.0 {
          AnimationAttachmentRange::TimelineRange { name, .. } => {
            AnimationRangeEnd(AnimationAttachmentRange::TimelineRange {
              name: name.clone(),
              offset: LengthPercentage::Percentage(Percentage(1.0)),
            })
          }
          _ => AnimationRangeEnd(AnimationAttachmentRange::default()),
        }
      });
    Ok(AnimationRange { start, end })
  }
}

impl ToCss for AnimationRange {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    self.start.to_css(dest)?;

    let omit_end = match (&self.start.0, &self.end.0) {
      (
        AnimationAttachmentRange::TimelineRange { name: start_name, .. },
        AnimationAttachmentRange::TimelineRange {
          name: end_name,
          offset: end_offset,
        },
      ) => start_name == end_name && *end_offset == LengthPercentage::Percentage(Percentage(1.0)),
      (_, end) => *end == AnimationAttachmentRange::default(),
    };

    if !omit_end {
      dest.write_char(' ')?;
      self.end.to_css(dest)?;
    }
    Ok(())
  }
}

define_list_shorthand! {
  /// A value for the [animation](https://drafts.csswg.org/css-animations/#animation) shorthand property.
  pub struct Animation<'i>(VendorPrefix) {
    /// The animation name.
    #[cfg_attr(feature = "serde", serde(borrow))]
    name: AnimationName(AnimationName<'i>, VendorPrefix),
    /// The animation duration.
    duration: AnimationDuration(Time, VendorPrefix),
    /// The easing function for the animation.
    timing_function: AnimationTimingFunction(EasingFunction, VendorPrefix),
    /// The number of times the animation will run.
    iteration_count: AnimationIterationCount(AnimationIterationCount, VendorPrefix),
    /// The direction of the animation.
    direction: AnimationDirection(AnimationDirection, VendorPrefix),
    /// The current play state of the animation.
    play_state: AnimationPlayState(AnimationPlayState, VendorPrefix),
    /// The animation delay.
    delay: AnimationDelay(Time, VendorPrefix),
    /// The animation fill mode.
    fill_mode: AnimationFillMode(AnimationFillMode, VendorPrefix),
    /// The animation timeline.
    timeline: AnimationTimeline(AnimationTimeline<'i>),
  }
}

impl<'i> Parse<'i> for Animation<'i> {
  fn parse<'t>(input: &mut Parser<'i, 't>) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let mut name = None;
    let mut duration = None;
    let mut timing_function = None;
    let mut iteration_count = None;
    let mut direction = None;
    let mut play_state = None;
    let mut delay = None;
    let mut fill_mode = None;
    let mut timeline = None;

    macro_rules! parse_prop {
      ($var: ident, $type: ident) => {
        if $var.is_none() {
          if let Ok(value) = input.try_parse($type::parse) {
            $var = Some(value);
            continue;
          }
        }
      };
    }

    loop {
      parse_prop!(duration, Time);
      parse_prop!(timing_function, EasingFunction);
      parse_prop!(delay, Time);
      parse_prop!(iteration_count, AnimationIterationCount);
      parse_prop!(direction, AnimationDirection);
      parse_prop!(fill_mode, AnimationFillMode);
      parse_prop!(play_state, AnimationPlayState);
      parse_prop!(name, AnimationName);
      parse_prop!(timeline, AnimationTimeline);
      break;
    }

    Ok(Animation {
      name: name.unwrap_or(AnimationName::None),
      duration: duration.unwrap_or(Time::Seconds(0.0)),
      timing_function: timing_function.unwrap_or(EasingFunction::Ease),
      iteration_count: iteration_count.unwrap_or(AnimationIterationCount::Number(1.0)),
      direction: direction.unwrap_or(AnimationDirection::Normal),
      play_state: play_state.unwrap_or(AnimationPlayState::Running),
      delay: delay.unwrap_or(Time::Seconds(0.0)),
      fill_mode: fill_mode.unwrap_or(AnimationFillMode::None),
      timeline: timeline.unwrap_or(AnimationTimeline::Auto),
    })
  }
}

impl<'i> ToCss for Animation<'i> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    let name = match &self.name {
      AnimationName::None => None,
      AnimationName::Ident(CustomIdent(name)) | AnimationName::String(CSSString(name)) => Some(name),
    };

    let mut has_item = false;

    if !self.duration.is_zero() || !self.delay.is_zero() {
      self.duration.to_css(dest)?;
      has_item = true;
    }

    if !self.timing_function.is_ease() || name.is_some_and(|name| EasingFunction::is_ident(name)) {
      if has_item {
        dest.write_char(' ')?;
      }
      self.timing_function.to_css(dest)?;
      has_item = true;
    }

    if !self.delay.is_zero() {
      if has_item {
        dest.write_char(' ')?;
      }
      self.delay.to_css(dest)?;
      has_item = true;
    }

    if self.iteration_count != AnimationIterationCount::default()
      || name.is_some_and(|name| name.as_ref() == "infinite")
    {
      if has_item {
        dest.write_char(' ')?;
      }
      self.iteration_count.to_css(dest)?;
      has_item = true;
    }

    if self.direction != AnimationDirection::default()
      || name.is_some_and(|name| AnimationDirection::parse_string(name).is_ok())
    {
      if has_item {
        dest.write_char(' ')?;
      }
      self.direction.to_css(dest)?;
      has_item = true;
    }

    if self.fill_mode != AnimationFillMode::default()
      || name
        .is_some_and(|name| !name.eq_ignore_ascii_case("none") && AnimationFillMode::parse_string(name).is_ok())
    {
      if has_item {
        dest.write_char(' ')?;
      }
      self.fill_mode.to_css(dest)?;
      has_item = true;
    }

    if self.play_state != AnimationPlayState::default()
      || name.is_some_and(|name| AnimationPlayState::parse_string(name).is_ok())
    {
      if has_item {
        dest.write_char(' ')?;
      }
      self.play_state.to_css(dest)?;
      has_item = true;
    }

    // Eventually we could output a string here to avoid duplicating some properties above.
    // Chrome does not yet support strings, however.
    if self.name != AnimationName::None {
      if has_item {
        dest.write_char(' ')?;
      }
      self.name.to_css(dest)?;
      has_item = true;

      if self.timeline != AnimationTimeline::default() {
        dest.write_char(' ')?;
        self.timeline.to_css(dest)?;
      }
    }

    if !has_item {
      if dest.minify {
        dest.write_char('1')?;
      } else {
        dest.write_str("none")?;
      }
    }

    Ok(())
  }
}

/// A list of animations.
pub type AnimationList<'i> = SmallVec<[Animation<'i>; 1]>;

#[derive(Default)]
pub(crate) struct AnimationHandler<'i> {
  names: Option<(SmallVec<[AnimationName<'i>; 1]>, VendorPrefix)>,
  durations: Option<(SmallVec<[Time; 1]>, VendorPrefix)>,
  timing_functions: Option<(SmallVec<[EasingFunction; 1]>, VendorPrefix)>,
  iteration_counts: Option<(SmallVec<[AnimationIterationCount; 1]>, VendorPrefix)>,
  directions: Option<(SmallVec<[AnimationDirection; 1]>, VendorPrefix)>,
  play_states: Option<(SmallVec<[AnimationPlayState; 1]>, VendorPrefix)>,
  delays: Option<(SmallVec<[Time; 1]>, VendorPrefix)>,
  fill_modes: Option<(SmallVec<[AnimationFillMode; 1]>, VendorPrefix)>,
  timelines: Option<SmallVec<[AnimationTimeline<'i>; 1]>>,
  range_starts: Option<SmallVec<[AnimationRangeStart; 1]>>,
  range_ends: Option<SmallVec<[AnimationRangeEnd; 1]>>,
  has_any: bool,
}

impl<'i> PropertyHandler<'i> for AnimationHandler<'i> {
  fn handle_property(
    &mut self,
    property: &Property<'i>,
    dest: &mut DeclarationList<'i>,
    context: &mut PropertyHandlerContext<'i, '_>,
  ) -> bool {
    macro_rules! maybe_flush {
      ($prop: ident, $val: expr, $vp: ident) => {{
        // If two vendor prefixes for the same property have different
        // values, we need to flush what we have immediately to preserve order.
        if let Some((val, prefixes)) = &self.$prop {
          if val != $val && !prefixes.contains(*$vp) {
            self.flush(dest, context);
          }
        }
      }};
    }

    macro_rules! property {
      ($prop: ident, $val: expr, $vp: ident) => {{
        maybe_flush!($prop, $val, $vp);

        // Otherwise, update the value and add the prefix.
        if let Some((val, prefixes)) = &mut self.$prop {
          *val = $val.clone();
          *prefixes |= *$vp;
        } else {
          self.$prop = Some(($val.clone(), *$vp));
          self.has_any = true;
        }
      }};
    }

    match property {
      Property::AnimationName(val, vp) => property!(names, val, vp),
      Property::AnimationDuration(val, vp) => property!(durations, val, vp),
      Property::AnimationTimingFunction(val, vp) => property!(timing_functions, val, vp),
      Property::AnimationIterationCount(val, vp) => property!(iteration_counts, val, vp),
      Property::AnimationDirection(val, vp) => property!(directions, val, vp),
      Property::AnimationPlayState(val, vp) => property!(play_states, val, vp),
      Property::AnimationDelay(val, vp) => property!(delays, val, vp),
      Property::AnimationFillMode(val, vp) => property!(fill_modes, val, vp),
      Property::AnimationTimeline(val) => {
        self.timelines = Some(val.clone());
        self.has_any = true;
      }
      Property::AnimationRangeStart(val) => {
        self.range_starts = Some(val.clone());
        self.has_any = true;
      }
      Property::AnimationRangeEnd(val) => {
        self.range_ends = Some(val.clone());
        self.has_any = true;
      }
      Property::AnimationRange(val) => {
        self.range_starts = Some(val.iter().map(|v| v.start.clone()).collect());
        self.range_ends = Some(val.iter().map(|v| v.end.clone()).collect());
        self.has_any = true;
      }
      Property::Animation(val, vp) => {
        let names = val.iter().map(|b| b.name.clone()).collect();
        maybe_flush!(names, &names, vp);

        let durations = val.iter().map(|b| b.duration.clone()).collect();
        maybe_flush!(durations, &durations, vp);

        let timing_functions = val.iter().map(|b| b.timing_function.clone()).collect();
        maybe_flush!(timing_functions, &timing_functions, vp);

        let iteration_counts = val.iter().map(|b| b.iteration_count.clone()).collect();
        maybe_flush!(iteration_counts, &iteration_counts, vp);

        let directions = val.iter().map(|b| b.direction.clone()).collect();
        maybe_flush!(directions, &directions, vp);

        let play_states = val.iter().map(|b| b.play_state.clone()).collect();
        maybe_flush!(play_states, &play_states, vp);

        let delays = val.iter().map(|b| b.delay.clone()).collect();
        maybe_flush!(delays, &delays, vp);

        let fill_modes = val.iter().map(|b| b.fill_mode.clone()).collect();
        maybe_flush!(fill_modes, &fill_modes, vp);

        self.timelines = Some(val.iter().map(|b| b.timeline.clone()).collect());

        property!(names, &names, vp);
        property!(durations, &durations, vp);
        property!(timing_functions, &timing_functions, vp);
        property!(iteration_counts, &iteration_counts, vp);
        property!(directions, &directions, vp);
        property!(play_states, &play_states, vp);
        property!(delays, &delays, vp);
        property!(fill_modes, &fill_modes, vp);

        // The animation shorthand resets animation-range
        // https://drafts.csswg.org/scroll-animations/#named-range-animation-declaration
        self.range_starts = None;
        self.range_ends = None;
      }
      Property::Unparsed(val) if is_animation_property(&val.property_id) => {
        let mut val = Cow::Borrowed(val);
        if matches!(val.property_id, PropertyId::Animation(_)) {
          use crate::properties::custom::Token;

          // Find an identifier that isn't a keyword and replace it with an
          // AnimationName token so it is scoped in CSS modules.
          for token in &mut val.to_mut().value.0 {
            match token {
              TokenOrValue::Token(Token::Ident(id)) => {
                if AnimationDirection::parse_string(&id).is_err()
                  && AnimationPlayState::parse_string(&id).is_err()
                  && AnimationFillMode::parse_string(&id).is_err()
                  && !EasingFunction::is_ident(&id)
                  && id.as_ref() != "infinite"
                  && id.as_ref() != "auto"
                {
                  *token = TokenOrValue::AnimationName(AnimationName::Ident(CustomIdent(id.clone())));
                }
              }
              TokenOrValue::Token(Token::String(s)) => {
                *token = TokenOrValue::AnimationName(AnimationName::String(CSSString(s.clone())));
              }
              _ => {}
            }
          }

          self.range_starts = None;
          self.range_ends = None;
        }

        self.flush(dest, context);
        dest.push(Property::Unparsed(
          val.get_prefixed(context.targets, Feature::Animation),
        ));
      }
      _ => return false,
    }

    true
  }

  fn finalize(&mut self, dest: &mut DeclarationList<'i>, context: &mut PropertyHandlerContext<'i, '_>) {
    self.flush(dest, context);
  }
}

impl<'i> AnimationHandler<'i> {
  fn flush(&mut self, dest: &mut DeclarationList<'i>, context: &mut PropertyHandlerContext<'i, '_>) {
    if !self.has_any {
      return;
    }

    self.has_any = false;

    let mut names = std::mem::take(&mut self.names);
    let mut durations = std::mem::take(&mut self.durations);
    let mut timing_functions = std::mem::take(&mut self.timing_functions);
    let mut iteration_counts = std::mem::take(&mut self.iteration_counts);
    let mut directions = std::mem::take(&mut self.directions);
    let mut play_states = std::mem::take(&mut self.play_states);
    let mut delays = std::mem::take(&mut self.delays);
    let mut fill_modes = std::mem::take(&mut self.fill_modes);
    let mut timelines_value = std::mem::take(&mut self.timelines);
    let range_starts = std::mem::take(&mut self.range_starts);
    let range_ends = std::mem::take(&mut self.range_ends);

    if let (
      Some((names, names_vp)),
      Some((durations, durations_vp)),
      Some((timing_functions, timing_functions_vp)),
      Some((iteration_counts, iteration_counts_vp)),
      Some((directions, directions_vp)),
      Some((play_states, play_states_vp)),
      Some((delays, delays_vp)),
      Some((fill_modes, fill_modes_vp)),
    ) = (
      &mut names,
      &mut durations,
      &mut timing_functions,
      &mut iteration_counts,
      &mut directions,
      &mut play_states,
      &mut delays,
      &mut fill_modes,
    ) {
      // Only use shorthand syntax if the number of animations matches on all properties.
      let len = names.len();
      let intersection = *names_vp
        & *durations_vp
        & *timing_functions_vp
        & *iteration_counts_vp
        & *directions_vp
        & *play_states_vp
        & *delays_vp
        & *fill_modes_vp;
      let mut timelines = if let Some(timelines) = &mut timelines_value {
        Cow::Borrowed(timelines)
      } else if !intersection.contains(VendorPrefix::None) {
        // Prefixed animation shorthand does not support animation-timeline
        Cow::Owned(std::iter::repeat(AnimationTimeline::Auto).take(len).collect())
      } else {
        Cow::Owned(SmallVec::new())
      };

      if !intersection.is_empty()
        && durations.len() == len
        && timing_functions.len() == len
        && iteration_counts.len() == len
        && directions.len() == len
        && play_states.len() == len
        && delays.len() == len
        && fill_modes.len() == len
        && timelines.len() == len
      {
        let timeline_property = if timelines.iter().any(|t| *t != AnimationTimeline::Auto)
          && (intersection != VendorPrefix::None
            || !context
              .targets
              .is_compatible(crate::compat::Feature::AnimationTimelineShorthand))
        {
          Some(Property::AnimationTimeline(timelines.clone().into_owned()))
        } else {
          None
        };

        let animations = izip!(
          names.drain(..),
          durations.drain(..),
          timing_functions.drain(..),
          iteration_counts.drain(..),
          directions.drain(..),
          play_states.drain(..),
          delays.drain(..),
          fill_modes.drain(..),
          timelines.to_mut().drain(..)
        )
        .map(
          |(
            name,
            duration,
            timing_function,
            iteration_count,
            direction,
            play_state,
            delay,
            fill_mode,
            timeline,
          )| {
            Animation {
              name,
              duration,
              timing_function,
              iteration_count,
              direction,
              play_state,
              delay,
              fill_mode,
              timeline: if timeline_property.is_some() {
                AnimationTimeline::Auto
              } else {
                timeline
              },
            }
          },
        )
        .collect();
        let prefix = context.targets.prefixes(intersection, Feature::Animation);
        dest.push(Property::Animation(animations, prefix));
        names_vp.remove(intersection);
        durations_vp.remove(intersection);
        timing_functions_vp.remove(intersection);
        iteration_counts_vp.remove(intersection);
        directions_vp.remove(intersection);
        play_states_vp.remove(intersection);
        delays_vp.remove(intersection);
        fill_modes_vp.remove(intersection);

        if let Some(p) = timeline_property {
          dest.push(p);
        }
        timelines_value = None;
      }
    }

    macro_rules! prop {
      ($var: ident, $property: ident) => {
        if let Some((val, vp)) = $var {
          if !vp.is_empty() {
            let prefix = context.targets.prefixes(vp, Feature::$property);
            dest.push(Property::$property(val, prefix))
          }
        }
      };
    }

    prop!(names, AnimationName);
    prop!(durations, AnimationDuration);
    prop!(timing_functions, AnimationTimingFunction);
    prop!(iteration_counts, AnimationIterationCount);
    prop!(directions, AnimationDirection);
    prop!(play_states, AnimationPlayState);
    prop!(delays, AnimationDelay);
    prop!(fill_modes, AnimationFillMode);

    if let Some(val) = timelines_value {
      dest.push(Property::AnimationTimeline(val));
    }

    match (range_starts, range_ends) {
      (Some(range_starts), Some(range_ends)) => {
        if range_starts.len() == range_ends.len() {
          dest.push(Property::AnimationRange(
            range_starts
              .into_iter()
              .zip(range_ends.into_iter())
              .map(|(start, end)| AnimationRange { start, end })
              .collect(),
          ));
        } else {
          dest.push(Property::AnimationRangeStart(range_starts));
          dest.push(Property::AnimationRangeEnd(range_ends));
        }
      }
      (range_starts, range_ends) => {
        if let Some(range_starts) = range_starts {
          dest.push(Property::AnimationRangeStart(range_starts));
        }

        if let Some(range_ends) = range_ends {
          dest.push(Property::AnimationRangeEnd(range_ends));
        }
      }
    }
  }
}

#[inline]
fn is_animation_property(property_id: &PropertyId) -> bool {
  match property_id {
    PropertyId::AnimationName(_)
    | PropertyId::AnimationDuration(_)
    | PropertyId::AnimationTimingFunction(_)
    | PropertyId::AnimationIterationCount(_)
    | PropertyId::AnimationDirection(_)
    | PropertyId::AnimationPlayState(_)
    | PropertyId::AnimationDelay(_)
    | PropertyId::AnimationFillMode(_)
    | PropertyId::AnimationComposition
    | PropertyId::AnimationTimeline
    | PropertyId::AnimationRange
    | PropertyId::AnimationRangeStart
    | PropertyId::AnimationRangeEnd
    | PropertyId::Animation(_) => true,
    _ => false,
  }
}
