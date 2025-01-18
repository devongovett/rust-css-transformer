//! The `@media` rule.

use super::Location;
use super::{CssRuleList, MinifyContext};
use crate::error::{MinifyError, PrinterError};
use crate::media_query::MediaList;
use crate::parser::DefaultAtRule;
use crate::printer::Printer;
use crate::traits::ToCss;
#[cfg(feature = "visitor")]
use crate::visitor::Visit;

/// A [@media](https://drafts.csswg.org/css-conditional-3/#at-media) rule.
#[derive(Debug, PartialEq, Clone)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct MediaRule<'i, R = DefaultAtRule> {
  /// The media query list.
  #[cfg_attr(feature = "serde", serde(borrow))]
  pub query: MediaList<'i>,
  /// The rules within the `@media` rule.
  pub rules: CssRuleList<'i, R>,
  /// The location of the rule in the source file.
  #[cfg_attr(feature = "visitor", skip_visit)]
  pub loc: Location,
}

impl<'i, T: Clone> MediaRule<'i, T> {
  pub(crate) fn minify(
    &mut self,
    context: &mut MinifyContext<'_, 'i>,
    parent_is_unused: bool,
  ) -> Result<bool, MinifyError> {
    self.rules.minify(context, parent_is_unused)?;

    if let Some(custom_media) = &context.custom_media {
      self.query.transform_custom_media(self.loc, custom_media)?;
    }

    self.query.transform_resolution(context.targets.current);
    Ok(self.rules.0.is_empty() || self.query.never_matches())
  }
}

impl<'a, 'i, T: ToCss> ToCss for MediaRule<'i, T> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    // If the media query always matches, we can just output the nested rules.
    if dest.minify && self.query.always_matches() {
      self.rules.to_css(dest)?;
      return Ok(());
    }

    #[cfg(feature = "sourcemap")]
    dest.add_mapping(self.loc);
    dest.write_str("@media ")?;
    self.query.to_css(dest)?;
    dest.whitespace()?;
    dest.write_char('{')?;
    dest.indent();
    dest.newline()?;
    self.rules.to_css(dest)?;
    dest.dedent();
    dest.newline()?;
    dest.write_char('}')
  }
}
