//! CSS rules.
//!
//! The [CssRule](CssRule) enum includes all supported rules, and can be used to parse
//! and serialize rules from CSS. Lists of rules (i.e. within a stylesheet, or inside
//! another rule such as `@media`) are represented by [CssRuleList](CssRuleList).
//!
//! Each rule includes a source location, which indicates the line and column within
//! the source file where it was parsed. This is used when generating source maps.
//!
//! # Example
//!
//! This example shows how you could parse a single CSS rule, and serialize it to a string.
//!
//! ```
//! use lightningcss::{
//!   rules::CssRule,
//!   traits::ToCss,
//!   stylesheet::{ParserOptions, PrinterOptions}
//! };
//!
//! let rule = CssRule::parse_string(
//!   ".foo { color: red; }",
//!   ParserOptions::default()
//! ).unwrap();
//!
//! assert_eq!(
//!   rule.to_css_string(PrinterOptions::default()).unwrap(),
//!   ".foo {\n  color: red;\n}"
//! );
//! ```
//!
//! If you have a [cssparser::Parser](cssparser::Parser) already, you can also use the `parse` and `to_css`
//! methods instead, rather than parsing from a string.
//!
//! See [StyleSheet](super::stylesheet::StyleSheet) to parse an entire file of multiple rules.

#![deny(missing_docs)]

pub mod container;
pub mod counter_style;
pub mod custom_media;
pub mod document;
pub mod font_face;
pub mod font_feature_values;
pub mod font_palette_values;
pub mod import;
pub mod keyframes;
pub mod layer;
pub mod media;
pub mod namespace;
pub mod nesting;
pub mod page;
pub mod property;
pub mod scope;
pub mod starting_style;
pub mod style;
pub mod supports;
pub mod unknown;
pub mod view_transition;
pub mod viewport;

use self::font_feature_values::FontFeatureValuesRule;
use self::font_palette_values::FontPaletteValuesRule;
use self::layer::{LayerBlockRule, LayerStatementRule};
use self::property::PropertyRule;
use crate::context::PropertyHandlerContext;
use crate::declaration::{DeclarationBlock, DeclarationHandler};
use crate::dependencies::{Dependency, ImportDependency};
use crate::error::{MinifyError, ParserError, PrinterError, PrinterErrorKind};
use crate::parser::{parse_rule_list, parse_style_block, DefaultAtRule, DefaultAtRuleParser, TopLevelRuleParser};
use crate::prefixes::Feature;
use crate::printer::Printer;
use crate::rules::keyframes::KeyframesName;
use crate::selector::{is_compatible, is_equivalent, Component, Selector, SelectorList};
use crate::stylesheet::ParserOptions;
use crate::targets::TargetsWithSupportsScope;
use crate::traits::{AtRuleParser, ToCss};
use crate::values::string::CowArcStr;
use crate::vendor_prefix::VendorPrefix;
#[cfg(feature = "visitor")]
use crate::visitor::{Visit, VisitTypes, Visitor};
use container::ContainerRule;
use counter_style::CounterStyleRule;
use cssparser::{parse_one_rule, ParseError, Parser, ParserInput};
use custom_media::CustomMediaRule;
use document::MozDocumentRule;
use font_face::FontFaceRule;
use import::ImportRule;
use itertools::Itertools;
use keyframes::KeyframesRule;
use media::MediaRule;
use namespace::NamespaceRule;
use nesting::NestingRule;
use page::PageRule;
use scope::ScopeRule;
use smallvec::{smallvec, SmallVec};
use starting_style::StartingStyleRule;
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};
use style::StyleRule;
use supports::SupportsRule;
use unknown::UnknownAtRule;
use view_transition::ViewTransitionRule;
use viewport::ViewportRule;

#[derive(Clone)]
pub(crate) struct StyleContext<'a, 'i> {
  pub selectors: &'a SelectorList<'i>,
  pub parent: Option<&'a StyleContext<'a, 'i>>,
}

/// A source location.
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
#[cfg_attr(any(feature = "serde", feature = "nodejs"), derive(serde::Serialize))]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct Location {
  /// The index of the source file within the source map.
  pub source_index: u32,
  /// The line number, starting at 0.
  pub line: u32,
  /// The column number within a line, starting at 1 for first the character of the line.
  /// Column numbers are counted in UTF-16 code units.
  pub column: u32,
}

