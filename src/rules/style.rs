//! Style rules.

use std::hash::{Hash, Hasher};
use std::ops::Range;

use super::Location;
use super::MinifyContext;
use crate::context::DeclarationContext;
use crate::declaration::DeclarationBlock;
use crate::error::ParserError;
use crate::error::{MinifyError, PrinterError, PrinterErrorKind};
use crate::parser::DefaultAtRule;
use crate::printer::Printer;
use crate::rules::CssRuleList;
use crate::selector::{
  downlevel_selectors, get_prefix, is_compatible, is_pure_css_modules_selector, is_unused, SelectorList,
};
use crate::targets::{should_compile, Targets};
use crate::traits::ToCss;
use crate::vendor_prefix::VendorPrefix;
#[cfg(feature = "visitor")]
use crate::visitor::Visit;
use cssparser::*;

/// A CSS [style rule](https://drafts.csswg.org/css-syntax/#style-rules).
#[derive(Debug, PartialEq, Clone)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct StyleRule<'i, R = DefaultAtRule> {
  /// The selectors for the style rule.
  #[cfg_attr(feature = "serde", serde(borrow))]
  pub selectors: SelectorList<'i>,
  /// A vendor prefix override, used during selector printing.
  #[cfg_attr(feature = "serde", serde(skip, default = "VendorPrefix::empty"))]
  #[cfg_attr(feature = "visitor", skip_visit)]
  pub vendor_prefix: VendorPrefix,
  /// The declarations within the style rule.
  #[cfg_attr(feature = "serde", serde(default))]
  pub declarations: DeclarationBlock<'i>,
  /// Nested rules within the style rule.
  #[cfg_attr(feature = "serde", serde(default = "default_rule_list::<R>"))]
  pub rules: CssRuleList<'i, R>,
  /// The location of the rule in the source file.
  #[cfg_attr(feature = "visitor", skip_visit)]
  pub loc: Location,
}

#[cfg(feature = "serde")]
fn default_rule_list<'i, R>() -> CssRuleList<'i, R> {
  CssRuleList(Vec::new())
}

impl<'i, T: Clone> StyleRule<'i, T> {
  pub(crate) fn minify(
    &mut self,
    context: &mut MinifyContext<'_, 'i>,
    parent_is_unused: bool,
  ) -> Result<bool, MinifyError> {
    self.selectors.minify();

    let mut unused = false;
    if !context.unused_symbols.is_empty() {
      if is_unused(&mut self.selectors.0.iter(), &context.unused_symbols, parent_is_unused) {
        if self.rules.0.is_empty() {
          return Ok(true);
        }

        self.declarations.declarations.clear();
        self.declarations.important_declarations.clear();
        unused = true;
      }
    }

    let pure_css_modules = context.pure_css_modules;
    if context.pure_css_modules {
      if !self.selectors.0.iter().all(is_pure_css_modules_selector) {
        return Err(MinifyError {
          kind: crate::error::MinifyErrorKind::ImpureCSSModuleSelector,
          loc: self.loc,
        });
      }

      // Parent rule contained id or class, so child rules don't need to.
      context.pure_css_modules = false;
    }

    context.handler_context.context = DeclarationContext::StyleRule;
    self
      .declarations
      .minify(context.handler, context.important_handler, &mut context.handler_context);
    context.handler_context.context = DeclarationContext::None;

    if !self.rules.0.is_empty() {
      let mut handler_context = context.handler_context.child(DeclarationContext::StyleRule);
      std::mem::swap(&mut context.handler_context, &mut handler_context);
      self.rules.minify(context, unused)?;
      context.handler_context = handler_context;
      if unused && self.rules.0.is_empty() {
        return Ok(true);
      }
    }

    context.pure_css_modules = pure_css_modules;
    Ok(false)
  }
}

impl<'i, T> StyleRule<'i, T> {
  /// Returns whether the rule is empty.
  pub fn is_empty(&self) -> bool {
    self.selectors.0.is_empty() || (self.declarations.is_empty() && self.rules.0.is_empty())
  }

  /// Returns whether the selectors in the rule are compatible
  /// with all of the given browser targets.
  pub fn is_compatible(&self, targets: Targets) -> bool {
    is_compatible(&self.selectors.0, targets)
  }