/// A CSS rule.
#[derive(Debug, PartialEq, Clone)]
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "visitor", visit(visit_rule, RULES))]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize),
  serde(tag = "type", content = "value", rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema), schemars(rename = "Rule"))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub enum CssRule<'i, R = DefaultAtRule> {
  /// A `@media` rule.
  #[cfg_attr(feature = "serde", serde(borrow))]
  Media(MediaRule<'i, R>),
  /// An `@import` rule.
  Import(ImportRule<'i>),
  /// A style rule.
  Style(StyleRule<'i, R>),
  /// A `@keyframes` rule.
  Keyframes(KeyframesRule<'i>),
  /// A `@font-face` rule.
  FontFace(FontFaceRule<'i>),
  /// A `@font-palette-values` rule.
  FontPaletteValues(FontPaletteValuesRule<'i>),
  /// A `@font-feature-values` rule.
  FontFeatureValues(FontFeatureValuesRule<'i>),
  /// A `@page` rule.
  Page(PageRule<'i>),
  /// A `@supports` rule.
  Supports(SupportsRule<'i, R>),
  /// A `@counter-style` rule.
  CounterStyle(CounterStyleRule<'i>),
  /// A `@namespace` rule.
  Namespace(NamespaceRule<'i>),
  /// A `@-moz-document` rule.
  MozDocument(MozDocumentRule<'i, R>),
  /// A `@nest` rule.
  Nesting(NestingRule<'i, R>),
  /// A `@viewport` rule.
  Viewport(ViewportRule<'i>),
  /// A `@custom-media` rule.
  CustomMedia(CustomMediaRule<'i>),
  /// A `@layer` statement rule.
  LayerStatement(LayerStatementRule<'i>),
  /// A `@layer` block rule.
  LayerBlock(LayerBlockRule<'i, R>),
  /// A `@property` rule.
  Property(PropertyRule<'i>),
  /// A `@container` rule.
  Container(ContainerRule<'i, R>),
  /// A `@scope` rule.
  Scope(ScopeRule<'i, R>),
  /// A `@starting-style` rule.
  StartingStyle(StartingStyleRule<'i, R>),
  /// A `@view-transition` rule.
  ViewTransition(ViewTransitionRule<'i>),
  /// A placeholder for a rule that was removed.
  Ignored,
  /// An unknown at-rule.
  Unknown(UnknownAtRule<'i>),
  /// A custom at-rule.
  Custom(R),
}

// Manually implemented deserialize to reduce binary size.
#[cfg(feature = "serde")]
#[cfg_attr(docsrs, doc(cfg(feature = "serde")))]
impl<'i, 'de: 'i, R: serde::Deserialize<'de>> serde::Deserialize<'de> for CssRule<'i, R> {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    #[derive(serde::Deserialize)]
    #[serde(field_identifier, rename_all = "snake_case")]
    enum Field {
      Type,
      Value,
    }

    struct PartialRule<'de> {
      rule_type: CowArcStr<'de>,
      content: serde::__private::de::Content<'de>,
    }

    struct CssRuleVisitor;

    impl<'de> serde::de::Visitor<'de> for CssRuleVisitor {
      type Value = PartialRule<'de>;

      fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a CssRule")
      }

      fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
      where
        A: serde::de::MapAccess<'de>,
      {
        let mut rule_type: Option<CowArcStr<'de>> = None;
        let mut value: Option<serde::__private::de::Content> = None;
        while let Some(key) = map.next_key()? {
          match key {
            Field::Type => {
              rule_type = Some(map.next_value()?);
            }
            Field::Value => {
              value = Some(map.next_value()?);
            }
          }
        }

        let rule_type = rule_type.ok_or_else(|| serde::de::Error::missing_field("type"))?;
        let content = value.ok_or_else(|| serde::de::Error::missing_field("value"))?;
        Ok(PartialRule { rule_type, content })
      }
    }

    let partial = deserializer.deserialize_map(CssRuleVisitor)?;
    let deserializer = serde::__private::de::ContentDeserializer::new(partial.content);

    match partial.rule_type.as_ref() {
      "media" => {
        let rule = MediaRule::deserialize(deserializer)?;
        Ok(CssRule::Media(rule))
      }
      "import" => {
        let rule = ImportRule::deserialize(deserializer)?;
        Ok(CssRule::Import(rule))
      }
      "style" => {
        let rule = StyleRule::deserialize(deserializer)?;
        Ok(CssRule::Style(rule))
      }
      "keyframes" => {
        let rule = KeyframesRule::deserialize(deserializer)?;
        Ok(CssRule::Keyframes(rule))
      }
      "font-face" => {
        let rule = FontFaceRule::deserialize(deserializer)?;
        Ok(CssRule::FontFace(rule))
      }
      "font-palette-values" => {
        let rule = FontPaletteValuesRule::deserialize(deserializer)?;
        Ok(CssRule::FontPaletteValues(rule))
      }
      "font-feature-values" => {
        let rule = FontFeatureValuesRule::deserialize(deserializer)?;
        Ok(CssRule::FontFeatureValues(rule))
      }
      "page" => {
        let rule = PageRule::deserialize(deserializer)?;
        Ok(CssRule::Page(rule))
      }
      "supports" => {
        let rule = SupportsRule::deserialize(deserializer)?;
        Ok(CssRule::Supports(rule))
      }
      "counter-style" => {
        let rule = CounterStyleRule::deserialize(deserializer)?;
        Ok(CssRule::CounterStyle(rule))
      }
      "namespace" => {
        let rule = NamespaceRule::deserialize(deserializer)?;
        Ok(CssRule::Namespace(rule))
      }
      "moz-document" => {
        let rule = MozDocumentRule::deserialize(deserializer)?;
        Ok(CssRule::MozDocument(rule))
      }
      "nesting" => {
        let rule = NestingRule::deserialize(deserializer)?;
        Ok(CssRule::Nesting(rule))
      }
      "viewport" => {
        let rule = ViewportRule::deserialize(deserializer)?;
        Ok(CssRule::Viewport(rule))
      }
      "custom-media" => {
        let rule = CustomMediaRule::deserialize(deserializer)?;
        Ok(CssRule::CustomMedia(rule))
      }
      "layer-statement" => {
        let rule = LayerStatementRule::deserialize(deserializer)?;
        Ok(CssRule::LayerStatement(rule))
      }
      "layer-block" => {
        let rule = LayerBlockRule::deserialize(deserializer)?;
        Ok(CssRule::LayerBlock(rule))
      }
      "property" => {
        let rule = PropertyRule::deserialize(deserializer)?;
        Ok(CssRule::Property(rule))
      }
      "container" => {
        let rule = ContainerRule::deserialize(deserializer)?;
        Ok(CssRule::Container(rule))
      }
      "scope" => {
        let rule = ScopeRule::deserialize(deserializer)?;
        Ok(CssRule::Scope(rule))
      }
      "starting-style" => {
        let rule = StartingStyleRule::deserialize(deserializer)?;
        Ok(CssRule::StartingStyle(rule))
      }
      "view-transition" => {
        let rule = ViewTransitionRule::deserialize(deserializer)?;
        Ok(CssRule::ViewTransition(rule))
      }
      "ignored" => Ok(CssRule::Ignored),
      "unknown" => {
        let rule = UnknownAtRule::deserialize(deserializer)?;
        Ok(CssRule::Unknown(rule))
      }
      "custom" => {
        let rule = R::deserialize(deserializer)?;
        Ok(CssRule::Custom(rule))
      }
      t => Err(serde::de::Error::unknown_variant(t, &[])),
    }
  }
}

impl<'a, 'i, T: ToCss> ToCss for CssRule<'i, T> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    match self {
      CssRule::Media(media) => media.to_css(dest),
      CssRule::Import(import) => import.to_css(dest),
      CssRule::Style(style) => style.to_css(dest),
      CssRule::Keyframes(keyframes) => keyframes.to_css(dest),
      CssRule::FontFace(font_face) => font_face.to_css(dest),
      CssRule::FontPaletteValues(f) => f.to_css(dest),
      CssRule::FontFeatureValues(font_feature_values) => font_feature_values.to_css(dest),
      CssRule::Page(font_face) => font_face.to_css(dest),
      CssRule::Supports(supports) => supports.to_css(dest),
      CssRule::CounterStyle(counter_style) => counter_style.to_css(dest),
      CssRule::Namespace(namespace) => namespace.to_css(dest),
      CssRule::MozDocument(document) => document.to_css(dest),
      CssRule::Nesting(nesting) => nesting.to_css(dest),
      CssRule::Viewport(viewport) => viewport.to_css(dest),
      CssRule::CustomMedia(custom_media) => custom_media.to_css(dest),
      CssRule::LayerStatement(layer) => layer.to_css(dest),
      CssRule::LayerBlock(layer) => layer.to_css(dest),
      CssRule::Property(property) => property.to_css(dest),
      CssRule::StartingStyle(rule) => rule.to_css(dest),
      CssRule::Container(container) => container.to_css(dest),
      CssRule::Scope(scope) => scope.to_css(dest),
      CssRule::ViewTransition(rule) => rule.to_css(dest),
      CssRule::Unknown(unknown) => unknown.to_css(dest),
      CssRule::Custom(rule) => rule.to_css(dest).map_err(|_| PrinterError {
        kind: PrinterErrorKind::FmtError,
        loc: None,
      }),
      CssRule::Ignored => Ok(()),
    }
  }
}

impl<'i> CssRule<'i, DefaultAtRule> {
  /// Parse a single rule.
  pub fn parse<'t>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    Self::parse_with(input, options, &mut DefaultAtRuleParser)
  }

  /// Parse a single rule from a string.
  pub fn parse_string(
    input: &'i str,
    options: ParserOptions<'_, 'i>,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    Self::parse_string_with(input, options, &mut DefaultAtRuleParser)
  }
}

impl<'i, T> CssRule<'i, T> {
  /// Parse a single rule.
  pub fn parse_with<'t, P: AtRuleParser<'i, AtRule = T>>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
    at_rule_parser: &mut P,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let mut rules = CssRuleList(Vec::new());
    parse_one_rule(input, &mut TopLevelRuleParser::new(options, at_rule_parser, &mut rules))?;
    Ok(rules.0.pop().unwrap())
  }

  /// Parse a single rule from a string.
  pub fn parse_string_with<P: AtRuleParser<'i, AtRule = T>>(
    input: &'i str,
    options: ParserOptions<'_, 'i>,
    at_rule_parser: &mut P,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    let mut input = ParserInput::new(input);
    let mut parser = Parser::new(&mut input);
    Self::parse_with(&mut parser, &options, at_rule_parser)
  }
}

/// A list of CSS rules.
#[derive(Debug, PartialEq, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct CssRuleList<'i, R = DefaultAtRule>(
  #[cfg_attr(feature = "serde", serde(borrow))] pub Vec<CssRule<'i, R>>,
);

impl<'i> CssRuleList<'i, DefaultAtRule> {
  /// Parse a rule list.
  pub fn parse<'t>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    Self::parse_with(input, options, &mut DefaultAtRuleParser)
  }

  /// Parse a style block, with both declarations and rules.
  /// Resulting declarations are returned in a nested style rule.
  pub fn parse_style_block<'t>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
    is_nested: bool,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    Self::parse_style_block_with(input, options, &mut DefaultAtRuleParser, is_nested)
  }
}

impl<'i, T> CssRuleList<'i, T> {
  /// Parse a rule list with a custom at rule parser.
  pub fn parse_with<'t, P: AtRuleParser<'i, AtRule = T>>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
    at_rule_parser: &mut P,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    parse_rule_list(input, options, at_rule_parser)
  }

  /// Parse a style block, with both declarations and rules.
  /// Resulting declarations are returned in a nested style rule.
  pub fn parse_style_block_with<'t, P: AtRuleParser<'i, AtRule = T>>(
    input: &mut Parser<'i, 't>,
    options: &ParserOptions<'_, 'i>,
    at_rule_parser: &mut P,
    is_nested: bool,
  ) -> Result<Self, ParseError<'i, ParserError<'i>>> {
    parse_style_block(input, options, at_rule_parser, is_nested)
  }
}

// Manually implemented to avoid circular child types.
#[cfg(feature = "visitor")]
#[cfg_attr(docsrs, doc(cfg(feature = "visitor")))]
impl<'i, T: Visit<'i, T, V>, V: ?Sized + Visitor<'i, T>> Visit<'i, T, V> for CssRuleList<'i, T> {
  const CHILD_TYPES: VisitTypes = VisitTypes::all();

  fn visit(&mut self, visitor: &mut V) -> Result<(), V::Error> {
    if visitor.visit_types().contains(VisitTypes::RULES) {
      visitor.visit_rule_list(self)
    } else {
      self.0.visit(visitor)
    }
  }

  fn visit_children(&mut self, visitor: &mut V) -> Result<(), V::Error> {
    self.0.visit(visitor)
  }
}

pub(crate) struct MinifyContext<'a, 'i> {
  pub targets: TargetsWithSupportsScope,
  pub handler: &'a mut DeclarationHandler<'i>,
  pub important_handler: &'a mut DeclarationHandler<'i>,
  pub handler_context: PropertyHandlerContext<'i, 'a>,
  pub unused_symbols: &'a HashSet<String>,
  pub custom_media: Option<HashMap<CowArcStr<'i>, CustomMediaRule<'i>>>,
  pub css_modules: bool,
  pub pure_css_modules: bool,
}

impl<'i, T: Clone> CssRuleList<'i, T> {
  pub(crate) fn minify(
    &mut self,
    context: &mut MinifyContext<'_, 'i>,
    parent_is_unused: bool,
  ) -> Result<(), MinifyError> {
    let mut keyframe_rules = HashMap::new();
    let mut layer_rules = HashMap::new();
    let mut property_rules = HashMap::new();
    let mut font_feature_values_rules = Vec::new();
    let mut style_rules =
      HashMap::with_capacity_and_hasher(self.0.len(), BuildHasherDefault::<PrecomputedHasher>::default());
    let mut rules = Vec::new();
    for mut rule in self.0.drain(..) {
      match &mut rule {
        CssRule::Keyframes(keyframes) => {
          if context.unused_symbols.contains(match &keyframes.name {
            KeyframesName::Ident(ident) => ident.0.as_ref(),
            KeyframesName::Custom(string) => string.as_ref(),
          }) {
            continue;
          }
          keyframes.minify(context);

          macro_rules! set_prefix {
            ($keyframes: ident) => {
              $keyframes.vendor_prefix =
                context.targets.current.prefixes($keyframes.vendor_prefix, Feature::AtKeyframes);
            };
          }

          // Merge @keyframes rules with the same name.
          if let Some(existing_idx) = keyframe_rules.get(&keyframes.name) {
            if let Some(CssRule::Keyframes(existing)) = &mut rules.get_mut(*existing_idx) {
              // If the existing rule has the same vendor prefixes, replace it with this rule.
              if existing.vendor_prefix == keyframes.vendor_prefix {
                *existing = keyframes.clone();
                continue;
              }
              // Otherwise, if the keyframes are identical, merge the prefixes.
              if existing.keyframes == keyframes.keyframes {
                existing.vendor_prefix |= keyframes.vendor_prefix;
                set_prefix!(existing);
                continue;
              }
            }
          }

          set_prefix!(keyframes);
          keyframe_rules.insert(keyframes.name.clone(), rules.len());

          let fallbacks = keyframes.get_fallbacks(&context.targets.current);
          rules.push(rule);
          rules.extend(fallbacks);
          continue;
        }
        CssRule::CustomMedia(_) => {
          if context.custom_media.is_some() {
            continue;
          }
        }
        CssRule::Media(media) => {
          if let Some(CssRule::Media(last_rule)) = rules.last_mut() {
            if last_rule.query == media.query {
              last_rule.rules.0.extend(media.rules.0.drain(..));
              last_rule.minify(context, parent_is_unused)?;
              continue;
            }
          }

          if media.minify(context, parent_is_unused)? {
            continue;
          }
        }
        CssRule::Supports(supports) => {
          if let Some(CssRule::Supports(last_rule)) = rules.last_mut() {
            if last_rule.condition == supports.condition {
              last_rule.rules.0.extend(supports.rules.0.drain(..));
              last_rule.minify(context, parent_is_unused)?;
              continue;
            }
          }

          supports.minify(context, parent_is_unused)?;
          if supports.rules.0.is_empty() {
            continue;
          }
        }
        CssRule::Container(container) => {
          if let Some(CssRule::Container(last_rule)) = rules.last_mut() {
            if last_rule.name == container.name && last_rule.condition == container.condition {
              last_rule.rules.0.extend(container.rules.0.drain(..));
              last_rule.minify(context, parent_is_unused)?;
              continue;
            }
          }

          if container.minify(context, parent_is_unused)? {
            continue;
          }
        }
        CssRule::LayerBlock(layer) => {
          // Merging non-adjacent layer rules is safe because they are applied
          // in the order they are first defined.
          if let Some(name) = &layer.name {
            if let Some(idx) = layer_rules.get(name) {
              if let Some(CssRule::LayerBlock(last_rule)) = rules.get_mut(*idx) {
                last_rule.rules.0.extend(layer.rules.0.drain(..));
                continue;
              }
            }

            layer_rules.insert(name.clone(), rules.len());
          }
        }
        CssRule::LayerStatement(layer) => {
          // Create @layer block rules for each declared layer name,
          // so we can merge other blocks into it later on.
          for name in &layer.names {
            if !layer_rules.contains_key(name) {
              layer_rules.insert(name.clone(), rules.len());
              rules.push(CssRule::LayerBlock(LayerBlockRule {
                name: Some(name.clone()),
                rules: CssRuleList(vec![]),
                loc: layer.loc.clone(),
              }));
            }
          }
          continue;
        }
        CssRule::MozDocument(document) => document.minify(context)?,
        CssRule::Style(style) => {
          if parent_is_unused || style.minify(context, parent_is_unused)? {
            continue;
          }

          // If some of the selectors in this rule are not compatible with the targets,
          // we need to either wrap in :is() or split them into multiple rules.
          let incompatible = if style.selectors.0.len() > 1
            && context.targets.current.should_compile_selectors()
            && !style.is_compatible(context.targets.current)
          {
            // The :is() selector accepts a forgiving selector list, so use that if possible.
            // Note that :is() does not allow pseudo elements, so we need to check for that.
            // In addition, :is() takes the highest specificity of its arguments, so if the selectors
            // have different weights, we need to split them into separate rules as well.
            if context.targets.current.is_compatible(crate::compat::Feature::IsSelector)
              && !style.selectors.0.iter().any(|selector| selector.has_pseudo_element())
              && style.selectors.0.iter().map(|selector| selector.specificity()).all_equal()
            {
              style.selectors =
                SelectorList::new(smallvec![
                  Component::Is(style.selectors.0.clone().into_boxed_slice()).into()
                ]);
              smallvec![]
            } else {
              // Otherwise, partition the selectors and keep the compatible ones in this rule.
              // We will generate additional rules for incompatible selectors later.
              let (compatible, incompatible) = style
                .selectors
                .0
                .iter()
                .cloned()
                .partition::<SmallVec<[Selector; 1]>, _>(|selector| {
                  let list = SelectorList::new(smallvec![selector.clone()]);
                  is_compatible(&list.0, context.targets.current)
                });
              style.selectors = SelectorList::new(compatible);
              incompatible
            }
          } else {
            smallvec![]
          };

          style.update_prefix(context);

          // Attempt to merge the new rule with the last rule we added.
          let mut merged = false;
          if let Some(CssRule::Style(last_style_rule)) = rules.last_mut() {
            if merge_style_rules(style, last_style_rule, context) {
              // If that was successful, then the last rule has been updated to include the
              // selectors/declarations of the new rule. This might mean that we can merge it
              // with the previous rule, so continue trying while we have style rules available.
              while rules.len() >= 2 {
                let len = rules.len();
                let (a, b) = rules.split_at_mut(len - 1);
                if let (CssRule::Style(last), CssRule::Style(prev)) = (&mut b[0], &mut a[len - 2]) {
                  if merge_style_rules(last, prev, context) {
                    // If we were able to merge the last rule into the previous one, remove the last.
                    rules.pop();
                    continue;
                  }
                }
                // If we didn't see a style rule, or were unable to merge, stop.
                break;
              }
              merged = true;
            }
          }

          // Create additional rules for logical properties, @supports overrides, and incompatible selectors.
          let supports = context.handler_context.get_supports_rules(&style);
          let logical = context.handler_context.get_additional_rules(&style);

          let incompatible_rules = incompatible
            .into_iter()
            .map(|selector| {
              // Create a clone of the rule with only the one incompatible selector.
              let list = SelectorList::new(smallvec![selector]);
              let mut clone = style.clone();
              clone.selectors = list;
              clone.update_prefix(context);

              // Also add rules for logical properties and @supports overrides.
              let supports = context.handler_context.get_supports_rules(&clone);
              let logical = context.handler_context.get_additional_rules(&clone);
              (clone, logical, supports)
            })
            .collect::<Vec<_>>();

          context.handler_context.reset();

          // If the rule has nested rules, and we have extra rules to insert such as for logical properties,
          // we need to split the rule in two so we can insert the extra rules in between the declarations from
          // the main rule and the nested rules.
          let nested_rule = if !style.rules.0.is_empty()
            // can happen if there are no compatible rules, above.
            && !style.selectors.0.is_empty()
            && (!logical.is_empty() || !supports.is_empty() || !incompatible_rules.is_empty())
          {
            let mut rules = CssRuleList(vec![]);
            std::mem::swap(&mut style.rules, &mut rules);
            Some(StyleRule {
              selectors: style.selectors.clone(),
              declarations: DeclarationBlock::default(),
              rules,
              vendor_prefix: style.vendor_prefix,
              loc: style.loc,
            })
          } else {
            None
          };

          if !merged && !style.is_empty() {
            let source_index = style.loc.source_index;
            let has_no_rules = style.rules.0.is_empty();
            let idx = rules.len();
            rules.push(rule);

            // Check if this rule is a duplicate of an earlier rule, meaning it has
            // the same selectors and defines the same properties. If so, remove the
            // earlier rule because this one completely overrides it.
            if has_no_rules {
              // SAFETY: StyleRuleKeys never live beyond this method.
              let key = StyleRuleKey::new(unsafe { &*(&rules as *const _) }, idx);
              if idx > 0 {
                if let Some(i) = style_rules.remove(&key) {
                  if let CssRule::Style(other) = &rules[i] {
                    // Don't remove the rule if this is a CSS module and the other rule came from a different file.
                    if !context.css_modules || source_index == other.loc.source_index {
                      // Only mark the rule as ignored so we don't need to change all of the indices.
                      rules[i] = CssRule::Ignored;
                    }
                  }
                }
              }

              style_rules.insert(key, idx);
            }
          }

          if !logical.is_empty() {
            let mut logical = CssRuleList(logical);
            logical.minify(context, parent_is_unused)?;
            rules.extend(logical.0)
          }

          rules.extend(supports);
          for (rule, logical, supports) in incompatible_rules {
            if !rule.is_empty() {
              rules.push(CssRule::Style(rule));
            }
            if !logical.is_empty() {
              let mut logical = CssRuleList(logical);
              logical.minify(context, parent_is_unused)?;
              rules.extend(logical.0)
            }
            rules.extend(supports);
          }

          if let Some(nested_rule) = nested_rule {
            rules.push(CssRule::Style(nested_rule));
          }

          continue;
        }
        CssRule::CounterStyle(counter_style) => {
          if context.unused_symbols.contains(counter_style.name.0.as_ref()) {
            continue;
          }
        }
        CssRule::Scope(scope) => scope.minify(context)?,
        CssRule::Nesting(nesting) => {
          if nesting.minify(context, parent_is_unused)? {
            continue;
          }
        }
        CssRule::StartingStyle(rule) => {
          if rule.minify(context, parent_is_unused)? {
            continue;
          }
        }
        CssRule::FontPaletteValues(f) => {
          if context.unused_symbols.contains(f.name.0.as_ref()) {
            continue;
          }

          f.minify(context, parent_is_unused);

          let fallbacks = f.get_fallbacks(context.targets.current);
          rules.push(rule);
          rules.extend(fallbacks);
          continue;
        }
        CssRule::FontFeatureValues(rule) => {
          if let Some(index) = font_feature_values_rules
            .iter()
            .find(|index| matches!(&rules[**index], CssRule::FontFeatureValues(r) if r.name == rule.name))
          {
            if let CssRule::FontFeatureValues(existing) = &mut rules[*index] {
              existing.merge(rule);
            }
            continue;
          } else {
            font_feature_values_rules.push(rules.len());
          }
        }
        CssRule::Property(property) => {
          if context.unused_symbols.contains(property.name.0.as_ref()) {
            continue;
          }

          if let Some(index) = property_rules.get(&property.name) {
            rules[*index] = rule;
            continue;
          } else {
            property_rules.insert(property.name.clone(), rules.len());
          }
        }
        _ => {}
      }

      rules.push(rule)
    }

    // Optimize @layer rules. Combine subsequent empty layer blocks into a single @layer statement
    // so that layers are declared in the correct order.
    if !layer_rules.is_empty() {
      let mut declared_layers = HashSet::new();
      let mut layer_statement = None;
      for index in 0..rules.len() {
        match &mut rules[index] {
          CssRule::LayerBlock(layer) => {
            if layer.minify(context, parent_is_unused)? {
              if let Some(name) = &layer.name {
                if declared_layers.contains(name) {
                  // Remove empty layer that has already been declared.
                  rules[index] = CssRule::Ignored;
                  continue;
                }

                let name = name.clone();
                declared_layers.insert(name.clone());

                if let Some(layer_index) = layer_statement {
                  if let CssRule::LayerStatement(layer) = &mut rules[layer_index] {
                    // Add name to previous layer statement rule and remove this one.
                    layer.names.push(name);
                    rules[index] = CssRule::Ignored;
                  }
                } else {
                  // Create a new layer statement rule to declare the name.
                  rules[index] = CssRule::LayerStatement(LayerStatementRule {
                    names: vec![name],
                    loc: layer.loc,
                  });
                  layer_statement = Some(index);
                }
              } else {
                // Remove empty anonymous layer.
                rules[index] = CssRule::Ignored;
              }
            } else {
              // Non-empty @layer block. Start a new statement.
              layer_statement = None;
            }
          }
          CssRule::Import(import) => {
            if let Some(layer) = &import.layer {
              // Start a new @layer statement so the import layer is in the right order.
              layer_statement = None;
              if let Some(name) = layer {
                declared_layers.insert(name.clone());
              }
            }
          }
          _ => {}
        }
      }
    }

    self.0 = rules;
    Ok(())
  }
}

fn merge_style_rules<'i, T>(
  style: &mut StyleRule<'i, T>,
  last_style_rule: &mut StyleRule<'i, T>,
  context: &mut MinifyContext<'_, 'i>,
) -> bool {
  // Merge declarations if the selectors are equivalent, and both are compatible with all targets.
  if style.selectors == last_style_rule.selectors
    && style.is_compatible(context.targets.current)
    && last_style_rule.is_compatible(context.targets.current)
    && style.rules.0.is_empty()
    && last_style_rule.rules.0.is_empty()
    && (!context.css_modules || style.loc.source_index == last_style_rule.loc.source_index)
  {
    last_style_rule
      .declarations
      .declarations
      .extend(style.declarations.declarations.drain(..));
    last_style_rule
      .declarations
      .important_declarations
      .extend(style.declarations.important_declarations.drain(..));
    last_style_rule
      .declarations
      .minify(context.handler, context.important_handler, &mut context.handler_context);
    return true;
  } else if style.declarations == last_style_rule.declarations
    && style.rules.0.is_empty()
    && last_style_rule.rules.0.is_empty()
  {
    // If both selectors are potentially vendor prefixable, and they are
    // equivalent minus prefixes, add the prefix to the last rule.
    if !style.vendor_prefix.is_empty()
      && !last_style_rule.vendor_prefix.is_empty()
      && is_equivalent(&style.selectors.0, &last_style_rule.selectors.0)
    {
      // If the new rule is unprefixed, replace the prefixes of the last rule.
      // Otherwise, add the new prefix.
      if style.vendor_prefix.contains(VendorPrefix::None) && context.targets.current.should_compile_selectors() {
        last_style_rule.vendor_prefix = style.vendor_prefix;
      } else {
        last_style_rule.vendor_prefix |= style.vendor_prefix;
      }
      return true;
    }

    // Append the selectors to the last rule if the declarations are the same, and all selectors are compatible.
    if style.is_compatible(context.targets.current) && last_style_rule.is_compatible(context.targets.current) {
      last_style_rule.selectors.0.extend(style.selectors.0.drain(..));
      if style.vendor_prefix.contains(VendorPrefix::None) && context.targets.current.should_compile_selectors() {
        last_style_rule.vendor_prefix = style.vendor_prefix;
      } else {
        last_style_rule.vendor_prefix |= style.vendor_prefix;
      }
      return true;
    }
  }
  false
}

impl<'a, 'i, T: ToCss> ToCss for CssRuleList<'i, T> {
  fn to_css<W>(&self, dest: &mut Printer<W>) -> Result<(), PrinterError>
  where
    W: std::fmt::Write,
  {
    let mut first = true;
    let mut last_without_block = false;

    for rule in &self.0 {
      if let CssRule::Ignored = &rule {
        continue;
      }

      // Skip @import rules if collecting dependencies.
      if let CssRule::Import(rule) = &rule {
        if dest.remove_imports {
          let dep = if dest.dependencies.is_some() {
            Some(Dependency::Import(ImportDependency::new(&rule, dest.filename())))
          } else {
            None
          };

          if let Some(dependencies) = &mut dest.dependencies {
            dependencies.push(dep.unwrap());
            continue;
          }
        }
      }

      if first {
        first = false;
      } else {
        if !dest.minify
          && !(last_without_block
            && matches!(
              rule,
              CssRule::Import(..) | CssRule::Namespace(..) | CssRule::LayerStatement(..)
            ))
        {
          dest.write_char('\n')?;
        }
        dest.newline()?;
      }
      rule.to_css(dest)?;
      last_without_block = matches!(
        rule,
        CssRule::Import(..) | CssRule::Namespace(..) | CssRule::LayerStatement(..)
      );
    }

    Ok(())
  }
}

impl<'i, T> std::ops::Index<usize> for CssRuleList<'i, T> {
  type Output = CssRule<'i, T>;

  fn index(&self, index: usize) -> &Self::Output {
    &self.0[index]
  }
}

impl<'i, T> std::ops::IndexMut<usize> for CssRuleList<'i, T> {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    &mut self.0[index]
  }
}

/// A hasher that expects to be called with a single u64, which is already a hash.
#[derive(Default)]
struct PrecomputedHasher {
  hash: Option<u64>,
}

impl Hasher for PrecomputedHasher {
  #[inline]
  fn write(&mut self, _: &[u8]) {
    unreachable!()
  }

  #[inline]
  fn write_u64(&mut self, i: u64) {
    debug_assert!(self.hash.is_none());
    self.hash = Some(i);
  }

  #[inline]
  fn finish(&self) -> u64 {
    self.hash.unwrap()
  }
}

/// A key to a StyleRule meant for use in a HashMap for quickly detecting duplicates.
/// It stores a reference to a list and an index so it can access items without cloning
/// even when the list is reallocated. A hash is also pre-computed for fast lookups.
#[derive(Clone)]
pub(crate) struct StyleRuleKey<'a, 'i, R> {
  list: &'a Vec<CssRule<'i, R>>,
  index: usize,
  hash: u64,
}

impl<'a, 'i, R> StyleRuleKey<'a, 'i, R> {
  fn new(list: &'a Vec<CssRule<'i, R>>, index: usize) -> Self {
    let rule = match &list[index] {
      CssRule::Style(style) => style,
      _ => unreachable!(),
    };

    Self {
      list,
      index,
      hash: rule.hash_key(),
    }
  }
}

impl<'a, 'i, R> PartialEq for StyleRuleKey<'a, 'i, R> {
  fn eq(&self, other: &Self) -> bool {
    let rule = match self.list.get(self.index) {
      Some(CssRule::Style(style)) => style,
      _ => return false,
    };

    let other_rule = match other.list.get(other.index) {
      Some(CssRule::Style(style)) => style,
      _ => return false,
    };

    rule.is_duplicate(other_rule)
  }
}

impl<'a, 'i, R> Eq for StyleRuleKey<'a, 'i, R> {}

impl<'a, 'i, R> std::hash::Hash for StyleRuleKey<'a, 'i, R> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    state.write_u64(self.hash);
  }
}