  /// Returns the line and column range of the property key and value at the given index in this style rule.
  ///
  /// For performance and memory efficiency in non-error cases, source locations are not stored during parsing.
  /// Instead, they are computed lazily using the original source string that was used to parse the stylesheet/rule.
  pub fn property_location<'t>(
    &self,
    code: &'i str,
    index: usize,
  ) -> Result<(Range<SourceLocation>, Range<SourceLocation>), ParseError<'i, ParserError<'i>>> {
    let mut input = ParserInput::new(code);
    let mut parser = Parser::new(&mut input);

    // advance until start location of this rule.
    parse_at(&mut parser, self.loc, |parser| {
      // skip selector
      parser.parse_until_before(Delimiter::CurlyBracketBlock, |parser| {
        while parser.next().is_ok() {}
        Ok(())
      })?;

      parser.expect_curly_bracket_block()?;
      parser.parse_nested_block(|parser| {
        let loc = self.declarations.property_location(parser, index);
        while parser.next().is_ok() {}
        loc
      })
    })
  }

  /// Returns a hash of this rule for use when deduplicating.
  /// Includes the selectors and properties.
  #[inline]
  pub(crate) fn hash_key(&self) -> u64 {
    let mut hasher = ahash::AHasher::default();
    self.selectors.hash(&mut hasher);
    for (property, _) in self.declarations.iter() {
      property.property_id().hash(&mut hasher);
    }
    hasher.finish()
  }

  /// Returns whether this rule is a duplicate of another rule.
  /// This means it has the same selectors and properties.
  #[inline]
  pub(crate) fn is_duplicate(&self, other_rule: &StyleRule<'i, T>) -> bool {
    self.declarations.len() == other_rule.declarations.len()
      && self.selectors == other_rule.selectors
      && self
        .declarations
        .iter()
        .zip(other_rule.declarations.iter())
        .all(|((a, _), (b, _))| a.property_id() == b.property_id())
  }

  pub(crate) fn update_prefix(&mut self, context: &mut MinifyContext<'_, 'i>) {
    self.vendor_prefix = get_prefix(&self.selectors);
    if self.vendor_prefix.contains(VendorPrefix::None) && context.targets.should_compile_selectors() {
      self.vendor_prefix = downlevel_selectors(self.selectors.0.as_mut_slice(), *context.targets);
    }
  }
}

fn parse_at<'i, 't, T, F>(
  parser: &mut Parser<'i, 't>,
  dest: Location,
  parse: F,
) -> Result<T, ParseError<'i, ParserError<'i>>>
where
  F: Copy + for<'tt> FnOnce(&mut Parser<'i, 'tt>) -> Result<T, ParseError<'i, ParserError<'i>>>,
{
  loop {
    let loc = parser.current_source_location();
    if loc.line >= dest.line || (loc.line == dest.line && loc.column >= dest.column) {
      return parse(parser);
    }

    match parser.next()? {
      Token::CurlyBracketBlock => {
        // Recursively parse nested blocks.
        let res = parser.parse_nested_block(|parser| {
          let res = parse_at(parser, dest, parse);
          while parser.next().is_ok() {}
          res
        });

        if let Ok(v) = res {
          return Ok(v);
        }
      }
      _ => {}
    }
  }
}

impl<'a, 'i, T: ToCss> ToCss for StyleRule<'i, T> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    if self.vendor_prefix.is_empty() {
      self.to_css_base(dest)
    } else {
      let mut first_rule = true;
      for prefix in self.vendor_prefix {
        if first_rule {
          first_rule = false;
        } else {
          if !dest.minify {
            dest.write_char('\n')?; // no indent
          }
          dest.newline()?;
        }
        dest.vendor_prefix = prefix;
        self.to_css_base(dest)?;
      }

      dest.vendor_prefix = VendorPrefix::empty();
      Ok(())
    }
  }
}

impl<'a, 'i, T: ToCss> StyleRule<'i, T> {
  fn to_css_base<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    // If supported, or there are no targets, preserve nesting. Otherwise, write nested rules after parent.
    let supports_nesting = self.rules.0.is_empty() || !should_compile!(dest.targets, Nesting);
    let len = self.declarations.declarations.len() + self.declarations.important_declarations.len();
    let has_declarations = supports_nesting || len > 0 || self.rules.0.is_empty();

    if has_declarations {
      #[cfg(feature = "sourcemap")]
      dest.add_mapping(self.loc);
      self.selectors.to_css(dest)?;
      dest.whitespace()?;
      dest.write_char('{')?;
      dest.indent();

      let mut i = 0;
      macro_rules! write {
        ($decls: ident, $important: literal) => {
          for decl in &self.declarations.$decls {
            // The CSS modules `composes` property is handled specially, and omitted during printing.
            // We need to add the classes it references to the list for the selectors in this rule.
            if let crate::properties::Property::Composes(composes) = &decl {
              if dest.is_nested() && dest.css_module.is_some() {
                return Err(dest.error(PrinterErrorKind::InvalidComposesNesting, composes.loc));
              }

              if let Some(css_module) = &mut dest.css_module {
                css_module
                  .handle_composes(&self.selectors, &composes, self.loc.source_index)
                  .map_err(|e| dest.error(e, composes.loc))?;
                continue;
              }
            }

            dest.newline()?;
            decl.to_css(dest, $important)?;
            if i != len - 1 || !dest.minify || (supports_nesting && !self.rules.0.is_empty()) {
              dest.write_char(';')?;
            }

            i += 1;
          }
        };
      }

      write!(declarations, false);
      write!(important_declarations, true);
    }

    macro_rules! newline {
      () => {
        if !dest.minify && (supports_nesting || len > 0) && !self.rules.0.is_empty() {
          if len > 0 {
            dest.write_char('\n')?;
          }
          dest.newline()?;
        }
      };
    }

    macro_rules! end {
      () => {
        if has_declarations {
          dest.dedent();
          dest.newline()?;
          dest.write_char('}')?;
        }
      };
    }

    // Write nested rules after the parent.
    if supports_nesting {
      newline!();
      self.rules.to_css(dest)?;
      end!();
    } else {
      end!();
      newline!();
      dest.with_context(&self.selectors, |dest| self.rules.to_css(dest))?;
    }

    Ok(())
  }
}
